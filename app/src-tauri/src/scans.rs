// SPDX-License-Identifier: GPL-3.0-or-later
//! Scans (K-2.6): the `scan_*` commands and the `ScanProgress` event. Each scan is a job on the
//! scheduler (kipple-engine::jobs) with its own index.db connection, so browsing and location
//! changes go on while it runs. A scan that's done is followed by an enrich run of its Location
//! (K-2.7): low-priority jobs the UI doesn't see, one slice of the backlog each, so a scan
//! queued meanwhile goes first. The scan controls act on them too.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::Duration;

use kipple_core::item::ItemId;
use kipple_core::location::{Location, LocationId};
use kipple_engine::enrich::{EnrichError, EnrichPorts, enrich};
use kipple_engine::jobs::{
    JobContext, JobError, JobEvent, JobId, JobSpec, JobState, Outcome, Priority, Scheduler,
};
use kipple_engine::scan::{ScanError, ScanOptions, ScanPorts, ScanSummary, scan, walker_threads};
use kipple_index::IndexDb;
use kipple_platform::{
    Directories, DownloadOrigins, NativeDirectories, NativeDownloadOrigins, NativePlaceholders,
    NativeScreenshots, NativeVolumes, Placeholders, ProtectedPaths, Screenshots, Volumes,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::ipc::Coded;
use crate::locations::{LocationsState, now};

/// Scans and enrich runs at once, across volumes (each volume has its own, lower limit).
const MAX_JOBS: usize = 4;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ScanPhase {
    #[default]
    Queued,
    Running,
    Paused,
    Done,
    Cancelled,
    Failed,
}

impl ScanPhase {
    fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Paused)
    }
}

/// A Location's current scan, or its last one. Counts are `f64`: specta has no 64-bit integers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
pub struct ScanView {
    pub location_id: u32,
    pub phase: ScanPhase,
    /// Items seen so far.
    pub items: f64,
    pub added: f64,
    pub updated: f64,
    pub removed: f64,
    /// Folders that couldn't be read (their Items are kept).
    pub unreadable: f64,
    /// Why it failed, as a code (`scan_offline`, `scan_index_failed`…).
    pub failure: Option<String>,
}

impl ScanView {
    fn queued(location: LocationId) -> Self {
        Self {
            location_id: u32::try_from(location.0).unwrap_or(u32::MAX),
            ..Self::default()
        }
    }

    fn record(&mut self, summary: &ScanSummary) {
        self.items = summary.items as f64;
        self.added = summary.added as f64;
        self.updated = summary.updated as f64;
        self.removed = summary.removed as f64;
        self.unreadable = summary.unreadable as f64;
    }
}

/// Sent whenever scans change state, and with progress at most 10 times a second.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct ScanProgress {
    pub scans: Vec<ScanView>,
}

/// Which job scans or enriches which Location, and every Location's latest scan. Locked by the
/// scheduler's event sink too: holding it across `submit` means no event about a job is handled
/// before the job is in here.
#[derive(Debug, Default)]
struct Book {
    jobs: BTreeMap<JobId, Location>,
    scans: BTreeMap<LocationId, ScanView>,
    enriching: BTreeMap<JobId, Enriching>,
    /// Jobs asked to stop: no enrich run follows them, even one that got to the end first.
    stopping: BTreeSet<JobId>,
    /// Between pause and resume: an enrich run queued meanwhile is paused too.
    paused: bool,
}

/// An enrich run (one slice of a pass), queued or running. One per Location at most. When it's
/// done with more to do, the next slice goes on from `next`. A scan that's done while it runs,
/// or before a slice after the first starts, asks for a new pass after it (`again`), since the
/// pass may have paged past what changed.
#[derive(Debug)]
struct Enriching {
    location: Location,
    /// Where the slice starts.
    from: Option<ItemId>,
    started: bool,
    again: bool,
    /// Where the next slice starts, once this one is done.
    next: Option<ItemId>,
}

impl Book {
    /// The Location to enrich after `event`, and from where, if any: a scan's, once it's done
    /// (unless a first slice for it is still queued: that one sees the changes too); the next
    /// slice after one that's done; or a new pass after a run that's done or failed, when a scan
    /// asked. Call before [`Book::apply`], which forgets finished jobs.
    fn enrich_after(&mut self, event: &JobEvent) -> Option<(Location, Option<ItemId>)> {
        match event {
            JobEvent::State(job, JobState::Running) => {
                if let Some(run) = self.enriching.get_mut(job) {
                    run.started = true;
                }
                None
            }
            JobEvent::Finished(job, outcome) => {
                // Also what keeps a forgotten Location from being enriched again.
                let stopped = self.stopping.remove(job);
                if let Some(run) = self.enriching.remove(job) {
                    return match outcome {
                        _ if stopped => None,
                        Outcome::Cancelled => None,
                        _ if run.again => Some((run.location, None)),
                        Outcome::Done => run.next.map(|next| (run.location, Some(next))),
                        Outcome::Failed(_) => None,
                    };
                }
                if *outcome != Outcome::Done || stopped {
                    return None;
                }
                let location = self.jobs.get(job)?;
                match self
                    .enriching
                    .values_mut()
                    .find(|run| run.location.id == location.id)
                {
                    Some(run) => {
                        run.again |= run.started || run.from.is_some();
                        None
                    }
                    None => Some((location.clone(), None)),
                }
            }
            _ => None,
        }
    }

    /// Every scan and enrich job, or only `location`'s.
    fn job_ids(&self, location: Option<LocationId>) -> Vec<JobId> {
        let scans = self.jobs.iter().map(|(job, l)| (*job, l.id));
        let runs = self
            .enriching
            .iter()
            .map(|(job, run)| (*job, run.location.id));
        scans
            .chain(runs)
            .filter(|(_, l)| location.is_none_or(|id| *l == id))
            .map(|(job, _)| job)
            .collect()
    }

    /// Applies a scheduler event; returns the scans it changed.
    fn apply(&mut self, event: JobEvent) -> Vec<ScanView> {
        match event {
            JobEvent::State(job, state) => {
                let phase = match state {
                    JobState::Queued => ScanPhase::Queued,
                    JobState::Running => ScanPhase::Running,
                    JobState::Paused => ScanPhase::Paused,
                };
                self.update(job, |view| view.phase = phase)
                    .into_iter()
                    .collect()
            }
            JobEvent::Progress(batch) => batch
                .into_iter()
                .filter_map(|(job, progress)| {
                    self.update(job, |view| view.items = progress.done as f64)
                })
                .collect(),
            JobEvent::Finished(job, outcome) => {
                let (phase, failure) = match outcome {
                    Outcome::Done => (ScanPhase::Done, None),
                    Outcome::Cancelled => (ScanPhase::Cancelled, None),
                    Outcome::Failed(code) => (ScanPhase::Failed, Some(code.to_string())),
                };
                let changed = self.update(job, |view| {
                    view.phase = phase;
                    view.failure = failure;
                });
                self.jobs.remove(&job);
                changed.into_iter().collect()
            }
        }
    }

    /// Changes the scan `job` is for, if it's still known; returns it as changed.
    fn update(&mut self, job: JobId, change: impl FnOnce(&mut ScanView)) -> Option<ScanView> {
        let view = self.scans.get_mut(&self.jobs.get(&job)?.id)?;
        change(view);
        Some(view.clone())
    }
}

struct Shared {
    index: PathBuf,
    protected: ProtectedPaths,
    volumes: Arc<dyn Volumes>,
    directories: Arc<dyn Directories>,
    placeholders: Arc<dyn Placeholders>,
    origins: Arc<dyn DownloadOrigins>,
    screenshots: Arc<dyn Screenshots>,
    book: Mutex<Book>,
    /// For the event sink, which queues enrich runs: a `Weak`, since the scheduler owns the
    /// jobs, which hold this. Set once the scheduler exists.
    scheduler: OnceLock<Weak<Scheduler>>,
}

impl Shared {
    fn book(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn run(&self, location: &Location, ctx: &JobContext) -> Result<(), JobError> {
        let mut index = IndexDb::open_for_scan(&self.index).map_err(|e| {
            tracing::error!(error = %e, "scan: can't open index.db");
            ScanError::Index(e)
        })?;
        let ports = ScanPorts {
            directories: self.directories.as_ref(),
            volumes: self.volumes.as_ref(),
            protected: &self.protected,
        };
        let options = ScanOptions {
            threads: walker_threads(location.caps.storage),
            // There's no exclusions setting yet.
            exclusions: Default::default(),
        };
        match scan(location, &mut index, &ports, &options, ctx, now()) {
            Ok(summary) => {
                tracing::info!(location = location.id.0, ?summary, "scan done");
                if let Some(view) = self.book().scans.get_mut(&location.id) {
                    view.record(&summary);
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!(location = location.id.0, error = %e, "scan stopped");
                Err(e.into())
            }
        }
    }

    fn run_enrich(
        &self,
        location: &Location,
        from: Option<ItemId>,
        ctx: &JobContext,
    ) -> Result<(), JobError> {
        let mut index = IndexDb::open_for_scan(&self.index).map_err(|e| {
            tracing::error!(error = %e, "enrich: can't open index.db");
            EnrichError::Index(e)
        })?;
        let ports = EnrichPorts {
            volumes: self.volumes.as_ref(),
            placeholders: self.placeholders.as_ref(),
            origins: self.origins.as_ref(),
            screenshots: self.screenshots.as_ref(),
        };
        match enrich(location, from, &mut index, &ports, ctx, now()) {
            Ok(summary) => {
                tracing::info!(location = location.id.0, ?summary, "enrich slice done");
                if let Some(run) = self.book().enriching.get_mut(&ctx.id()) {
                    run.next = summary.next;
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!(location = location.id.0, error = %e, "enrich stopped");
                Err(e.into())
            }
        }
    }

    /// Queues an enrich run of `location` from `from`, on its volume like a scan; paused if
    /// the scans are.
    fn queue_enrich(self: &Arc<Self>, book: &mut Book, location: Location, from: Option<ItemId>) {
        let Some(scheduler) = self.scheduler.get().and_then(Weak::upgrade) else {
            return; // shutting down
        };
        let spec = JobSpec::new(Priority::Enrich)
            .on_volume(location.volume.clone(), location.caps.storage);
        let shared = Arc::clone(self);
        let job_location = location.clone();
        let job = scheduler.submit(spec, move |ctx| shared.run_enrich(&job_location, from, ctx));
        if book.paused {
            scheduler.pause(job);
        }
        book.enriching.insert(
            job,
            Enriching {
                location,
                from,
                started: false,
                again: false,
                next: None,
            },
        );
    }
}

pub struct Scans {
    shared: Arc<Shared>,
    scheduler: Arc<Scheduler>,
}

/// What [`Scans`] needs from its surroundings (the app, or a test).
pub struct Setup {
    pub index: PathBuf,
    pub protected: ProtectedPaths,
    pub volumes: Arc<dyn Volumes>,
    pub directories: Arc<dyn Directories>,
    pub placeholders: Arc<dyn Placeholders>,
    pub origins: Arc<dyn DownloadOrigins>,
    pub screenshots: Arc<dyn Screenshots>,
    /// Called with the scans that changed, on the scheduler's event thread.
    pub emit: Box<dyn Fn(Vec<ScanView>) + Send>,
}

impl Scans {
    /// The real thing: native adapters, the app's index, events to the UI.
    pub fn from_app(
        app: &AppHandle,
        locations: &LocationsState,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let home = app.path().home_dir()?;
        Ok(Self::new(Setup {
            index: locations.index_path(),
            protected: locations.protected().clone(),
            volumes: Arc::new(NativeVolumes::new(home.clone())),
            directories: Arc::new(NativeDirectories),
            placeholders: Arc::new(NativePlaceholders),
            origins: Arc::new(NativeDownloadOrigins),
            screenshots: Arc::new(NativeScreenshots::new(home)),
            emit: emitter(app),
        })?)
    }

    pub fn new(setup: Setup) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            index: setup.index,
            protected: setup.protected,
            volumes: setup.volumes,
            directories: setup.directories,
            placeholders: setup.placeholders,
            origins: setup.origins,
            screenshots: setup.screenshots,
            book: Mutex::default(),
            scheduler: OnceLock::new(),
        });
        let sink = Arc::downgrade(&shared);
        let emit = setup.emit;
        let scheduler = Arc::new(Scheduler::new(MAX_JOBS, move |event| {
            let Some(shared) = sink.upgrade() else {
                return;
            };
            let mut book = shared.book();
            let enrich = book.enrich_after(&event);
            let changed = book.apply(event);
            if let Some((location, from)) = enrich {
                shared.queue_enrich(&mut book, location, from);
            }
            drop(book);
            if !changed.is_empty() {
                emit(changed);
            }
        })?);
        let _ = shared.scheduler.set(Arc::downgrade(&scheduler));
        Ok(Self { shared, scheduler })
    }

    /// Queues a scan of each enabled, online Location that isn't being scanned already.
    pub fn start(&self, locations: &[Location]) {
        let mut book = self.shared.book();
        for location in locations.iter().filter(|l| l.enabled && l.online) {
            if book
                .scans
                .get(&location.id)
                .is_some_and(|view| view.phase.is_active())
            {
                continue;
            }
            book.scans
                .insert(location.id, ScanView::queued(location.id));
            let spec = JobSpec::new(Priority::Scan)
                .on_volume(location.volume.clone(), location.caps.storage);
            let shared = Arc::clone(&self.shared);
            let job_location = location.clone();
            let job = self
                .scheduler
                .submit(spec, move |ctx| shared.run(&job_location, ctx));
            book.jobs.insert(job, location.clone());
        }
    }

    /// Pauses every scan and enrich run, and the enrich runs queued until [`Scans::resume`].
    /// Under the book's lock, so a run queued meanwhile is either paused here or sees the flag.
    pub fn pause(&self) {
        let mut book = self.shared.book();
        book.paused = true;
        for job in book.job_ids(None) {
            self.scheduler.pause(job);
        }
    }

    pub fn resume(&self) {
        let mut book = self.shared.book();
        book.paused = false;
        for job in book.job_ids(None) {
            self.scheduler.resume(job);
        }
    }

    /// Cancels every scan and enrich run, or only `location`'s. Under the book's lock, so a scan
    /// finishing meanwhile can't queue an enrich run this misses.
    pub fn cancel(&self, location: Option<LocationId>) {
        let mut book = self.shared.book();
        for job in book.job_ids(location) {
            // Every job here still gets its `Finished` event, which takes it out again.
            book.stopping.insert(job);
            self.scheduler.cancel(job);
        }
    }

    /// Cancels `location`'s jobs and drops it from the status: the Location is going away.
    pub fn forget(&self, location: LocationId) {
        self.cancel(Some(location));
        self.shared.book().scans.remove(&location);
    }

    pub fn status(&self) -> Vec<ScanView> {
        self.shared.book().scans.values().cloned().collect()
    }

    /// Waits for the scans to end (on quit, so an open batch gets committed).
    pub fn wait(&self, timeout: Duration) -> bool {
        self.scheduler.wait_idle(timeout)
    }
}

impl std::fmt::Debug for Scans {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scans").finish_non_exhaustive()
    }
}

/// Scans the given Location, or every one, when the UI asks.
pub fn start(app: &AppHandle, location: Option<LocationId>) {
    let locations: Vec<Location> = app
        .state::<LocationsState>()
        .locations()
        .into_iter()
        .filter(|l| location.is_none_or(|id| l.id == id))
        .collect();
    app.state::<Scans>().start(&locations);
}

#[tauri::command]
#[specta::specta]
pub async fn scan_start(app: AppHandle, location_id: Option<u32>) -> Result<(), Coded> {
    let location = location_id.map(|id| LocationId(i64::from(id)));
    tauri::async_runtime::spawn_blocking(move || start(&app, location))
        .await
        .map_err(|_| Coded::new("scan_not_started"))
}

#[tauri::command]
#[specta::specta]
pub fn scan_pause(scans: tauri::State<'_, Scans>) {
    scans.pause();
}

#[tauri::command]
#[specta::specta]
pub fn scan_resume(scans: tauri::State<'_, Scans>) {
    scans.resume();
}

#[tauri::command]
#[specta::specta]
pub fn scan_cancel(scans: tauri::State<'_, Scans>) {
    scans.cancel(None);
}

/// Every Location's current or last scan.
#[tauri::command]
#[specta::specta]
pub fn scan_status(scans: tauri::State<'_, Scans>) -> Vec<ScanView> {
    scans.status()
}

/// Emits [`ScanProgress`] to the UI.
fn emitter(app: &AppHandle) -> Box<dyn Fn(Vec<ScanView>) + Send> {
    let app = app.clone();
    Box::new(move |scans| {
        let _ = ScanProgress { scans }.emit(&app);
    })
}

#[cfg(test)]
mod tests {
    use kipple_core::identity::VolumeId;
    use kipple_core::location::{LocationKind, StorageKind, TrashSupport, VolumeCaps};
    use kipple_engine::enrich::{BATCH, SLICE};
    use kipple_platform::fake::{
        FakeDownloadOrigins, FakePlaceholders, FakeScreenshots, FakeVolumes,
    };
    use kipple_platform::{Os, PortResult};

    use super::*;

    const CAPS: VolumeCaps = VolumeCaps {
        trash: TrashSupport::Yes,
        case_sensitive: true,
        stable_file_ids: true,
        storage: StorageKind::Ssd,
    };
    const WAIT: Duration = Duration::from_secs(10);

    struct World {
        _dir: tempfile::TempDir,
        root: PathBuf,
        location: Location,
        index: PathBuf,
        emitted: Arc<Mutex<Vec<ScanView>>>,
        scans: Scans,
    }

    #[allow(clippy::disallowed_methods)] // test fixture
    fn world() -> World {
        world_listing_with(Arc::new(NativeDirectories))
    }

    fn world_listing_with(directories: Arc<dyn Directories>) -> World {
        world_with(directories, Arc::new(FakeDownloadOrigins::default()), 0)
    }

    /// Two files and a folder, and `more` files besides.
    #[allow(clippy::disallowed_methods)] // test fixture
    fn world_with(
        directories: Arc<dyn Directories>,
        origins: Arc<dyn DownloadOrigins>,
        more: usize,
    ) -> World {
        let dir = tempfile::tempdir().unwrap();
        let base = kipple_platform::canonical(dir.path()).unwrap();
        let root = base.join("files");
        std::fs::create_dir_all(root.join("Sub")).unwrap();
        std::fs::write(root.join("a.txt"), "1").unwrap();
        std::fs::write(root.join("Sub/b.txt"), "2").unwrap();
        for i in 0..more {
            std::fs::write(root.join(format!("more-{i}.txt")), "3").unwrap();
        }
        let location = Location {
            id: LocationId(3),
            path: root.clone(),
            kind: LocationKind::Custom,
            enabled: true,
            volume: VolumeId("temp".into()),
            online: true,
            last_seen_online: None,
            caps: CAPS,
            in_sync_root: false,
        };
        let index = base.join("index.db");
        IndexDb::open(&index)
            .unwrap()
            .put_location(&location)
            .unwrap();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&emitted);
        let scans = Scans::new(Setup {
            index: index.clone(),
            protected: ProtectedPaths::for_os(Os::Linux, &base, std::path::Path::new("/"), &[]),
            volumes: Arc::new(FakeVolumes::single("temp", base.clone(), CAPS)),
            directories,
            placeholders: Arc::new(FakePlaceholders::default()),
            origins,
            screenshots: Arc::new(FakeScreenshots::default()),
            emit: Box::new(move |views| sink.lock().unwrap().extend(views)),
        })
        .unwrap();
        World {
            _dir: dir,
            root,
            location,
            index,
            emitted,
            scans,
        }
    }

    fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
        let start = std::time::Instant::now();
        while !cond() {
            assert!(start.elapsed() < WAIT, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_scan_fills_the_index_and_reports_its_counts() {
        let w = world();
        w.scans.start(std::slice::from_ref(&w.location));
        assert!(w.scans.wait(WAIT));
        wait_for("the done event", || {
            w.emitted
                .lock()
                .unwrap()
                .last()
                .is_some_and(|v| v.phase == ScanPhase::Done)
        });
        let status = w.scans.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].location_id, 3);
        assert_eq!(status[0].phase, ScanPhase::Done);
        assert_eq!((status[0].items, status[0].added), (3.0, 3.0));
        let index = IndexDb::open(&w.index).unwrap();
        assert_eq!(
            index.items_in(LocationId(3), &w.root).unwrap().len(),
            2,
            "a.txt and Sub"
        );
        let mut phases: Vec<_> = w.emitted.lock().unwrap().iter().map(|v| v.phase).collect();
        phases.dedup();
        assert_eq!(
            phases,
            [ScanPhase::Queued, ScanPhase::Running, ScanPhase::Done]
        );
    }

    /// Lists folders only while the test lets it.
    struct Gate(Mutex<bool>, std::sync::Condvar);

    impl Directories for Gate {
        fn list(
            &self,
            dir: &std::path::Path,
        ) -> kipple_platform::PortResult<Vec<kipple_platform::DirEntry>> {
            let mut open = self.0.lock().unwrap();
            while !*open {
                open = self.1.wait(open).unwrap();
            }
            NativeDirectories.list(dir)
        }
    }

    #[test]
    fn disabled_offline_and_already_running_locations_are_not_scanned_again() {
        let gate = Arc::new(Gate(Mutex::new(false), std::sync::Condvar::new()));
        let w = world_listing_with(gate.clone());
        let mut disabled = w.location.clone();
        disabled.id = LocationId(4);
        disabled.enabled = false;
        let mut offline = w.location.clone();
        offline.id = LocationId(5);
        offline.online = false;
        w.scans.start(&[w.location.clone(), disabled, offline]);
        // The first scan can't have finished: it waits at the gate.
        w.scans.start(std::slice::from_ref(&w.location));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(w.scans.wait(WAIT));
        assert_eq!(w.scans.status().len(), 1, "only the enabled, online one");
        wait_for("the done event", || {
            w.emitted
                .lock()
                .unwrap()
                .iter()
                .any(|v| v.phase == ScanPhase::Done)
        });
        let runs = w
            .emitted
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.phase == ScanPhase::Done)
            .count();
        assert_eq!(runs, 1, "a second start while queued is ignored");
    }

    #[test]
    fn a_cancelled_scan_says_so() {
        let w = world();
        w.scans.start(std::slice::from_ref(&w.location));
        w.scans.cancel(Some(LocationId(3)));
        assert!(w.scans.wait(WAIT));
        wait_for("the end", || !w.scans.status()[0].phase.is_active());
        let phase = w.scans.status()[0].phase;
        // It may have finished before the cancel arrived: a tiny tree scans in no time.
        assert!(
            matches!(phase, ScanPhase::Cancelled | ScanPhase::Done),
            "{phase:?}"
        );
    }

    #[test]
    fn an_offline_location_fails_with_a_code() {
        let mut w = world();
        w.location.volume = VolumeId("ejected".into());
        w.scans.start(std::slice::from_ref(&w.location));
        assert!(w.scans.wait(WAIT));
        wait_for("the event", || {
            w.scans.status()[0].phase == ScanPhase::Failed
        });
        assert_eq!(w.scans.status()[0].failure.as_deref(), Some("scan_offline"));
    }

    #[test]
    fn a_forgotten_location_leaves_the_status() {
        let w = world();
        w.scans.start(std::slice::from_ref(&w.location));
        w.scans.forget(LocationId(3));
        assert!(w.scans.status().is_empty());
        assert!(w.scans.wait(WAIT));
    }

    /// How many of the test Location's Items are still to enrich.
    fn to_enrich(w: &World) -> usize {
        IndexDb::open(&w.index)
            .unwrap()
            .items_to_enrich(LocationId(3), None, usize::MAX)
            .unwrap()
            .len()
    }

    fn wait_for_phase(w: &World, phase: ScanPhase) {
        wait_for("the scan's end", || w.scans.status()[0].phase == phase);
    }

    #[test]
    fn a_done_scan_is_followed_by_an_enrich_run() {
        let w = world();
        w.scans.start(std::slice::from_ref(&w.location));
        wait_for_phase(&w, ScanPhase::Done);
        wait_for("the enrich run", || to_enrich(&w) == 0);
        assert!(w.scans.wait(WAIT));
    }

    #[test]
    fn failed_and_cancelled_scans_are_not_followed_by_one() {
        let mut w = world();
        w.location.volume = VolumeId("ejected".into());
        w.scans.start(std::slice::from_ref(&w.location));
        // Decided under the same lock that records the phase.
        wait_for_phase(&w, ScanPhase::Failed);
        assert!(w.scans.shared.book().enriching.is_empty(), "failed");

        let gate = Arc::new(Gate(Mutex::new(false), std::sync::Condvar::new()));
        let w = world_listing_with(gate.clone());
        w.scans.start(std::slice::from_ref(&w.location));
        w.scans.cancel(Some(LocationId(3)));
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        wait_for("the end", || !w.scans.status()[0].phase.is_active());
        assert!(w.scans.shared.book().enriching.is_empty(), "cancelled");
        assert!(w.scans.wait(WAIT));
    }

    /// Holds every origin lookup until the test lets them go, so an enrich run stays running.
    #[derive(Default)]
    struct HeldOrigins {
        /// (a lookup started, let go)
        state: Mutex<(bool, bool)>,
        changed: std::sync::Condvar,
    }

    impl HeldOrigins {
        fn wait_until_running(&self) {
            let state = self.state.lock().unwrap();
            let (_state, timeout) = self
                .changed
                .wait_timeout_while(state, WAIT, |(started, _)| !*started)
                .unwrap();
            assert!(!timeout.timed_out(), "the enrich run didn't start");
        }

        fn let_go(&self) {
            self.state.lock().unwrap().1 = true;
            self.changed.notify_all();
        }
    }

    impl DownloadOrigins for HeldOrigins {
        fn origin_of(&self, _path: &std::path::Path) -> PortResult<Option<String>> {
            let mut state = self.state.lock().unwrap();
            state.0 = true;
            self.changed.notify_all();
            while !state.1 {
                state = self.changed.wait(state).unwrap();
            }
            Ok(None)
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn a_location_has_one_enrich_run_at_a_time_and_another_after_it_if_needed() {
        let origins = Arc::new(HeldOrigins::default());
        let w = world_with(Arc::new(NativeDirectories), origins.clone(), 0);
        w.scans.start(std::slice::from_ref(&w.location));
        origins.wait_until_running();
        // Changed after the run read it: what it learned about a.txt isn't stored.
        std::fs::write(w.root.join("a.txt"), "changed").unwrap();
        w.scans.start(std::slice::from_ref(&w.location));
        wait_for_phase(&w, ScanPhase::Done);
        {
            let book = w.scans.shared.book();
            assert_eq!(book.enriching.len(), 1, "no second run beside it");
            assert!(book.enriching.values().all(|run| run.again));
        }
        origins.let_go();
        wait_for("the run after it", || to_enrich(&w) == 0);
        assert!(w.scans.wait(WAIT));
    }

    #[test]
    fn switching_a_location_off_or_removing_it_cancels_its_enrich_run() {
        for remove in [false, true] {
            let origins = Arc::new(HeldOrigins::default());
            let w = world_with(Arc::new(NativeDirectories), origins.clone(), BATCH);
            w.scans.start(std::slice::from_ref(&w.location));
            origins.wait_until_running();
            if remove {
                w.scans.forget(LocationId(3));
            } else {
                w.scans.cancel(Some(LocationId(3)));
            }
            origins.let_go();
            wait_for("the run's end", || {
                w.scans.shared.book().enriching.is_empty()
            });
            assert!(w.scans.wait(WAIT));
            // It stops between batches: a.txt, b.txt and the more-*.txt files, less one batch.
            assert_eq!(to_enrich(&w), 2, "remove: {remove}");
        }
    }

    #[test]
    fn a_scan_queued_during_an_enrich_slice_goes_before_the_next_slice() {
        let gate = Arc::new(Gate(Mutex::new(true), std::sync::Condvar::new()));
        let origins = Arc::new(HeldOrigins::default());
        let mut w = world_with(gate.clone(), origins.clone(), SLICE);
        // One slot: scans and slices take turns.
        w.location.caps.storage = StorageKind::Hdd;
        w.scans.start(std::slice::from_ref(&w.location));
        origins.wait_until_running();
        // Queued behind the slice; once it runs, it waits at the gate.
        *gate.0.lock().unwrap() = false;
        w.scans.start(std::slice::from_ref(&w.location));
        origins.let_go();
        wait_for_phase(&w, ScanPhase::Running);
        // The slice ended at its bound, and the next one waits for the scan.
        assert_eq!(
            to_enrich(&w),
            2,
            "a.txt, b.txt and the more-*.txt files, less a slice"
        );
        assert_eq!(w.scans.shared.book().enriching.len(), 1, "the next slice");
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        wait_for("the next slice", || to_enrich(&w) == 0);
        assert!(w.scans.wait(WAIT));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn an_enrich_run_queued_while_paused_waits_for_resume() {
        let origins = Arc::new(HeldOrigins::default());
        let w = world_with(Arc::new(NativeDirectories), origins.clone(), 0);
        w.scans.start(std::slice::from_ref(&w.location));
        origins.wait_until_running();
        std::fs::write(w.root.join("a.txt"), "changed").unwrap();
        w.scans.start(std::slice::from_ref(&w.location));
        // Done while the run reads: it asks for another after it.
        wait_for_phase(&w, ScanPhase::Done);
        let first = *w.scans.shared.book().enriching.keys().next().unwrap();
        w.scans.pause();
        // In its last batch: it ends without another checkpoint, so the pause doesn't stop it.
        origins.let_go();
        wait_for("the run after it, paused", || {
            let book = w.scans.shared.book();
            let Some(&next) = book.enriching.keys().find(|&&job| job != first) else {
                return false;
            };
            w.scans
                .scheduler
                .status()
                .iter()
                .any(|job| job.id == next && job.state == JobState::Paused)
        });
        assert_eq!(to_enrich(&w), 1, "a.txt waits");
        w.scans.resume();
        wait_for("the run after it", || to_enrich(&w) == 0);
        assert!(w.scans.wait(WAIT));
    }

    fn run(location: &Location, from: Option<ItemId>, started: bool) -> Enriching {
        Enriching {
            location: location.clone(),
            from,
            started,
            again: false,
            next: None,
        }
    }

    #[test]
    fn a_run_that_failed_goes_again_if_a_scan_asked_but_not_if_cancelled() {
        let w = world();
        for (outcome, again) in [
            (Outcome::Failed("enrich_offline"), true),
            (Outcome::Cancelled, false),
        ] {
            let mut book = Book::default();
            let mut asked = run(&w.location, None, true);
            asked.again = true;
            book.enriching.insert(JobId(1), asked);
            let follow = book.enrich_after(&JobEvent::Finished(JobId(1), outcome));
            assert_eq!(follow.is_some(), again, "{outcome:?}");
        }
    }

    #[test]
    fn a_scan_done_before_a_later_slice_starts_asks_for_a_new_pass() {
        let w = world();
        for (from, again) in [(None, false), (Some(ItemId(7)), true)] {
            let mut book = Book::default();
            book.jobs.insert(JobId(1), w.location.clone());
            book.enriching
                .insert(JobId(2), run(&w.location, from, false));
            assert!(
                book.enrich_after(&JobEvent::Finished(JobId(1), Outcome::Done))
                    .is_none()
            );
            // A first slice sees what the scan changed; one that starts further on may not.
            assert_eq!(book.enriching[&JobId(2)].again, again, "{from:?}");
        }
    }
}
