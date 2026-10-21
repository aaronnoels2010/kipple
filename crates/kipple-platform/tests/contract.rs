// SPDX-License-Identifier: GPL-3.0-or-later
//! Port contract tests (ADR-0005): the same checks against the in-memory fakes and against the
//! real adapter of the OS the tests run on. CI runs them on macOS, Windows and Ubuntu.

use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
use kipple_platform::fake::{FakePlaceholders, FakeSyncRoots, FakeVolumes};
use kipple_platform::{
    NativePlaceholders, NativeSyncRoots, NativeVolumes, Placeholders, PortError, SyncProvider,
    SyncRoot, SyncRoots, VolumeInfo, Volumes, sync_root_of,
};

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .expect("a home folder")
        .into()
}

/// What every `Volumes` must do, for an existing folder `dir` with a file inside it.
fn volumes_contract(volumes: &dyn Volumes, dir: &Path, file: &Path) -> VolumeInfo {
    let volume = volumes
        .volume_of(dir)
        .unwrap_or_else(|e| panic!("volume_of({}): {e}", dir.display()));
    assert!(!volume.id.0.is_empty(), "an ID");
    // macOS firmlinks (`/Users`, `/private/var`) put paths outside their volume's mount point.
    assert!(
        volume.mount_point.is_dir()
            && (cfg!(target_os = "macos")
                || kipple_core::paths::is_within(
                    &volume.mount_point,
                    dir,
                    volume.caps.case_sensitive
                )),
        "{} is mounted at {}",
        dir.display(),
        volume.mount_point.display()
    );
    let again = volumes.volume_of(file).expect("the file's volume");
    assert_eq!(
        (again.id.clone(), again.mount_point.clone()),
        (volume.id.clone(), volume.mount_point.clone()),
        "same volume"
    );
    assert_eq!(
        volumes.volume_of(dir).unwrap().id,
        volume.id,
        "the ID is stable"
    );
    if volume.caps.storage == StorageKind::Network || volume.read_only {
        assert!(
            !volume.caps.can_trash(),
            "network and read-only volumes never claim a Trash (S9)"
        );
    }
    volume
}

fn temp_with_file() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    // Canonical: on macOS the temp folder is reached through the /var → /private/var symlink.
    let path = std::fs::canonicalize(dir.path()).unwrap();
    let file = path.join("a.txt");
    #[allow(clippy::disallowed_methods)] // test fixture
    std::fs::write(&file, "x").unwrap();
    (dir, path, file)
}

#[test]
fn native_volumes_meet_the_contract() {
    let (_keep, dir, file) = temp_with_file();
    let native = NativeVolumes::new(home());
    let temp = volumes_contract(&native, &dir, &file);
    eprintln!("temp: {temp:?}");
    let home = std::fs::canonicalize(home()).unwrap();
    let home_volume = native.volume_of(&home).expect("the home volume");
    eprintln!("home: {home_volume:?}");
    // The user's own (local, writable) home volume: a known Trash on Windows and Linux; macOS
    // stays "unknown" until the Trash port checks it (K-4.6).
    assert!(!home_volume.read_only && home_volume.caps.storage != StorageKind::Network);
    // Linux knows the home Trash; macOS and Windows stay "unknown" until the Trash port checks
    // the volume (K-4.6). Never "no" for the home volume.
    let expected = if cfg!(target_os = "linux") {
        TrashSupport::Yes
    } else {
        TrashSupport::Unknown
    };
    assert_eq!(home_volume.caps.trash, expected);
    if cfg!(any(target_os = "macos", windows)) {
        assert!(
            !home_volume.caps.case_sensitive,
            "macOS and Windows default to case-insensitive"
        );
    }
}

#[test]
fn fake_volumes_meet_the_contract() {
    let (_keep, dir, file) = temp_with_file();
    let caps = VolumeCaps {
        trash: TrashSupport::Yes,
        case_sensitive: true,
        stable_file_ids: true,
        storage: StorageKind::Ssd,
    };
    let root = dir.ancestors().last().unwrap().to_path_buf();
    let fake = FakeVolumes {
        volumes: vec![
            VolumeInfo {
                id: VolumeId("root".into()),
                mount_point: root,
                fs_type: "fakefs".into(),
                read_only: false,
                caps,
            },
            VolumeInfo {
                id: VolumeId("temp".into()),
                mount_point: dir.clone(),
                fs_type: "fakefs".into(),
                read_only: false,
                caps,
            },
        ],
    };
    assert_eq!(
        volumes_contract(&fake, &dir, &file).id,
        VolumeId("temp".into()),
        "the longest mount point wins"
    );
}

/// What every `Placeholders` must do: a normal local file is never a placeholder.
fn placeholders_contract(placeholders: &dyn Placeholders, local_file: &Path) {
    match placeholders.is_placeholder(local_file) {
        Ok(is) => assert!(!is, "a file just written locally has its content"),
        Err(PortError::Unsupported) => {}
        Err(e) => panic!("is_placeholder: {e}"),
    }
}

/// What every `SyncRoots` must do: real, absolute folders, each listed once.
fn sync_roots_contract(sync_roots: &dyn SyncRoots) -> Option<Vec<SyncRoot>> {
    match sync_roots.sync_roots() {
        Ok(roots) => {
            for root in &roots {
                assert!(root.path.is_absolute() && root.path.is_dir(), "{root:?}");
            }
            let mut paths: Vec<_> = roots.iter().map(|r| &r.path).collect();
            paths.dedup();
            assert_eq!(paths.len(), roots.len(), "no duplicates");
            Some(roots)
        }
        Err(PortError::Unsupported) => None,
        Err(e) => panic!("sync_roots: {e}"),
    }
}

#[test]
fn native_placeholders_and_sync_roots_meet_the_contract() {
    let (_keep, _dir, file) = temp_with_file();
    placeholders_contract(&NativePlaceholders, &file);
    let roots = sync_roots_contract(&NativeSyncRoots::new(home()));
    eprintln!("sync roots: {roots:?}");
    // Linux has neither; macOS and Windows always answer (possibly with none).
    assert_eq!(roots.is_none(), cfg!(target_os = "linux"));
    assert_eq!(
        matches!(
            NativePlaceholders.is_placeholder(&file),
            Err(PortError::Unsupported)
        ),
        cfg!(target_os = "linux")
    );
}

#[test]
fn fake_placeholders_and_sync_roots_meet_the_contract() {
    let (_keep, dir, file) = temp_with_file();
    let placeholders = FakePlaceholders {
        placeholders: Some(Default::default()),
    };
    placeholders_contract(&placeholders, &file);
    let fake = FakeSyncRoots {
        roots: Some(vec![SyncRoot {
            path: dir.clone(),
            provider: SyncProvider::OneDrive,
        }]),
    };
    assert_eq!(sync_roots_contract(&fake).unwrap().len(), 1);
}

#[test]
fn sync_root_of_finds_the_containing_root() {
    let roots = [
        SyncRoot {
            path: PathBuf::from("/Users/zoe/Library/CloudStorage/OneDrive-Personal"),
            provider: SyncProvider::OneDrive,
        },
        SyncRoot {
            path: PathBuf::from("/Users/zoe/Library/Mobile Documents/com~apple~CloudDocs"),
            provider: SyncProvider::ICloudDrive,
        },
    ];
    let inside = Path::new("/Users/zoe/Library/CloudStorage/OneDrive-Personal/Docs/a.pdf");
    if cfg!(windows) {
        return; // the table uses Unix paths, which aren't absolute on Windows
    }
    assert_eq!(
        sync_root_of(inside, &roots, false).map(|r| &r.provider),
        Some(&SyncProvider::OneDrive)
    );
    assert_eq!(
        sync_root_of(Path::new("/Users/zoe/Downloads/a.pdf"), &roots, false),
        None
    );
}

#[test]
fn unsupported_fakes_say_so() {
    assert!(matches!(
        FakePlaceholders::default().is_placeholder(Path::new("/x")),
        Err(PortError::Unsupported)
    ));
    assert!(matches!(
        FakeSyncRoots::default().sync_roots(),
        Err(PortError::Unsupported)
    ));
}
