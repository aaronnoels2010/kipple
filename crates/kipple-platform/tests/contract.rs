// SPDX-License-Identifier: GPL-3.0-or-later
//! Port contract tests (ADR-0005): the same checks against the in-memory fakes and against the
//! real adapter of the OS the tests run on. CI runs them on macOS, Windows and Ubuntu.

use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
use kipple_platform::fake::{FakePlaceholders, FakeSyncRoots, FakeVolumes};
use kipple_platform::{NativeVolumes, PortError, VolumeInfo, Volumes};

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
    if !cfg!(target_os = "macos") {
        assert_eq!(
            home_volume.caps.trash,
            TrashSupport::Yes,
            "home has a Trash / Recycle Bin"
        );
    }
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

#[test]
fn unsupported_fakes_say_so() {
    use kipple_platform::{Placeholders, SyncRoots};
    assert!(matches!(
        FakePlaceholders::default().is_placeholder(Path::new("/x")),
        Err(PortError::Unsupported)
    ));
    assert!(matches!(
        FakeSyncRoots::default().sync_roots(),
        Err(PortError::Unsupported)
    ));
}
