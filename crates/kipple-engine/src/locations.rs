// SPDX-License-Identifier: GPL-3.0-or-later
//! Locations: the root folders the user lets Kipple manage (CONTEXT.md). Adding one checks it
//! against protected paths and the other Locations, and records its volume; the online check
//! compares the volume it's on now with the recorded one (ARCHITECTURE §7.1).

use std::path::{Path, PathBuf};

use kipple_core::location::{Location, LocationId, LocationKind};
use kipple_core::paths::is_within;
use kipple_core::time::Timestamp;
use kipple_platform::{PortError, ProtectedPaths, SyncRoots, Volumes, canonical, sync_root_of};

/// Why a folder can't become a Location. Each has a stable code for the UI (ADR-0004).
#[derive(Debug, thiserror::Error)]
pub enum AddError {
    #[error("not a folder")]
    NotAFolder,
    /// The folder's path isn't valid Unicode (only possible on Linux): Locations are kept in a
    /// JSON file, which can't hold it.
    #[error("unsupported folder name")]
    UnsupportedName,
    #[error("protected folder")]
    Protected,
    #[error("already a location")]
    AlreadyAdded(LocationId),
    #[error("inside another location")]
    InsideLocation(LocationId),
    #[error("contains another location")]
    ContainsLocation(LocationId),
    #[error(transparent)]
    Port(#[from] PortError),
}

impl AddError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotAFolder => "location_not_a_folder",
            Self::UnsupportedName => "location_unsupported_name",
            Self::Protected => "location_protected",
            Self::AlreadyAdded(_) => "location_already_added",
            Self::InsideLocation(_) => "location_inside_location",
            Self::ContainsLocation(_) => "location_contains_location",
            Self::Port(_) => "location_unreadable",
        }
    }
}

/// The OS facts a Location needs.
pub struct Ports<'a> {
    pub volumes: &'a dyn Volumes,
    pub sync_roots: &'a dyn SyncRoots,
    pub protected: &'a ProtectedPaths,
}

impl std::fmt::Debug for Ports<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ports").finish_non_exhaustive()
    }
}

/// A new Location `id` for the folder at `path` (as the native dialog or the OS returned it).
/// The path is resolved first (symlinks, junctions), and the resolved one is what's checked and
/// stored: a link can't smuggle a protected or already-added folder in.
pub fn new_location(
    path: &Path,
    kind: LocationKind,
    id: LocationId,
    existing: &[Location],
    ports: &Ports,
    now: Timestamp,
) -> Result<Location, AddError> {
    let Ok(resolved) = canonical(path) else {
        return Err(AddError::NotAFolder);
    };
    let path = resolved.as_path();
    if !std::fs::metadata(path).is_ok_and(|m| m.is_dir()) {
        return Err(AddError::NotAFolder);
    }
    if path.to_str().is_none() {
        return Err(AddError::UnsupportedName);
    }
    // A whole filesystem (`/`, `C:\`) holds the system itself: never a Location.
    if path.parent().is_none() || ports.protected.is_protected(path) {
        return Err(AddError::Protected);
    }
    // Nested Locations would scan, count and clean the same files twice.
    for other in existing {
        let case_sensitive = other.caps.case_sensitive;
        let inside = is_within(&other.path, path, case_sensitive);
        let contains = is_within(path, &other.path, case_sensitive);
        match (inside, contains) {
            (true, true) => return Err(AddError::AlreadyAdded(other.id)),
            (true, false) => return Err(AddError::InsideLocation(other.id)),
            (false, true) => return Err(AddError::ContainsLocation(other.id)),
            (false, false) => {}
        }
    }
    let volume = ports.volumes.volume_of(path)?;
    // Only informational (a notice about syncing), so a sync-root list the OS can't give
    // doesn't stop the user from adding the folder.
    let sync_roots = ports.sync_roots.sync_roots().unwrap_or_default();
    let in_sync_root = sync_root_of(path, &sync_roots, volume.caps.case_sensitive).is_some();
    Ok(Location {
        id,
        path: PathBuf::from(path),
        kind,
        enabled: true,
        volume: volume.id,
        online: true,
        last_seen_online: Some(now),
        caps: volume.caps,
        in_sync_root,
    })
}

/// Whether `location` is reachable right now: its folder exists and is on the volume it was added
/// on. An unmounted Linux mount point is an empty folder on another volume, and an ejected drive
/// or a disconnected NAS is missing: both are offline, and their Items are kept.
pub fn is_online(location: &Location, volumes: &dyn Volumes) -> bool {
    std::fs::metadata(&location.path).is_ok_and(|m| m.is_dir())
        && volumes
            .volume_of(&location.path)
            .is_ok_and(|v| v.id == location.volume)
}

#[cfg(test)]
mod tests {
    use kipple_core::identity::VolumeId;
    use kipple_core::location::{StorageKind, TrashSupport, UserFolder, VolumeCaps};
    use kipple_platform::fake::{FakeSyncRoots, FakeVolumes};
    use kipple_platform::{Os, SyncProvider, SyncRoot, VolumeInfo};

    use super::*;

    const CAPS: VolumeCaps = VolumeCaps {
        trash: TrashSupport::Yes,
        case_sensitive: false,
        stable_file_ids: true,
        storage: StorageKind::Ssd,
    };

    struct World {
        _dir: tempfile::TempDir,
        root: PathBuf,
        volumes: FakeVolumes,
        sync_roots: FakeSyncRoots,
        protected: ProtectedPaths,
    }

    /// A temp folder standing in for a home folder on volume "internal", with a USB stick mounted
    /// at `usb` (volume "usb") and a OneDrive folder.
    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = kipple_platform::canonical(dir.path()).unwrap();
        #[allow(clippy::disallowed_methods)] // test fixture
        {
            for folder in [
                "Downloads",
                "Documents/Taxes",
                "OneDrive/Docs",
                "usb/Photos",
                ".config/app",
            ] {
                std::fs::create_dir_all(root.join(folder)).unwrap();
            }
            std::fs::write(root.join("notes.txt"), "x").unwrap();
        }
        let volume = |id: &str, mount: PathBuf| VolumeInfo {
            id: VolumeId(id.into()),
            mount_point: mount,
            fs_type: "apfs".into(),
            read_only: false,
            caps: CAPS,
        };
        World {
            volumes: FakeVolumes {
                volumes: vec![
                    volume("internal", root.clone()),
                    volume("usb", root.join("usb")),
                ],
            },
            sync_roots: FakeSyncRoots {
                roots: Some(vec![SyncRoot {
                    path: root.join("OneDrive"),
                    provider: SyncProvider::OneDrive,
                }]),
            },
            // Linux rules: they protect `<home>/.config` and don't match any OS's temp folder.
            protected: ProtectedPaths::for_os(Os::Linux, &root, Path::new("/"), &[]),
            root,
            _dir: dir,
        }
    }

    impl World {
        fn ports(&self) -> Ports<'_> {
            Ports {
                volumes: &self.volumes,
                sync_roots: &self.sync_roots,
                protected: &self.protected,
            }
        }

        fn add(&self, sub: &str, existing: &[Location]) -> Result<Location, AddError> {
            let id = LocationId(existing.len() as i64 + 1);
            new_location(
                &self.root.join(sub),
                LocationKind::Custom,
                id,
                existing,
                &self.ports(),
                Timestamp(7),
            )
        }
    }

    #[test]
    fn a_new_location_records_its_volume_and_sync_root() {
        let w = world();
        let downloads = new_location(
            &w.root.join("Downloads"),
            LocationKind::UserFolder(UserFolder::Downloads),
            LocationId(1),
            &[],
            &w.ports(),
            Timestamp(7),
        )
        .unwrap();
        assert_eq!(downloads.id, LocationId(1));
        assert_eq!(downloads.path, w.root.join("Downloads"));
        assert_eq!(downloads.volume, VolumeId("internal".into()));
        assert_eq!(downloads.caps, CAPS);
        assert!(downloads.enabled && downloads.online && !downloads.in_sync_root);
        assert_eq!(downloads.last_seen_online, Some(Timestamp(7)));

        let onedrive = w
            .add("OneDrive/Docs", std::slice::from_ref(&downloads))
            .unwrap();
        assert_eq!(onedrive.id, LocationId(2));
        assert!(onedrive.in_sync_root);
        assert_eq!(
            w.add("usb/Photos", &[]).unwrap().volume,
            VolumeId("usb".into())
        );
    }

    #[test]
    fn folders_that_cannot_be_locations() {
        let w = world();
        let docs = w.add("Documents", &[]).unwrap();
        let existing = std::slice::from_ref(&docs);
        assert!(matches!(w.add("notes.txt", &[]), Err(AddError::NotAFolder)));
        assert!(matches!(w.add("missing", &[]), Err(AddError::NotAFolder)));
        assert!(matches!(
            w.add(".config/app", &[]),
            Err(AddError::Protected)
        ));
        assert!(matches!(
            w.add("Documents", existing),
            Err(AddError::AlreadyAdded(LocationId(1)))
        ));
        assert!(matches!(
            w.add("Documents/Taxes", existing),
            Err(AddError::InsideLocation(LocationId(1)))
        ));
        assert!(matches!(
            w.add("", existing),
            Err(AddError::ContainsLocation(LocationId(1)))
        ));
        assert_eq!(AddError::Protected.code(), "location_protected");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_whose_name_is_not_unicode_is_refused() {
        use std::os::unix::ffi::OsStrExt;
        let w = world();
        let odd = w.root.join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
        #[allow(clippy::disallowed_methods)] // test fixture
        if std::fs::create_dir(&odd).is_err() {
            return; // the filesystem itself refuses such names (APFS does)
        }
        let added = new_location(
            &odd,
            LocationKind::Custom,
            LocationId(1),
            &[],
            &w.ports(),
            Timestamp(7),
        );
        assert!(matches!(added, Err(AddError::UnsupportedName)));
    }

    #[test]
    fn a_filesystem_root_is_never_a_location() {
        let w = world();
        let root = w.root.ancestors().last().unwrap();
        let added = new_location(
            root,
            LocationKind::Custom,
            LocationId(1),
            &[],
            &w.ports(),
            Timestamp(7),
        );
        assert!(matches!(added, Err(AddError::Protected)), "{added:?}");
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn links_are_resolved_before_checking() {
        let w = world();
        let docs = w.add("Documents", &[]).unwrap();
        std::os::unix::fs::symlink(w.root.join(".config/app"), w.root.join("to-config")).unwrap();
        std::os::unix::fs::symlink(w.root.join("Documents"), w.root.join("to-docs")).unwrap();
        std::os::unix::fs::symlink(w.root.join("Downloads"), w.root.join("to-downloads")).unwrap();
        assert!(matches!(w.add("to-config", &[]), Err(AddError::Protected)));
        assert!(matches!(
            w.add("to-docs", std::slice::from_ref(&docs)),
            Err(AddError::AlreadyAdded(_))
        ));
        assert_eq!(
            w.add("to-downloads", &[]).unwrap().path,
            w.root.join("Downloads"),
            "stored resolved"
        );
    }

    #[test]
    fn online_means_present_on_the_same_volume() {
        let w = world();
        let usb = w.add("usb/Photos", &[]).unwrap();
        assert!(is_online(&usb, &w.volumes));

        // Ejected: the mount point is gone, so the folder is too.
        let mut ejected = usb.clone();
        ejected.path = w.root.join("usb/Missing");
        assert!(!is_online(&ejected, &w.volumes));

        // Unmounted but the empty mount point is still there: same path, other volume.
        let mut unmounted = w.volumes.clone();
        unmounted.volumes.retain(|v| v.id.0 != "usb");
        assert!(!is_online(&usb, &unmounted));
    }
}
