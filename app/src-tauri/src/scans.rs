// SPDX-License-Identifier: GPL-3.0-or-later
//! Scans (K-2.6): the `scan_*` commands and the `ScanProgress` event. Each scan is a job on the
//! scheduler (kipple-engine::jobs) with its own index.db connection, so browsing and location
//! changes go on while it runs.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use kipple_core::location::{Location, LocationId};
use kipple_engine::jobs::{
    JobContext, JobError, JobEvent, JobId, JobSpec, JobState, Outcome, Priority, Scheduler,
};
use kipple_engine::scan::{ScanError, ScanOptions, ScanPorts, ScanSummary, scan, walker_threads};
use kipple_index::IndexDb;
use kipple_platform::{Directories, NativeDirectories, NativeVolumes, ProtectedPaths, Volumes};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::ipc::Coded;
use crate::locations::{LocationsState, now};

/// Scans running at once, across volumes (each volume has its own, lower limit).
const MAX_SCANS: usize = 4;

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

/// Which job scans which Location, and every Location's latest scan. Locked by the scheduler's
/// event sink too: holding it across `submit` means no event about a job is handled before the
/// job is in here.
#[derive(Debug, Default)]
struct Book {
    jobs: BTreeMap<JobId, LocationId>,
    scans: BTreeMap<LocationId, ScanView>,
}

impl Book {
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
        let view = self.scans.get_mut(self.jobs.get(&job)?)?;
        change(view);
        Some(view.clone())
    }
}

struct Shared {
    index: PathBuf,
    protected: ProtectedPaths,
    volumes: Arc<dyn Volumes>,
    directories: Arc<dyn Directories>,
    book: Mutex<Book>,
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
}

pub struct Scans {
    shared: Arc<Shared>,
    scheduler: Scheduler,
}

/// What [`Scans`] needs from its surroundings (the app, or a test).
pub struct Setup {
    pub index: PathBuf,
    pub protected: ProtectedPaths,
    pub volumes: Arc<dyn Volumes>,
    pub directories: Arc<dyn Directories>,
    /// Called with the scans that changed, on the scheduler's event thread.
    pub emit: Box<dyn Fn(Vec<ScanView>) + Send>,
}

impl Scans {
    /// The real thing: native adapters, the app's index, events to the UI.
    pub fn from_app(
        app: &AppHandle,
        locations: &LocationsState,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self::new(Setup {
            index: locations.index_path(),
            protected: locations.protected().clone(),
            volumes: Arc::new(NativeVolumes::new(app.path().home_dir()?)),
            directories: Arc::new(NativeDirectories),
            emit: emitter(app),
        })?)
    }

    pub fn new(setup: Setup) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            index: setup.index,
            protected: setup.protected,
            volumes: setup.volumes,
            directories: setup.directories,
            book: Mutex::default(),
        });
        let sink = Arc::downgrade(&shared);
        let emit = setup.emit;
        let scheduler = Scheduler::new(MAX_SCANS, move |event| {
            let Some(shared) = sink.upgrade() else {
                return;
            };
            let changed = shared.book().apply(event);
            if !changed.is_empty() {
                emit(changed);
            }
        })?;
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
            book.jobs.insert(job, location.id);
        }
    }

    fn jobs(&self, location: Option<LocationId>) -> Vec<JobId> {
        self.shared
            .book()
            .jobs
            .iter()
            .filter(|(_, l)| location.is_none_or(|id| **l == id))
            .map(|(job, _)| *job)
            .collect()
    }

    pub fn pause(&self) {
        for job in self.jobs(None) {
            self.scheduler.pause(job);
        }
    }

    pub fn resume(&self) {
        for job in self.jobs(None) {
            self.scheduler.resume(job);
        }
    }

    /// Cancels every scan, or only `location`'s.
    pub fn cancel(&self, location: Option<LocationId>) {
        for job in self.jobs(location) {
            self.scheduler.cancel(job);
        }
    }

    /// Cancels `location`'s scan and drops it from the status: the Location is going away.
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
    use kipple_platform::Os;
    use kipple_platform::fake::FakeVolumes;

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

    #[allow(clippy::disallowed_methods)] // test fixture
    fn world_listing_with(directories: Arc<dyn Directories>) -> World {
        let dir = tempfile::tempdir().unwrap();
        let base = kipple_platform::canonical(dir.path()).unwrap();
        let root = base.join("files");
        std::fs::create_dir_all(root.join("Sub")).unwrap();
        std::fs::write(root.join("a.txt"), "1").unwrap();
        std::fs::write(root.join("Sub/b.txt"), "2").unwrap();
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

    /// Lists folders only once the test lets it.
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
}
