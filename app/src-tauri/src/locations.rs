// SPDX-License-Identifier: GPL-3.0-or-later
//! Location management (K-2.4): the commands behind Settings → Locations. A Location is only
//! ever created from a folder Rust got from the OS (a default user folder, or the native folder
//! dialog Rust opens), never from a path the webview sends (ADR-0007).
//!
//! `locations.json` is the source of truth and index.db mirrors it (ADR-0003). Every change is
//! saved to the file first and only then applied in memory, so the UI never shows a change that
//! wasn't stored. The commands run off the main thread: checking a disconnected network share
//! can take a long time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use kipple_core::location::{Location, LocationId, LocationKind, UserFolder};
use kipple_core::paths::is_within;
use kipple_core::time::Timestamp;
use kipple_engine::locations::{AddError, Ports, is_online, new_location};
use kipple_index::IndexDb;
use kipple_platform::{NativeSyncRoots, NativeVolumes, Os, ProtectedPaths, SyncRoots, Volumes};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::ipc::Coded;
use crate::locations_store::{self, Stored};

/// The default user folders Kipple offers (FUNCTIONAL_PLAN §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum DefaultFolder {
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Music,
    Videos,
}

impl DefaultFolder {
    fn user_folder(self) -> UserFolder {
        match self {
            Self::Desktop => UserFolder::Desktop,
            Self::Downloads => UserFolder::Downloads,
            Self::Documents => UserFolder::Documents,
            Self::Pictures => UserFolder::Pictures,
            Self::Music => UserFolder::Music,
            Self::Videos => UserFolder::Videos,
        }
    }

    /// Where the OS keeps this folder for the current user (localised names included).
    fn path(self, app: &AppHandle) -> Option<PathBuf> {
        let paths = app.path();
        match self {
            Self::Desktop => paths.desktop_dir(),
            Self::Downloads => paths.download_dir(),
            Self::Documents => paths.document_dir(),
            Self::Pictures => paths.picture_dir(),
            Self::Music => paths.audio_dir(),
            Self::Videos => paths.video_dir(),
        }
        .ok()
    }
}

/// A Location as the UI shows it. `kind` is a stable code (`downloads`, `custom`, `external`…).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct LocationView {
    pub id: u32,
    pub kind: String,
    /// For display only: the UI never sends it back.
    pub path: String,
    pub enabled: bool,
    pub online: bool,
    pub in_sync_root: bool,
    pub can_trash: bool,
}

impl From<&Location> for LocationView {
    fn from(l: &Location) -> Self {
        Self {
            id: u32::try_from(l.id.0).unwrap_or(u32::MAX),
            kind: l.kind.code().into(),
            path: l.path.to_string_lossy().into_owned(),
            enabled: l.enabled,
            online: l.online,
            in_sync_root: l.in_sync_root,
            can_trash: l.caps.can_trash(),
        }
    }
}

/// What a [`LocationsState`] needs from its surroundings (the app, or a test).
pub struct Setup {
    pub data_dir: PathBuf,
    pub home: PathBuf,
    pub protected: ProtectedPaths,
    pub volumes: Box<dyn Volumes>,
    pub sync_roots: Box<dyn SyncRoots>,
}

struct Inner {
    stored: Stored,
    /// False when locations.json couldn't be read (locked, no permission): then nothing may be
    /// changed, or the next save would overwrite the user's list with an empty one.
    writable: bool,
    index: IndexDb,
}

pub struct LocationsState {
    file: PathBuf,
    home: PathBuf,
    protected: ProtectedPaths,
    volumes: Box<dyn Volumes>,
    sync_roots: Box<dyn SyncRoots>,
    inner: Mutex<Inner>,
    /// One folder dialog at a time.
    dialog_open: AtomicBool,
}

fn now() -> Timestamp {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp(i64::try_from(since.as_nanos()).unwrap_or(i64::MAX))
}

fn storage_failed(error: impl std::fmt::Display) -> Coded {
    tracing::error!(error = %error, "saving locations failed");
    Coded::new("location_storage_failed")
}

/// The index is a cache: a failed write there is logged, never shown as a failed change.
fn cache_write<E: std::fmt::Display>(result: Result<(), E>) {
    if let Err(e) = result {
        tracing::warn!(error = %e, "index.db not updated (it will catch up on the next start)");
    }
}

impl LocationsState {
    /// The real thing: OS paths and adapters.
    pub fn from_app(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let paths = app.path();
        let data_dir = paths.app_data_dir()?;
        let home = paths.home_dir()?;
        // Kipple's own folders are protected too: the index, settings, logs and caches.
        let own: Vec<PathBuf> = [
            paths.app_config_dir(),
            paths.app_cache_dir(),
            paths.app_log_dir(),
        ]
        .into_iter()
        .flatten()
        .chain([data_dir.clone()])
        .collect();
        // `C:` plus a backslash: the drive's root (Path::join("\\") would replace, not append).
        let system_drive = match std::env::var_os("SystemDrive") {
            Some(mut drive) => {
                drive.push("\\");
                PathBuf::from(drive)
            }
            None if cfg!(windows) => PathBuf::from("C:\\"),
            None => PathBuf::from("/"),
        };
        Ok(Self::new(Setup {
            protected: ProtectedPaths::for_os(Os::current(), &home, &system_drive, &own),
            volumes: Box::new(NativeVolumes::new(home.clone())),
            sync_roots: Box::new(NativeSyncRoots::new(home.clone())),
            data_dir,
            home,
        }))
    }

    /// Loads `locations.json` and makes the index mirror it. Never fails: a damaged index is
    /// recreated, and an unreadable locations file makes the list read-only. The online check
    /// runs separately ([`Self::refresh_online`]), after the window is up.
    pub fn new(setup: Setup) -> Self {
        let file = setup.data_dir.join("locations.json");
        let (mut index, dropped) = IndexDb::open_or_recreate(&setup.data_dir.join("index.db"));
        if let Some(reason) = dropped {
            tracing::warn!(reason = %reason, "index.db was damaged, started a new one");
        }
        let (stored, writable) = match locations_store::load(&file) {
            Ok(stored) => (stored, true),
            Err(e) => {
                tracing::error!(error = %e, "can't read locations.json: locations are read-only until restart");
                let mirrored = index.locations().unwrap_or_default();
                (
                    Stored {
                        locations: mirrored,
                        ..Stored::default()
                    },
                    false,
                )
            }
        };
        if writable {
            // locations.json is the truth; the index follows it.
            let known: Vec<LocationId> = stored.locations.iter().map(|l| l.id).collect();
            for stale in index
                .locations()
                .unwrap_or_default()
                .into_iter()
                .filter(|l| !known.contains(&l.id))
            {
                cache_write(index.delete_location(stale.id));
            }
            for location in &stored.locations {
                cache_write(index.put_location(location));
            }
        }
        Self {
            file,
            home: setup.home,
            protected: setup.protected,
            volumes: setup.volumes,
            sync_roots: setup.sync_roots,
            inner: Mutex::new(Inner {
                stored,
                writable,
                index,
            }),
            dialog_open: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves consistent data (every change is "save, then
        // apply"), so carry on with it.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Re-checks every Location's online state (by volume ID) and mirrors it into the index.
    /// Offline Locations keep their Items: nothing is deleted here.
    pub fn refresh_online(&self) -> Vec<LocationView> {
        let mut inner = self.lock();
        let now = now();
        let mut flipped = false;
        let Inner {
            stored,
            writable,
            index,
        } = &mut *inner;
        for location in &mut stored.locations {
            let online = is_online(location, self.volumes.as_ref());
            flipped |= online != location.online;
            location.online = online;
            if online {
                location.last_seen_online = Some(now);
            }
            cache_write(index.put_location(location));
        }
        if flipped
            && *writable
            && let Err(e) = locations_store::save(&self.file, stored)
        {
            tracing::warn!(error = %e, "couldn't store when locations were last seen");
        }
        stored.locations.iter().map(LocationView::from).collect()
    }

    /// Saves a changed copy of the stored Locations, and only then makes it current.
    fn change<T>(&self, change: impl FnOnce(&mut Stored) -> Result<T, Coded>) -> Result<T, Coded> {
        let mut inner = self.lock();
        if !inner.writable {
            return Err(Coded::new("location_storage_failed"));
        }
        let mut next = inner.stored.clone();
        let value = change(&mut next)?;
        locations_store::save(&self.file, &next).map_err(storage_failed)?;
        inner.stored = next;
        Ok(value)
    }

    /// Adds the folder at `path` (from the OS, never from the webview) as a new Location.
    pub fn add(&self, path: &Path, kind: LocationKind) -> Result<LocationView, Coded> {
        let ports = Ports {
            volumes: self.volumes.as_ref(),
            sync_roots: self.sync_roots.as_ref(),
            protected: &self.protected,
        };
        let location = self.change(|stored| {
            let id = stored.take_id();
            let location = new_location(path, kind, id, &stored.locations, &ports, now()).map_err(|e: AddError| {
                tracing::info!(path = %path.display(), reason = e.code(), "folder not added as a location");
                Coded::new(e.code())
            })?;
            stored.locations.push(location.clone());
            Ok(location)
        })?;
        cache_write(self.lock().index.put_location(&location));
        Ok(LocationView::from(&location))
    }

    /// Adds a default user folder, if it exists and lies inside the home folder. On Linux, an
    /// unused XDG user folder (e.g. Music) points at the home folder itself: never that.
    pub fn add_default(
        &self,
        folder: DefaultFolder,
        path: Option<PathBuf>,
    ) -> Result<LocationView, Coded> {
        let path = path
            .filter(|p| {
                p.is_dir() && is_within(&self.home, p, true) && !is_within(p, &self.home, true)
            })
            .ok_or_else(|| Coded::new("location_default_missing"))?;
        self.add(&path, LocationKind::UserFolder(folder.user_folder()))
    }

    /// Stops managing a Location and forgets its Items (the files themselves are untouched).
    pub fn remove(&self, id: LocationId) -> Result<(), Coded> {
        self.change(|stored| {
            let before = stored.locations.len();
            stored.locations.retain(|l| l.id != id);
            if stored.locations.len() == before {
                return Err(Coded::new("location_not_found"));
            }
            Ok(())
        })?;
        cache_write(self.lock().index.delete_location(id));
        Ok(())
    }

    /// Pauses or resumes a Location: a disabled one isn't scanned or cleaned, its Items stay.
    pub fn set_enabled(&self, id: LocationId, enabled: bool) -> Result<LocationView, Coded> {
        let location = self.change(|stored| {
            let location = stored
                .locations
                .iter_mut()
                .find(|l| l.id == id)
                .ok_or_else(|| Coded::new("location_not_found"))?;
            location.enabled = enabled;
            Ok(location.clone())
        })?;
        cache_write(self.lock().index.put_location(&location));
        Ok(LocationView::from(&location))
    }
}

/// Runs `work` on the state, on a blocking thread: never on the main thread, which drives the UI.
async fn off_main_thread<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&LocationsState) -> Result<T, Coded> + Send + 'static,
) -> Result<T, Coded> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<LocationsState>()))
        .await
        .map_err(storage_failed)?
}

/// Every Location, with its online state checked now.
#[tauri::command]
#[specta::specta]
pub async fn locations_list(app: AppHandle) -> Result<Vec<LocationView>, Coded> {
    off_main_thread(app, |state| Ok(state.refresh_online())).await
}

/// Adds one of the default user folders.
#[tauri::command]
#[specta::specta]
pub async fn locations_add_default(
    app: AppHandle,
    folder: DefaultFolder,
) -> Result<LocationView, Coded> {
    let path = folder.path(&app);
    off_main_thread(app, move |state| state.add_default(folder, path)).await
}

/// Lets the user pick a folder in the OS's own dialog and adds it. `None` if they cancelled (or
/// a dialog is already open).
#[tauri::command]
#[specta::specta]
pub async fn locations_add_via_dialog(app: AppHandle) -> Result<Option<LocationView>, Coded> {
    let state = app.state::<LocationsState>();
    if state.dialog_open.swap(true, Ordering::SeqCst) {
        return Ok(None);
    }
    let window = app.get_webview_window(crate::MAIN_WINDOW);
    let picker = app.clone();
    // The dialog blocks until the user answers: off the async runtime's threads.
    let picked = tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = picker.dialog().file();
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        dialog.blocking_pick_folder()
    })
    .await;
    state.dialog_open.store(false, Ordering::SeqCst);
    let Some(folder) = picked.map_err(|_| Coded::new("location_dialog_failed"))? else {
        return Ok(None);
    };
    let path = folder
        .into_path()
        .map_err(|_| Coded::new("location_not_a_folder"))?;
    off_main_thread(app, move |state| {
        state.add(&path, LocationKind::Custom).map(Some)
    })
    .await
}

/// Stops managing a Location and forgets its Items (the files themselves are untouched).
#[tauri::command]
#[specta::specta]
pub async fn locations_remove(app: AppHandle, id: u32) -> Result<(), Coded> {
    off_main_thread(app, move |state| state.remove(LocationId(i64::from(id)))).await
}

/// Pauses or resumes a Location: a disabled one isn't scanned or cleaned, its Items stay.
#[tauri::command]
#[specta::specta]
pub async fn locations_set_enabled(
    app: AppHandle,
    id: u32,
    enabled: bool,
) -> Result<LocationView, Coded> {
    off_main_thread(app, move |state| {
        state.set_enabled(LocationId(i64::from(id)), enabled)
    })
    .await
}

#[cfg(test)]
mod tests {
    use kipple_core::identity::VolumeId;
    use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
    use kipple_platform::VolumeInfo;
    use kipple_platform::fake::{FakeSyncRoots, FakeVolumes};

    use super::*;

    struct World {
        _dir: tempfile::TempDir,
        home: PathBuf,
        data: PathBuf,
    }

    #[allow(clippy::disallowed_methods)] // test fixture
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = kipple_platform::canonical(dir.path()).unwrap();
        let (home, data) = (root.join("home"), root.join("data"));
        for folder in ["Downloads", "Documents", "Pictures"] {
            std::fs::create_dir_all(home.join(folder)).unwrap();
        }
        std::fs::create_dir_all(&data).unwrap();
        World {
            _dir: dir,
            home,
            data,
        }
    }

    impl World {
        fn state(&self) -> LocationsState {
            let caps = VolumeCaps {
                trash: TrashSupport::Yes,
                case_sensitive: true,
                stable_file_ids: true,
                storage: StorageKind::Ssd,
            };
            let root = self.home.ancestors().last().unwrap().to_path_buf();
            LocationsState::new(Setup {
                data_dir: self.data.clone(),
                home: self.home.clone(),
                protected: ProtectedPaths::for_os(
                    Os::Linux,
                    &self.home,
                    Path::new("/"),
                    std::slice::from_ref(&self.data),
                ),
                volumes: Box::new(FakeVolumes {
                    volumes: vec![VolumeInfo {
                        id: VolumeId("disk".into()),
                        mount_point: root,
                        fs_type: "fake".into(),
                        read_only: false,
                        caps,
                    }],
                }),
                sync_roots: Box::new(FakeSyncRoots::default()),
            })
        }

        fn add(&self, state: &LocationsState, folder: &str) -> Result<LocationView, Coded> {
            state.add(&self.home.join(folder), LocationKind::Custom)
        }
    }

    #[test]
    fn locations_survive_a_restart_and_ids_are_never_reused() {
        let w = world();
        let state = w.state();
        let downloads = w.add(&state, "Downloads").unwrap();
        let docs = w.add(&state, "Documents").unwrap();
        assert_eq!((downloads.id, docs.id), (1, 2));
        state.remove(LocationId(2)).unwrap();
        drop(state);

        let state = w.state();
        let list = state.refresh_online();
        assert_eq!(list.iter().map(|l| l.id).collect::<Vec<_>>(), [1]);
        assert!(list[0].online, "checked by volume");
        assert_eq!(
            w.add(&state, "Pictures").unwrap().id,
            3,
            "2 belonged to a removed location"
        );
        assert!(!state.set_enabled(LocationId(1), false).unwrap().enabled);
        assert_eq!(
            state.remove(LocationId(9)).unwrap_err().code,
            "location_not_found"
        );
    }

    #[test]
    fn the_index_mirrors_the_file_at_startup() {
        let w = world();
        let state = w.state();
        w.add(&state, "Downloads").unwrap();
        w.add(&state, "Documents").unwrap();
        // The file loses Documents behind Kipple's back (e.g. restored from a backup).
        let mut stored = locations_store::load(&w.data.join("locations.json")).unwrap();
        stored.locations.retain(|l| l.id == LocationId(1));
        locations_store::save(&w.data.join("locations.json"), &stored).unwrap();
        drop(state);
        let state = w.state();
        let mirrored = state.lock().index.locations().unwrap();
        assert_eq!(
            mirrored.iter().map(|l| l.id).collect::<Vec<_>>(),
            [LocationId(1)]
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn an_unreadable_locations_file_makes_the_list_read_only_not_empty() {
        let w = world();
        let state = w.state();
        w.add(&state, "Downloads").unwrap();
        drop(state);
        // Something other than "missing": here a folder where the file should be.
        std::fs::remove_file(w.data.join("locations.json")).unwrap();
        std::fs::create_dir(w.data.join("locations.json")).unwrap();
        let state = w.state();
        assert_eq!(state.refresh_online().len(), 1, "shown from the index");
        assert_eq!(
            w.add(&state, "Documents").unwrap_err().code,
            "location_storage_failed"
        );
        assert_eq!(
            state.lock().index.locations().unwrap().len(),
            1,
            "the index wasn't emptied"
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn a_failed_save_changes_nothing() {
        let w = world();
        let state = w.state();
        w.add(&state, "Downloads").unwrap();
        // The next save can't replace the file: a folder is in the way.
        std::fs::remove_file(w.data.join("locations.json")).unwrap();
        std::fs::create_dir(w.data.join("locations.json")).unwrap();
        assert_eq!(
            w.add(&state, "Documents").unwrap_err().code,
            "location_storage_failed"
        );
        assert_eq!(
            state.set_enabled(LocationId(1), false).unwrap_err().code,
            "location_storage_failed"
        );
        let list = state.refresh_online();
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
    }

    #[test]
    fn a_default_folder_must_exist_inside_home() {
        let w = world();
        let state = w.state();
        let added = state
            .add_default(DefaultFolder::Downloads, Some(w.home.join("Downloads")))
            .unwrap();
        assert_eq!(added.kind, "downloads");
        for path in [
            Some(w.home.clone()),
            Some(w.home.join("Music")),
            Some(w.data.clone()),
            None,
        ] {
            assert_eq!(
                state
                    .add_default(DefaultFolder::Music, path)
                    .unwrap_err()
                    .code,
                "location_default_missing"
            );
        }
    }
}
