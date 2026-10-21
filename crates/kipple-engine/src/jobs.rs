// SPDX-License-Identifier: GPL-3.0-or-later
//! The job scheduler (ARCHITECTURE §2, §7.1): runs scans, enrichers, hashing and thumbnails in
//! the background, in priority order, with per-volume concurrency limits. Jobs are cancelled and
//! paused cooperatively: they call [`JobContext::checkpoint`] between units of work (a directory,
//! a batch), which is where a cancel or pause takes effect. A paused job gives its slot back, so
//! other jobs can run meanwhile.
//!
//! Jobs are blocking code (the walker, hashing), so each running job gets its own thread. One
//! more thread delivers [`JobEvent`]s in order and coalesces progress.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kipple_core::identity::VolumeId;
use kipple_core::location::StorageKind;

/// Higher priorities start first: interactive > scan > enrich > hash > thumbnails. Priority only
/// picks the next job to start; a running job is never interrupted for a more urgent one, so
/// low-priority work comes in small batches.
///
/// `Ord` follows urgency: the most urgent sorts first (`Interactive < Scan`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Priority {
    /// The user is waiting for it. Never held back by the concurrency limits: keep these short.
    Interactive,
    Scan,
    Enrich,
    Hash,
    Thumbnails,
}

impl Priority {
    /// Counts toward the global and volume limits.
    const fn limited(self) -> bool {
        !matches!(self, Self::Interactive)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

/// How many jobs may run at once on a volume of this kind. Spinning disks, USB sticks and network
/// shares slow down when reads compete. Most local volumes are `Unknown` until SSD/HDD detection
/// lands, so that gets a middle value.
pub const fn volume_limit(storage: StorageKind) -> usize {
    match storage {
        StorageKind::Ssd => 4,
        StorageKind::Unknown => 2,
        StorageKind::Hdd | StorageKind::Removable | StorageKind::Network => 1,
    }
}

/// What a job needs before it can start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub priority: Priority,
    /// The volume the job reads from, if any: it takes one of that volume's slots.
    pub volume: Option<(VolumeId, StorageKind)>,
}

impl JobSpec {
    pub const fn new(priority: Priority) -> Self {
        Self {
            priority,
            volume: None,
        }
    }

    pub fn on_volume(self, id: VolumeId, storage: StorageKind) -> Self {
        Self {
            volume: Some((id, storage)),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    /// `None` while the total isn't known yet (a scan doesn't know how many files it will find).
    pub total: Option<u64>,
}

/// Why a job stopped early. Failures carry a stable code for the UI (ADR-0004).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Failed(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Waiting for a free slot (also after a resume).
    Queued,
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
    Failed(&'static str),
}

/// The failure code of a job that panicked.
pub const JOB_PANICKED: &str = "job_panicked";
/// The failure code of a job whose thread couldn't be started.
pub const JOB_NOT_STARTED: &str = "job_not_started";

impl From<Result<(), JobError>> for Outcome {
    fn from(result: Result<(), JobError>) -> Self {
        match result {
            Ok(()) => Self::Done,
            Err(JobError::Cancelled) => Self::Cancelled,
            Err(JobError::Failed(code)) => Self::Failed(code),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEvent {
    State(JobId, JobState),
    /// The latest progress of every job that reported since the last one; at most
    /// [`PROGRESS_INTERVAL`] apart. A job's last report may never be sent: `Finished` with
    /// [`Outcome::Done`] means it's complete.
    Progress(Vec<(JobId, Progress)>),
    /// The job is gone from the scheduler. Every job gets one, and no events about it follow.
    Finished(JobId, Outcome),
}

/// Progress events are at least this far apart: 10 per second at most.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStatus {
    pub id: JobId,
    pub priority: Priority,
    pub state: JobState,
    pub progress: Option<Progress>,
}

type Work = Box<dyn FnOnce(&JobContext) -> Result<(), JobError> + Send>;

/// Read by the job without taking the scheduler's lock.
#[derive(Debug, Default)]
struct Flags {
    cancelled: AtomicBool,
    pause: AtomicBool,
}

impl Flags {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    fn paused(&self) -> bool {
        self.pause.load(Ordering::Acquire)
    }

    fn set_paused(&self, paused: bool) {
        self.pause.store(paused, Ordering::Release);
    }
}

struct Job {
    spec: JobSpec,
    state: JobState,
    /// Its thread was started (it may be paused or waiting for a slot again since).
    started: bool,
    /// Taken by the job's thread when it starts.
    work: Option<Work>,
    flags: Arc<Flags>,
    /// Counts toward the global and volume limits (interactive jobs never do).
    holds_slot: bool,
    progress: Option<Progress>,
    /// Reported since the last progress event.
    progress_new: bool,
}

#[derive(Default)]
struct State {
    next_id: u64,
    /// By ID, which is also submission order.
    jobs: BTreeMap<JobId, Job>,
    slots_used: usize,
    volume_slots_used: HashMap<VolumeId, usize>,
    /// State events waiting for the event thread, in order.
    outbox: Vec<JobEvent>,
    /// The work of jobs that ended without running, dropped by the event thread outside the
    /// lock: what a closure holds may call back into the scheduler when it's dropped.
    discarded: Vec<Work>,
    progress_new: bool,
    shutdown: bool,
}

struct Shared {
    state: Mutex<State>,
    /// Job state changes: a paused job waiting to run again, `wait_idle`.
    jobs_changed: Condvar,
    /// Something for the event thread.
    events_ready: Condvar,
    max_jobs: usize,
}

/// What a running job gets to talk to the scheduler.
pub struct JobContext {
    shared: Arc<Shared>,
    id: JobId,
    flags: Arc<Flags>,
}

impl std::fmt::Debug for JobContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobContext").field("id", &self.id).finish()
    }
}

impl JobContext {
    pub fn id(&self) -> JobId {
        self.id
    }

    /// Where a cancel or pause takes effect: returns `Err(Cancelled)` once the job is
    /// cancelled, and blocks while it's paused. Call it between units of work, outside any
    /// index transaction: a paused job keeps its thread and whatever it holds open.
    pub fn checkpoint(&self) -> Result<(), JobError> {
        if self.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        if !self.flags.paused() {
            return Ok(());
        }
        let mut state = self.shared.lock();
        loop {
            if self.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            let Some(job) = state.jobs.get(&self.id) else {
                // The scheduler was dropped.
                return Err(JobError::Cancelled);
            };
            if job.state == JobState::Running {
                // `pause` and `resume` change the flag under the lock: this is the settled
                // answer, also right after a resume.
                if !self.flags.paused() {
                    return Ok(());
                }
                state.set_state(self.id, JobState::Paused);
                state.release_slot(self.id);
                self.shared.pump(&mut state);
            }
            state = wait(&self.shared.jobs_changed, state);
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.flags.cancelled()
    }

    /// Takes the scheduler's lock: report per directory or batch, not per file.
    pub fn progress(&self, done: u64, total: Option<u64>) {
        let mut state = self.shared.lock();
        let Some(job) = state.jobs.get_mut(&self.id) else {
            return;
        };
        job.progress = Some(Progress { done, total });
        job.progress_new = true;
        if !state.progress_new {
            state.progress_new = true;
            self.shared.events_ready.notify_one();
        }
    }
}

pub struct Scheduler {
    shared: Arc<Shared>,
    events: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scheduler")
            .field("max_jobs", &self.shared.max_jobs)
            .finish_non_exhaustive()
    }
}

impl Scheduler {
    /// A scheduler running at most `max_jobs` non-interactive jobs at once. `sink` receives
    /// every [`JobEvent`] in order, on the scheduler's own event thread. It must not block on
    /// the thread that drops the scheduler: dropping waits for the last events to be delivered.
    pub fn new(max_jobs: usize, sink: impl Fn(JobEvent) + Send + 'static) -> io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            jobs_changed: Condvar::new(),
            events_ready: Condvar::new(),
            max_jobs: max_jobs.max(1),
        });
        let events = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("kipple-job-events".into())
                .spawn(move || deliver_events(&shared, &sink))?
        };
        Ok(Self {
            shared,
            events: Some(events),
        })
    }

    pub fn submit(
        &self,
        spec: JobSpec,
        work: impl FnOnce(&JobContext) -> Result<(), JobError> + Send + 'static,
    ) -> JobId {
        let mut state = self.shared.lock();
        state.next_id += 1;
        let id = JobId(state.next_id);
        state.jobs.insert(
            id,
            Job {
                spec,
                state: JobState::Queued,
                started: false,
                work: Some(Box::new(work)),
                flags: Arc::default(),
                holds_slot: false,
                progress: None,
                progress_new: false,
            },
        );
        state.outbox.push(JobEvent::State(id, JobState::Queued));
        self.shared.pump(&mut state);
        id
    }

    /// Whether `id` was still there to cancel.
    pub fn cancel(&self, id: JobId) -> bool {
        let mut state = self.shared.lock();
        let Some(job) = state.jobs.get(&id) else {
            return false;
        };
        job.flags.cancel();
        if job.started {
            // It stops at its next checkpoint; a paused one wakes up to stop.
            self.shared.jobs_changed.notify_all();
        } else {
            self.shared.finish(&mut state, id, Outcome::Cancelled);
        }
        true
    }

    /// A running job pauses at its next checkpoint; a queued one isn't started.
    pub fn pause(&self, id: JobId) -> bool {
        let mut state = self.shared.lock();
        let Some(job) = state.jobs.get(&id) else {
            return false;
        };
        if job.flags.cancelled() {
            return true;
        }
        job.flags.set_paused(true);
        if job.state == JobState::Queued {
            state.set_state(id, JobState::Paused);
            self.shared.events_ready.notify_one();
        }
        true
    }

    pub fn resume(&self, id: JobId) -> bool {
        let mut state = self.shared.lock();
        let Some(job) = state.jobs.get(&id) else {
            return false;
        };
        if job.flags.cancelled() {
            return true;
        }
        job.flags.set_paused(false);
        if job.state == JobState::Paused {
            state.set_state(id, JobState::Queued);
            self.shared.pump(&mut state);
        }
        true
    }

    pub fn status(&self) -> Vec<JobStatus> {
        let state = self.shared.lock();
        state
            .jobs
            .iter()
            .map(|(&id, job)| JobStatus {
                id,
                priority: job.spec.priority,
                state: job.state,
                progress: job.progress,
            })
            .collect()
    }

    /// Waits until no job is left (paused jobs count). False if `timeout` passed first.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.shared.lock();
        while !state.jobs.is_empty() {
            match deadline {
                None => state = wait(&self.shared.jobs_changed, state),
                Some(deadline) => {
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        return false;
                    };
                    state = wait_timeout(&self.shared.jobs_changed, state, left);
                }
            }
        }
        true
    }
}

impl Drop for Scheduler {
    /// Cancels every job and delivers its `Finished` event. Running jobs stop at their next
    /// checkpoint, in the background: to let them end first, cancel them and [`wait_idle`].
    ///
    /// [`wait_idle`]: Scheduler::wait_idle
    fn drop(&mut self) {
        let mut state = self.shared.lock();
        state.shutdown = true;
        // Job threads still running find their job gone, and end without a second `Finished`.
        let jobs = std::mem::take(&mut state.jobs);
        state.slots_used = 0;
        state.volume_slots_used.clear();
        for (id, job) in jobs {
            job.flags.cancel();
            state.discarded.extend(job.work);
            state
                .outbox
                .push(JobEvent::Finished(id, Outcome::Cancelled));
        }
        self.shared.jobs_changed.notify_all();
        self.shared.events_ready.notify_one();
        drop(state);
        if let Some(events) = self.events.take()
            // The sink dropped the last owner: it can't wait for itself.
            && events.thread().id() != std::thread::current().id()
        {
            let _ = events.join();
        }
        let discarded = std::mem::take(&mut self.shared.lock().discarded);
        drop(discarded);
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts (or lets continue) the queued jobs that fit, best first: by priority, then in
    /// submission order. A job that doesn't fit doesn't hold back the ones behind it.
    fn pump(self: &Arc<Self>, state: &mut State) {
        while !state.shutdown {
            let next = state
                .jobs
                .iter()
                .filter(|(_, job)| {
                    job.state == JobState::Queued
                        && !job.flags.cancelled()
                        && state.fits(job, self.max_jobs)
                })
                .min_by_key(|(id, job)| (job.spec.priority, **id))
                .map(|(id, _)| *id);
            let Some(id) = next else {
                break;
            };
            state.set_state(id, JobState::Running);
            state.take_slot(id);
            let job = state.job_mut(id);
            if job.started {
                // A resumed job, waiting in its checkpoint.
                self.jobs_changed.notify_all();
                continue;
            }
            job.started = true;
            let ctx = JobContext {
                shared: Arc::clone(self),
                id,
                flags: Arc::clone(&job.flags),
            };
            let spawned = std::thread::Builder::new()
                .name(format!("kipple-job-{}", id.0))
                .spawn(move || run(ctx));
            if spawned.is_err() {
                // Out of threads: the rest waits until a job ends or another is submitted.
                self.finish(state, id, Outcome::Failed(JOB_NOT_STARTED));
                break;
            }
        }
        // Callers queue events before pumping, too.
        if !state.outbox.is_empty() {
            self.events_ready.notify_one();
        }
    }

    /// Removes the job and frees its slot. The caller pumps the next one in.
    fn finish(&self, state: &mut State, id: JobId, outcome: Outcome) {
        state.release_slot(id);
        // Gone already if the scheduler was dropped while the job ran.
        let Some(job) = state.jobs.remove(&id) else {
            return;
        };
        state.discarded.extend(job.work);
        state.outbox.push(JobEvent::Finished(id, outcome));
        // Paused jobs wait for `pump` or `cancel`; only `wait_idle` waits for this.
        if state.jobs.is_empty() {
            self.jobs_changed.notify_all();
        }
        self.events_ready.notify_one();
    }
}

impl State {
    fn job_mut(&mut self, id: JobId) -> &mut Job {
        self.jobs.get_mut(&id).expect("a known job")
    }

    fn set_state(&mut self, id: JobId, to: JobState) {
        self.job_mut(id).state = to;
        self.outbox.push(JobEvent::State(id, to));
    }

    fn fits(&self, job: &Job, max_jobs: usize) -> bool {
        if !job.spec.priority.limited() {
            return true;
        }
        self.slots_used < max_jobs
            && job.spec.volume.as_ref().is_none_or(|(volume, storage)| {
                self.volume_slots_used.get(volume).copied().unwrap_or(0) < volume_limit(*storage)
            })
    }

    fn take_slot(&mut self, id: JobId) {
        let job = self.job_mut(id);
        if !job.spec.priority.limited() {
            return;
        }
        job.holds_slot = true;
        let volume = job.spec.volume.as_ref().map(|(volume, _)| volume.clone());
        self.slots_used += 1;
        if let Some(volume) = volume {
            *self.volume_slots_used.entry(volume).or_default() += 1;
        }
    }

    fn release_slot(&mut self, id: JobId) {
        let Some(job) = self.jobs.get_mut(&id) else {
            return;
        };
        if !std::mem::take(&mut job.holds_slot) {
            return;
        }
        self.slots_used -= 1;
        if let Some((volume, _)) = &job.spec.volume
            && let Some(used) = self.volume_slots_used.get_mut(volume)
        {
            *used -= 1;
            if *used == 0 {
                self.volume_slots_used.remove(volume);
            }
        }
    }

    /// The latest progress of every job that reported since the last call.
    fn take_progress(&mut self) -> Vec<(JobId, Progress)> {
        self.progress_new = false;
        self.jobs
            .iter_mut()
            .filter_map(|(id, job)| std::mem::take(&mut job.progress_new).then_some((id, job)))
            .filter_map(|(&id, job)| Some((id, job.progress?)))
            .collect()
    }
}

fn wait<'a>(condvar: &Condvar, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
    condvar.wait(guard).unwrap_or_else(PoisonError::into_inner)
}

fn wait_timeout<'a>(
    condvar: &Condvar,
    guard: MutexGuard<'a, State>,
    timeout: Duration,
) -> MutexGuard<'a, State> {
    condvar
        .wait_timeout(guard, timeout)
        .unwrap_or_else(PoisonError::into_inner)
        .0
}

fn run(ctx: JobContext) {
    let work = ctx
        .shared
        .lock()
        .jobs
        .get_mut(&ctx.id)
        .and_then(|job| job.work.take());
    // None: the scheduler was dropped before the thread got going.
    let Some(work) = work else {
        return;
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // Cancelled between being started and getting its thread: don't do a first unit.
        ctx.checkpoint()?;
        work(&ctx)
    }))
    .map_or(Outcome::Failed(JOB_PANICKED), Outcome::from);
    let mut state = ctx.shared.lock();
    ctx.shared.finish(&mut state, ctx.id, outcome);
    ctx.shared.pump(&mut state);
}

/// The event thread: delivers state events as they happen and progress at most every
/// [`PROGRESS_INTERVAL`]. A job's progress ends with its `Finished` event, since the job is
/// removed under the same lock. After a shutdown it delivers what's left, then ends.
fn deliver_events(shared: &Shared, sink: &dyn Fn(JobEvent)) {
    let mut last_progress: Option<Instant> = None;
    let mut state = shared.lock();
    loop {
        let next_progress = last_progress.map(|at| at + PROGRESS_INTERVAL);
        let progress_due =
            state.progress_new && next_progress.is_none_or(|next| Instant::now() >= next);
        if state.outbox.is_empty() && !progress_due {
            if state.shutdown {
                return;
            }
            state = match next_progress {
                Some(next) if state.progress_new => {
                    let left = next.saturating_duration_since(Instant::now());
                    wait_timeout(&shared.events_ready, state, left)
                }
                _ => wait(&shared.events_ready, state),
            };
            continue;
        }
        let mut events = std::mem::take(&mut state.outbox);
        let discarded = std::mem::take(&mut state.discarded);
        if progress_due {
            let batch = state.take_progress();
            if !batch.is_empty() {
                events.push(JobEvent::Progress(batch));
                last_progress = Some(Instant::now());
            }
        }
        drop(state);
        // What a closure holds runs code when dropped: a panic there mustn't stop the events.
        let _ = catch_unwind(AssertUnwindSafe(|| drop(discarded)));
        for event in events {
            // A panicking sink loses that one event, not all the ones after it.
            let _ = catch_unwind(AssertUnwindSafe(|| sink(event)));
        }
        state = shared.lock();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use super::*;

    const WAIT: Duration = Duration::from_secs(10);

    type Events = Arc<Mutex<Vec<JobEvent>>>;

    fn scheduler(max_jobs: usize) -> (Scheduler, Events) {
        let events = Events::default();
        let sink = Arc::clone(&events);
        let s = Scheduler::new(max_jobs, move |e| sink.lock().unwrap().push(e)).unwrap();
        (s, events)
    }

    /// Polls until `cond` holds; panics after [`WAIT`].
    fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
        let start = Instant::now();
        while !cond() {
            assert!(start.elapsed() < WAIT, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn state_of(s: &Scheduler, id: JobId) -> Option<JobState> {
        s.status().into_iter().find(|j| j.id == id).map(|j| j.state)
    }

    fn finished(events: &Events, id: JobId) -> Option<Outcome> {
        events.lock().unwrap().iter().find_map(|e| match e {
            JobEvent::Finished(j, o) if *j == id => Some(*o),
            _ => None,
        })
    }

    /// A job that runs until the test sends it something; it checkpoints after every message.
    fn gated() -> (Sender<()>, impl FnOnce(&JobContext) -> Result<(), JobError>) {
        let (tx, rx) = mpsc::channel::<()>();
        let work = move |ctx: &JobContext| {
            while rx.recv().is_ok() {
                ctx.checkpoint()?;
            }
            Ok(())
        };
        (tx, work)
    }

    /// `work`, telling the receiver when it starts. A job is `Running` before its thread
    /// passes the checkpoint ahead of the work: a pause or cancel then stops it right there.
    fn announced(
        work: impl FnOnce(&JobContext) -> Result<(), JobError>,
    ) -> (
        Receiver<()>,
        impl FnOnce(&JobContext) -> Result<(), JobError>,
    ) {
        let (tx, rx) = mpsc::channel();
        let work = move |ctx: &JobContext| {
            tx.send(()).unwrap();
            work(ctx)
        };
        (rx, work)
    }

    /// A job that records its name in `log` and ends.
    fn logs(
        log: &Arc<Mutex<Vec<&'static str>>>,
        name: &'static str,
    ) -> impl FnOnce(&JobContext) -> Result<(), JobError> + use<> {
        let log = Arc::clone(log);
        move |_| {
            log.lock().unwrap().push(name);
            Ok(())
        }
    }

    fn ssd(name: &str) -> (VolumeId, StorageKind) {
        (VolumeId(name.into()), StorageKind::Ssd)
    }

    #[test]
    fn slower_volumes_get_fewer_slots() {
        assert_eq!(volume_limit(StorageKind::Ssd), 4);
        assert_eq!(volume_limit(StorageKind::Unknown), 2);
        assert_eq!(volume_limit(StorageKind::Network), 1);
        assert_eq!(volume_limit(StorageKind::Hdd), 1);
        assert_eq!(volume_limit(StorageKind::Removable), 1);
    }

    #[test]
    fn queued_jobs_start_by_priority_then_in_order() {
        let (s, _) = scheduler(1);
        let (gate, blocker) = gated();
        let blocker = s.submit(JobSpec::new(Priority::Scan), blocker);
        wait_for("the blocker to run", || {
            state_of(&s, blocker) == Some(JobState::Running)
        });

        let log = Arc::default();
        s.submit(JobSpec::new(Priority::Thumbnails), logs(&log, "thumbs"));
        s.submit(JobSpec::new(Priority::Hash), logs(&log, "hash"));
        s.submit(JobSpec::new(Priority::Enrich), logs(&log, "enrich 1"));
        s.submit(JobSpec::new(Priority::Scan), logs(&log, "scan"));
        s.submit(JobSpec::new(Priority::Enrich), logs(&log, "enrich 2"));
        assert!(log.lock().unwrap().is_empty(), "one slot, and it's taken");

        drop(gate);
        assert!(s.wait_idle(WAIT));
        assert_eq!(
            *log.lock().unwrap(),
            ["scan", "enrich 1", "enrich 2", "hash", "thumbs"]
        );
    }

    #[test]
    fn interactive_jobs_never_wait_for_a_slot() {
        let (s, events) = scheduler(1);
        let (gate, blocker) = gated();
        let (volume, storage) = (VolumeId("usb".into()), StorageKind::Removable);
        let scan = s.submit(
            JobSpec::new(Priority::Scan).on_volume(volume.clone(), storage),
            blocker,
        );
        wait_for("the scan to run", || {
            state_of(&s, scan) == Some(JobState::Running)
        });
        let log = Arc::default();
        let quick = s.submit(
            JobSpec::new(Priority::Interactive).on_volume(volume, storage),
            logs(&log, "quick"),
        );
        wait_for("the interactive job", || {
            finished(&events, quick) == Some(Outcome::Done)
        });
        assert_eq!(state_of(&s, scan), Some(JobState::Running));
        drop(gate);
        assert!(s.wait_idle(WAIT));
    }

    #[test]
    fn jobs_on_one_volume_share_its_slots() {
        let (s, _) = scheduler(8);
        let mut gates = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..6 {
            let (gate, work) = gated();
            let (id, kind) = ssd("internal");
            ids.push(s.submit(JobSpec::new(Priority::Scan).on_volume(id, kind), work));
            gates.push(gate);
        }
        // Another volume has slots of its own.
        let (gate, work) = gated();
        let usb = s.submit(
            JobSpec::new(Priority::Scan).on_volume(VolumeId("usb".into()), StorageKind::Removable),
            work,
        );
        gates.push(gate);
        let (gate, work) = gated();
        let usb_2 = s.submit(
            JobSpec::new(Priority::Scan).on_volume(VolumeId("usb".into()), StorageKind::Removable),
            work,
        );
        gates.push(gate);

        wait_for("the usb job", || {
            state_of(&s, usb) == Some(JobState::Running)
        });
        wait_for("four ssd jobs", || {
            ids.iter()
                .filter(|&&id| state_of(&s, id) == Some(JobState::Running))
                .count()
                == 4
        });
        let states: Vec<_> = ids.iter().map(|&id| state_of(&s, id).unwrap()).collect();
        assert_eq!(states[4..], [JobState::Queued, JobState::Queued]);
        assert_eq!(state_of(&s, usb_2), Some(JobState::Queued));

        drop(gates);
        assert!(s.wait_idle(WAIT));
    }

    #[test]
    fn the_global_limit_holds_across_volumes() {
        let (s, _) = scheduler(2);
        let mut gates = Vec::new();
        let mut ids = Vec::new();
        for name in ["a", "b", "c"] {
            let (gate, work) = gated();
            let (id, kind) = ssd(name);
            ids.push(s.submit(JobSpec::new(Priority::Scan).on_volume(id, kind), work));
            gates.push(gate);
        }
        // No volume: still takes a global slot.
        let (gate, work) = gated();
        ids.push(s.submit(JobSpec::new(Priority::Hash), work));
        gates.push(gate);

        wait_for("two jobs", || {
            s.status()
                .iter()
                .filter(|j| j.state == JobState::Running)
                .count()
                == 2
        });
        let running = s
            .status()
            .iter()
            .filter(|j| j.state == JobState::Running)
            .count();
        assert_eq!(running, 2);
        drop(gates);
        assert!(s.wait_idle(WAIT));
    }

    #[test]
    fn cancelling_a_queued_job_means_it_never_runs() {
        let (s, events) = scheduler(1);
        let (gate, blocker) = gated();
        s.submit(JobSpec::new(Priority::Scan), blocker);
        let log = Arc::default();
        let queued = s.submit(JobSpec::new(Priority::Scan), logs(&log, "queued"));

        assert!(s.cancel(queued));
        assert!(!s.cancel(queued), "it's gone");
        assert_eq!(state_of(&s, queued), None);
        drop(gate);
        assert!(s.wait_idle(WAIT));
        assert!(log.lock().unwrap().is_empty());
        wait_for("the event", || finished(&events, queued).is_some());
        assert_eq!(finished(&events, queued), Some(Outcome::Cancelled));
    }

    #[test]
    fn a_running_job_stops_at_its_next_checkpoint() {
        let (s, events) = scheduler(1);
        let (gate, work) = gated();
        let (started, work) = announced(work);
        let id = s.submit(JobSpec::new(Priority::Scan), work);
        started.recv_timeout(WAIT).unwrap();

        assert!(s.cancel(id));
        gate.send(()).unwrap();
        assert!(s.wait_idle(WAIT));
        wait_for("the event", || finished(&events, id).is_some());
        assert_eq!(finished(&events, id), Some(Outcome::Cancelled));
    }

    #[test]
    fn a_paused_job_gives_its_slot_back_until_resumed() {
        let (s, events) = scheduler(1);
        let (gate, work) = gated();
        let (started, work) = announced(work);
        let a = s.submit(JobSpec::new(Priority::Scan), work);
        started.recv_timeout(WAIT).unwrap();
        let log = Arc::default();
        let b = s.submit(JobSpec::new(Priority::Enrich), logs(&log, "b"));

        assert!(s.pause(a));
        // Pausing takes effect at a's next checkpoint.
        assert_eq!(state_of(&s, a), Some(JobState::Running));
        gate.send(()).unwrap();
        wait_for("b to finish", || {
            finished(&events, b) == Some(Outcome::Done)
        });
        assert_eq!(state_of(&s, a), Some(JobState::Paused));
        assert!(!s.wait_idle(Duration::from_millis(20)), "a is still there");

        assert!(s.resume(a));
        drop(gate);
        assert!(s.wait_idle(WAIT));
        wait_for("a's end", || finished(&events, a).is_some());
        assert_eq!(finished(&events, a), Some(Outcome::Done));

        let states: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                JobEvent::State(id, state) if *id == a => Some(*state),
                _ => None,
            })
            .collect();
        assert_eq!(
            states,
            [
                JobState::Queued,
                JobState::Running,
                JobState::Paused,
                JobState::Queued,
                JobState::Running
            ]
        );
    }

    #[test]
    fn a_resumed_job_waits_for_a_slot_like_any_other() {
        let (s, _) = scheduler(1);
        let (gate_a, work) = gated();
        let a = s.submit(JobSpec::new(Priority::Scan), work);
        wait_for("a to run", || state_of(&s, a) == Some(JobState::Running));
        assert!(s.pause(a));
        gate_a.send(()).unwrap();
        wait_for("a to pause", || state_of(&s, a) == Some(JobState::Paused));

        let (gate_b, work) = gated();
        let b = s.submit(JobSpec::new(Priority::Scan), work);
        wait_for("b to run", || state_of(&s, b) == Some(JobState::Running));
        assert!(s.resume(a));
        assert_eq!(state_of(&s, a), Some(JobState::Queued));

        drop(gate_b);
        wait_for("a to run again", || {
            state_of(&s, a) == Some(JobState::Running)
        });
        drop(gate_a);
        assert!(s.wait_idle(WAIT));
    }

    #[test]
    fn a_paused_queued_job_is_held_back() {
        let (s, _) = scheduler(1);
        let log = Arc::default();
        let (gate, blocker) = gated();
        s.submit(JobSpec::new(Priority::Scan), blocker);
        let held = s.submit(JobSpec::new(Priority::Scan), logs(&log, "held"));
        let other = s.submit(JobSpec::new(Priority::Hash), logs(&log, "other"));
        assert!(s.pause(held));
        assert_eq!(state_of(&s, held), Some(JobState::Paused));

        drop(gate);
        wait_for("other to run", || state_of(&s, other).is_none());
        assert_eq!(*log.lock().unwrap(), ["other"]);
        assert_eq!(state_of(&s, held), Some(JobState::Paused));

        assert!(s.resume(held));
        assert!(s.wait_idle(WAIT));
        assert_eq!(*log.lock().unwrap(), ["other", "held"]);
    }

    #[test]
    fn cancelling_a_paused_job_wakes_it_up_to_stop() {
        let (s, events) = scheduler(1);
        let (gate, work) = gated();
        let id = s.submit(JobSpec::new(Priority::Scan), work);
        wait_for("it to run", || state_of(&s, id) == Some(JobState::Running));
        assert!(s.pause(id));
        gate.send(()).unwrap();
        wait_for("it to pause", || state_of(&s, id) == Some(JobState::Paused));

        assert!(s.cancel(id));
        assert!(s.wait_idle(WAIT));
        wait_for("the event", || finished(&events, id).is_some());
        assert_eq!(finished(&events, id), Some(Outcome::Cancelled));
    }

    #[test]
    fn unknown_jobs_cannot_be_controlled() {
        let (s, _) = scheduler(1);
        assert!(!s.cancel(JobId(42)));
        assert!(!s.pause(JobId(42)));
        assert!(!s.resume(JobId(42)));
    }

    #[test]
    fn failures_and_panics_are_reported_and_free_the_slot() {
        let (s, events) = scheduler(1);
        let failing = s.submit(JobSpec::new(Priority::Scan), |_| {
            Err(JobError::Failed("scan_unreadable"))
        });
        let panicking = s.submit(JobSpec::new(Priority::Scan), |_| panic!("boom"));
        let log = Arc::default();
        let after = s.submit(JobSpec::new(Priority::Scan), logs(&log, "after"));
        assert!(s.wait_idle(WAIT));
        wait_for("the events", || finished(&events, after).is_some());
        assert_eq!(
            finished(&events, failing),
            Some(Outcome::Failed("scan_unreadable"))
        );
        assert_eq!(
            finished(&events, panicking),
            Some(Outcome::Failed(JOB_PANICKED))
        );
        assert_eq!(finished(&events, after), Some(Outcome::Done));
    }

    #[test]
    fn progress_is_coalesced_to_ten_events_a_second() {
        let (s, events) = scheduler(1);
        let (tx, rx): (Sender<()>, Receiver<()>) = mpsc::channel();
        let id = s.submit(JobSpec::new(Priority::Scan), move |ctx| {
            let start = Instant::now();
            let mut done = 0;
            while start.elapsed() < Duration::from_millis(500) {
                done += 1;
                ctx.progress(done, None);
            }
            ctx.progress(done, Some(done));
            // Stay running so the last value gets out.
            let _ = rx.recv();
            Ok(())
        });
        let last = |events: &Events| {
            events.lock().unwrap().iter().rev().find_map(|e| match e {
                JobEvent::Progress(batch) => batch.iter().find(|(j, _)| *j == id).map(|p| p.1),
                _ => None,
            })
        };
        wait_for("the final progress", || {
            last(&events).is_some_and(|p| p.total.is_some())
        });
        let p = last(&events).unwrap();
        assert_eq!(p.total, Some(p.done));
        assert_eq!(
            s.status().iter().find(|j| j.id == id).unwrap().progress,
            Some(p)
        );
        drop(tx);
        assert!(s.wait_idle(WAIT));

        let count = events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| matches!(e, JobEvent::Progress(_)))
            .count();
        // At least 100 ms apart, and all but the last one within the 500 ms of reports.
        assert!((1..=7).contains(&count), "{count} progress events");
        assert!(events.lock().unwrap().iter().all(|e| match e {
            JobEvent::Progress(batch) => !batch.is_empty(),
            _ => true,
        }));
    }

    #[test]
    fn no_progress_follows_the_end_of_a_job() {
        let (s, events) = scheduler(1);
        let (go, wait_go) = mpsc::channel::<()>();
        let id = s.submit(JobSpec::new(Priority::Scan), move |ctx| {
            ctx.progress(1, None);
            let _ = wait_go.recv();
            // Not due yet when the job ends: it must not come after `Finished`.
            ctx.progress(2, None);
            Ok(())
        });
        let progressed = |events: &Events| {
            events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, JobEvent::Progress(_)))
        };
        wait_for("the first progress", || progressed(&events));
        go.send(()).unwrap();
        assert!(s.wait_idle(WAIT));
        wait_for("the end", || finished(&events, id).is_some());
        std::thread::sleep(PROGRESS_INTERVAL * 2);
        let events = events.lock().unwrap();
        let end = events
            .iter()
            .position(|e| matches!(e, JobEvent::Finished(..)))
            .unwrap();
        assert!(
            events[end..]
                .iter()
                .all(|e| !matches!(e, JobEvent::Progress(_)))
        );
    }

    #[test]
    fn a_cancelled_paused_job_cannot_be_resumed() {
        let (s, events) = scheduler(1);
        let (gate, work) = gated();
        let id = s.submit(JobSpec::new(Priority::Scan), work);
        wait_for("it to run", || state_of(&s, id) == Some(JobState::Running));
        assert!(s.pause(id));
        gate.send(()).unwrap();
        let paused = JobEvent::State(id, JobState::Paused);
        wait_for("the paused event", || {
            events.lock().unwrap().contains(&paused)
        });
        let before = events.lock().unwrap().len();

        assert!(s.cancel(id));
        // False if the woken job already stopped and is gone: either way it stays cancelled.
        s.resume(id);
        assert!(s.wait_idle(WAIT));
        wait_for("the end", || finished(&events, id).is_some());
        assert_eq!(
            events.lock().unwrap()[before..],
            [JobEvent::Finished(id, Outcome::Cancelled)]
        );
    }

    #[test]
    fn a_paused_job_gives_its_volume_slot_back() {
        let (s, _) = scheduler(8);
        let hdd =
            || JobSpec::new(Priority::Scan).on_volume(VolumeId("hdd".into()), StorageKind::Hdd);
        let (gate, work) = gated();
        let a = s.submit(hdd(), work);
        wait_for("a to run", || state_of(&s, a) == Some(JobState::Running));
        let log = Arc::default();
        let b = s.submit(hdd(), logs(&log, "b"));
        assert_eq!(state_of(&s, b), Some(JobState::Queued));

        assert!(s.pause(a));
        gate.send(()).unwrap();
        wait_for("b to run", || state_of(&s, b).is_none());
        assert!(s.resume(a));
        drop(gate);
        assert!(s.wait_idle(WAIT));
    }

    #[test]
    fn dropping_the_scheduler_ends_every_job() {
        let (s, events) = scheduler(1);
        let (gate, work) = gated();
        let running = s.submit(JobSpec::new(Priority::Scan), work);
        wait_for("it to run", || {
            state_of(&s, running) == Some(JobState::Running)
        });
        let (_paused_gate, work) = gated();
        let paused = s.submit(JobSpec::new(Priority::Scan), work);
        assert!(s.pause(paused));
        let (_queued_gate, work) = gated();
        let queued = s.submit(JobSpec::new(Priority::Scan), work);

        drop(s);
        let finished: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                JobEvent::Finished(id, outcome) => Some((*id, *outcome)),
                _ => None,
            })
            .collect();
        assert_eq!(
            finished,
            [
                (running, Outcome::Cancelled),
                (paused, Outcome::Cancelled),
                (queued, Outcome::Cancelled)
            ]
        );
        // The running job sees the cancel at its next checkpoint.
        let _ = gate.send(());
    }

    #[test]
    fn work_is_dropped_outside_the_lock() {
        /// Asks the scheduler for its status when dropped.
        struct CallsBack(Arc<Mutex<Option<std::sync::Weak<Scheduler>>>>, Sender<()>);
        impl Drop for CallsBack {
            fn drop(&mut self) {
                if let Some(s) = self.0.lock().unwrap().as_ref().and_then(|s| s.upgrade()) {
                    s.status();
                }
                let _ = self.1.send(());
            }
        }

        let (s, _) = scheduler(1);
        let s = Arc::new(s);
        let (gate, blocker) = gated();
        s.submit(JobSpec::new(Priority::Scan), blocker);
        let weak = Arc::new(Mutex::new(Some(Arc::downgrade(&s))));
        let (dropped, was_dropped) = mpsc::channel();
        let guard = CallsBack(weak, dropped);
        let queued = s.submit(JobSpec::new(Priority::Scan), move |_| {
            let _guard = guard;
            Ok(())
        });

        let (done, cancelled) = mpsc::channel();
        let canceller = Arc::clone(&s);
        std::thread::spawn(move || done.send(canceller.cancel(queued)));
        assert_eq!(cancelled.recv_timeout(WAIT), Ok(true), "cancel deadlocked");
        assert_eq!(was_dropped.recv_timeout(WAIT), Ok(()));
        drop(gate);
        assert!(s.wait_idle(WAIT));
    }

    #[test]
    fn a_panicking_sink_misses_only_that_event() {
        let events = Events::default();
        let sink = Arc::clone(&events);
        let s = Scheduler::new(1, move |e| {
            if matches!(e, JobEvent::State(JobId(1), JobState::Queued)) {
                panic!("sink failure");
            }
            sink.lock().unwrap().push(e);
        })
        .unwrap();
        let id = s.submit(JobSpec::new(Priority::Scan), |_| Ok(()));
        assert!(s.wait_idle(WAIT));
        wait_for("the end", || finished(&events, id) == Some(Outcome::Done));
    }
}
