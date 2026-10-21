// SPDX-License-Identifier: GPL-3.0-or-later
//! The `Directories` contract against the real adapter of the OS the tests run on: what a
//! listing reports must match what the OS says about each entry, without following links.
// Test fixtures write files.
#![allow(clippy::disallowed_methods)]

use std::fs;
use std::path::{Path, PathBuf};

use kipple_core::time::Timestamp;
use kipple_platform::{DirEntry, Directories, EntryKind, NativeDirectories};

fn temp() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    (dir, path)
}

fn list(dir: &Path) -> Vec<DirEntry> {
    let mut entries = NativeDirectories.list(dir).unwrap();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

#[test]
fn a_listing_reports_what_the_os_knows_about_each_entry() {
    let (_keep, root) = temp();
    fs::write(root.join("a.txt"), "hello").unwrap();
    fs::write(root.join(".dotfile"), "").unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/inside.txt"), "not listed").unwrap();

    let entries = list(&root);
    let names: Vec<_> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
    assert_eq!(names, [".dotfile", "a.txt", "sub"], "one level, no . or ..");

    let file = &entries[1];
    let meta = fs::symlink_metadata(root.join("a.txt")).unwrap();
    assert_eq!(file.kind, EntryKind::File);
    assert_eq!(file.size, 5);
    assert_eq!(
        file.modified,
        Timestamp::from_system_time(meta.modified().unwrap())
    );
    assert!(!file.hidden && !file.placeholder);
    assert!(file.file_id.is_some(), "every OS reports a file ID here");
    assert!(file.changed.is_some(), "ctime / ChangeTime");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(file.file_id.unwrap().0, u128::from(meta.ino()));
        let ctime = Timestamp::from_secs(meta.ctime()).0 + meta.ctime_nsec();
        assert_eq!(file.changed, Some(Timestamp(ctime)));
    }
    assert_ne!(file.file_id, entries[0].file_id, "each file its own ID");

    assert_eq!(entries[0].kind, EntryKind::File);
    assert!(
        !entries[0].hidden,
        "a dotfile isn't OS-hidden: the scanner decides that"
    );
    assert_eq!(entries[2].kind, EntryKind::Dir);
    assert_eq!(entries[2].size, 0, "folders have no size of their own");
}

#[test]
fn links_are_listed_never_followed() {
    let (_keep, root) = temp();
    fs::create_dir(root.join("target")).unwrap();
    fs::write(root.join("target/x.txt"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("target"), root.join("link")).unwrap();
    // A junction: unlike a symlink, creating one needs no special rights on Windows.
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(root.join("link"))
            .arg(root.join("target"))
            .status()
            .unwrap()
            .success()
    );
    let entries = list(&root);
    let link = entries.iter().find(|e| e.name == "link").unwrap();
    assert_eq!(link.kind, EntryKind::Symlink);
}

#[test]
fn big_folders_are_listed_completely() {
    // More than one 64 KiB listing buffer's worth on Windows.
    let (_keep, root) = temp();
    let long = "a-rather-long-file-name-to-fill-the-listing-buffer-quickly".repeat(2);
    for i in 0..1500 {
        fs::write(root.join(format!("{long}-{i:04}.txt")), "").unwrap();
    }
    assert_eq!(list(&root).len(), 1500);
}

#[test]
fn a_missing_folder_or_a_file_cannot_be_listed() {
    let (_keep, root) = temp();
    fs::write(root.join("a.txt"), "").unwrap();
    assert!(NativeDirectories.list(&root.join("missing")).is_err());
    assert!(NativeDirectories.list(&root.join("a.txt")).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn macos_hidden_flag_is_reported() {
    let (_keep, root) = temp();
    fs::write(root.join("secret.txt"), "").unwrap();
    let status = std::process::Command::new("chflags")
        .arg("hidden")
        .arg(root.join("secret.txt"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(list(&root)[0].hidden);
}

#[cfg(windows)]
#[test]
fn windows_hidden_attribute_is_reported() {
    let (_keep, root) = temp();
    fs::write(root.join("secret.txt"), "").unwrap();
    let status = std::process::Command::new("attrib")
        .arg("+h")
        .arg(root.join("secret.txt"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(list(&root)[0].hidden);
}

#[cfg(target_os = "linux")]
#[test]
fn names_that_are_not_utf8_are_kept() {
    use std::os::unix::ffi::OsStrExt;
    let (_keep, root) = temp();
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    fs::write(root.join(name), "").unwrap();
    assert_eq!(list(&root)[0].name, name);
}

#[test]
fn placeholders_are_never_downloaded_from_here_on() {
    // Nothing to observe on a machine without cloud files: the call itself must succeed.
    kipple_platform::never_download_placeholders().unwrap();
}

#[cfg(windows)]
#[test]
fn folders_deeper_than_260_characters_are_listed() {
    // Kipple's own canonical form, without the `\\?\` prefix std's canonicalize adds.
    let dir = tempfile::tempdir().unwrap();
    let mut deep = kipple_platform::canonical(dir.path()).unwrap();
    while deep.as_os_str().len() < 300 {
        deep.push("a-folder-with-a-long-name");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("x.txt"), "").unwrap();
    assert_eq!(list(&deep).len(), 1);
}
