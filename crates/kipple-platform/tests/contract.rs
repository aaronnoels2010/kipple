// SPDX-License-Identifier: GPL-3.0-or-later
//! Port contract tests (ADR-0005): the same checks against the in-memory fakes and against the
//! real adapter of the OS the tests run on. CI runs them on macOS, Windows and Ubuntu.

use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{StorageKind, TrashSupport, VolumeCaps};
use kipple_platform::fake::{
    FakeDownloadOrigins, FakePlaceholders, FakeScreenshots, FakeSyncRoots, FakeVolumes,
};
use kipple_platform::{
    DownloadOrigins, NativeDownloadOrigins, NativePlaceholders, NativeScreenshots, NativeSyncRoots,
    NativeVolumes, Placeholders, PortError, Screenshots, SyncProvider, SyncRoot, SyncRoots,
    VolumeInfo, Volumes, sync_root_of,
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
    assert!(matches!(
        FakeDownloadOrigins::default().origin_of(Path::new("/x")),
        Err(PortError::Unsupported)
    ));
    assert!(matches!(
        FakeScreenshots::default().is_screenshot(Path::new("/x")),
        Err(PortError::Unsupported)
    ));
}

/// What every `DownloadOrigins` must do: a file written locally was downloaded from nowhere.
fn download_origins_contract(origins: &dyn DownloadOrigins, local_file: &Path) {
    match origins.origin_of(local_file) {
        Ok(origin) => assert_eq!(origin, None, "a file just written locally has no origin"),
        Err(PortError::Unsupported) => {}
        Err(e) => panic!("origin_of: {e}"),
    }
}

/// What every `Screenshots` must do: a text file just written in a temp folder isn't one.
fn screenshots_contract(screenshots: &dyn Screenshots, local_file: &Path) {
    match screenshots.is_screenshot(local_file) {
        Ok(is) => assert!(!is, "a text file in a temp folder is no screenshot"),
        Err(PortError::Unsupported) => {}
        Err(e) => panic!("is_screenshot: {e}"),
    }
}

#[test]
fn native_download_origins_and_screenshots_meet_the_contract() {
    let (_keep, dir, file) = temp_with_file();
    download_origins_contract(&NativeDownloadOrigins, &file);
    screenshots_contract(&NativeScreenshots::new(home()), &file);
    assert!(
        NativeDownloadOrigins
            .origin_of(&dir.join("missing.txt"))
            .is_err(),
        "a missing file is an error, not a file without an origin"
    );
}

#[test]
fn fake_download_origins_and_screenshots_meet_the_contract() {
    let (_keep, dir, file) = temp_with_file();
    let downloaded = dir.join("setup.exe");
    let origins = FakeDownloadOrigins {
        origins: Some([(downloaded.clone(), "https://example.com/setup.exe".into())].into()),
    };
    download_origins_contract(&origins, &file);
    assert_eq!(
        origins.origin_of(&downloaded).unwrap().as_deref(),
        Some("https://example.com/setup.exe")
    );
    let shot = dir.join("shot.png");
    let screenshots = FakeScreenshots {
        screenshots: Some([shot.clone()].into()),
    };
    screenshots_contract(&screenshots, &file);
    assert!(screenshots.is_screenshot(&shot).unwrap());
}

/// Sets the extended attribute `name` of `path` (a test fixture; Kipple never writes them).
#[cfg(unix)]
fn set_xattr(path: &Path, name: &str, value: &[u8]) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    let name = CString::new(name).unwrap();
    // SAFETY: NUL-terminated path and name; `value` is valid for `value.len()` bytes.
    #[cfg(target_os = "macos")]
    let status = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    // SAFETY: as above.
    #[cfg(target_os = "linux")]
    let status = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn bplist(value: plist::Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    value.to_writer_binary(&mut bytes).unwrap();
    bytes
}

#[cfg(target_os = "macos")]
#[test]
fn macos_reads_where_a_file_was_downloaded_from() {
    let (_keep, dir, file) = temp_with_file();
    // What Safari and Chrome write: the download URL, then the page that linked to it.
    let where_froms = plist::Value::Array(vec![
        "https://example.com/files/report.pdf".into(),
        "https://example.com/downloads".into(),
    ]);
    set_xattr(
        &file,
        "com.apple.metadata:kMDItemWhereFroms",
        &bplist(where_froms),
    )
    .unwrap();
    assert_eq!(
        NativeDownloadOrigins.origin_of(&file).unwrap().as_deref(),
        Some("https://example.com/files/report.pdf")
    );

    let link = dir.join("link.txt");
    #[allow(clippy::disallowed_methods)] // test fixture
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert_eq!(
        NativeDownloadOrigins.origin_of(&link).unwrap(),
        None,
        "a link has no origin of its own: it isn't followed"
    );

    let garbage = dir.join("garbage.txt");
    #[allow(clippy::disallowed_methods)] // test fixture
    std::fs::write(&garbage, "x").unwrap();
    set_xattr(
        &garbage,
        "com.apple.metadata:kMDItemWhereFroms",
        b"not a plist",
    )
    .unwrap();
    assert_eq!(
        NativeDownloadOrigins.origin_of(&garbage).unwrap(),
        None,
        "an unreadable record is no record"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_screenshots_are_what_the_screenshot_tool_tagged() {
    let (_keep, dir, file) = temp_with_file();
    let shot = dir.join("Schermafbeelding 2026-10-21 om 10.20.30.png");
    #[allow(clippy::disallowed_methods)] // test fixture
    std::fs::write(&shot, "png").unwrap();
    let screenshots = NativeScreenshots::new(home());
    assert!(
        !screenshots.is_screenshot(&shot).unwrap(),
        "the name alone doesn't make it one"
    );
    // What `screencapture` and the Screenshot app write on every capture.
    let tagged = bplist(plist::Value::Boolean(true));
    assert!(tagged.starts_with(b"bplist00\x09"));
    set_xattr(&shot, "com.apple.metadata:kMDItemIsScreenCapture", &tagged).unwrap();
    assert!(screenshots.is_screenshot(&shot).unwrap());
    set_xattr(
        &file,
        "com.apple.metadata:kMDItemIsScreenCapture",
        &bplist(plist::Value::Boolean(false)),
    )
    .unwrap();
    assert!(!screenshots.is_screenshot(&file).unwrap());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_reads_the_origin_browsers_record() {
    let (_keep, dir, file) = temp_with_file();
    let url = "https://example.com/files/report.pdf";
    match set_xattr(&file, "user.xdg.origin.url", url.as_bytes()) {
        Ok(()) => {}
        // Some filesystems keep no user attributes: then there's nothing to read either.
        Err(e) if e.raw_os_error() == Some(libc::ENOTSUP) => {
            eprintln!("skipped: {} keeps no user xattrs", dir.display());
            return;
        }
        Err(e) => panic!("setxattr: {e}"),
    }
    assert_eq!(
        NativeDownloadOrigins.origin_of(&file).unwrap().as_deref(),
        Some(url)
    );
    let link = dir.join("link.txt");
    #[allow(clippy::disallowed_methods)] // test fixture
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert_eq!(
        NativeDownloadOrigins.origin_of(&link).unwrap(),
        None,
        "a link has no origin of its own: it isn't followed"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn linux_screenshots_by_name_wherever_they_are() {
    let (_keep, dir, _file) = temp_with_file();
    let screenshots = NativeScreenshots::new(home());
    for name in [
        "Screenshot From 2026-10-21 10-20-30.png",
        "Schermafdruk van 2026-10-21 10-20-30.png",
        "Screenshot_20261021_102030.png",
    ] {
        assert!(
            screenshots.is_screenshot(&dir.join(name)).unwrap(),
            "{name}"
        );
    }
    assert!(!screenshots.is_screenshot(&dir.join("holiday.png")).unwrap());
    let pictures = home().join("Pictures/Screenshots/holiday.png");
    assert!(
        screenshots.is_screenshot(&pictures).unwrap(),
        "whatever it is called in ~/Pictures/Screenshots"
    );
}

#[cfg(windows)]
#[test]
fn windows_reads_the_zone_identifier_stream() {
    let (_keep, _dir, file) = temp_with_file();
    let mut stream = file.clone().into_os_string();
    stream.push(":Zone.Identifier");
    // What Edge and Chrome write next to a download.
    #[allow(clippy::disallowed_methods)] // test fixture
    std::fs::write(
        &stream,
        "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/downloads\r\nHostUrl=https://example.com/files/report.pdf\r\n",
    )
    .unwrap();
    assert_eq!(
        NativeDownloadOrigins.origin_of(&file).unwrap().as_deref(),
        Some("https://example.com/files/report.pdf")
    );
}

#[cfg(windows)]
#[test]
fn windows_reads_origins_of_files_deeper_than_260_characters() {
    let dir = tempfile::tempdir().unwrap();
    let mut deep = kipple_platform::canonical(dir.path()).unwrap();
    while deep.as_os_str().len() < 300 {
        deep.push("a-folder-with-a-long-name");
    }
    #[allow(clippy::disallowed_methods)] // test fixture
    std::fs::create_dir_all(&deep).unwrap();
    let file = deep.join("x.txt");
    #[allow(clippy::disallowed_methods)] // test fixture
    std::fs::write(&file, "x").unwrap();
    assert_eq!(NativeDownloadOrigins.origin_of(&file).unwrap(), None);
}

#[cfg(windows)]
#[test]
fn windows_screenshots_are_what_is_in_the_screenshots_folder() {
    let screenshots = NativeScreenshots::new(home());
    // Where the known folder is unless it was moved (CI machines don't move it).
    let folder = kipple_platform::canonical(&home())
        .unwrap()
        .join(r"Pictures\Screenshots");
    assert!(
        screenshots
            .is_screenshot(&folder.join("Screenshot 2026-10-21 102030.png"))
            .unwrap()
    );
    // Windows compares names case-insensitively.
    let shouting = PathBuf::from(folder.to_string_lossy().to_uppercase()).join("x.png");
    assert!(screenshots.is_screenshot(&shouting).unwrap());
    assert!(
        !screenshots
            .is_screenshot(&home().join("Screenshot 2026-10-21 102030.png"))
            .unwrap()
    );
}
