// SPDX-License-Identifier: GPL-3.0-or-later
//! macOS and Linux directory listings: `readdir` plus an `lstat` per entry (which the walker
//! needs anyway for the inode and ctime).

use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use kipple_core::identity::FileId;
use kipple_core::time::Timestamp;

use crate::{DirEntry, Directories, EntryKind, PortResult};

#[derive(Debug, Default, Clone)]
pub struct NativeDirectories;

impl Directories for NativeDirectories {
    fn list(&self, dir: &Path) -> PortResult<Vec<DirEntry>> {
        let device = fs::metadata(dir)?.dev();
        let mut entries = Vec::new();
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            // Doesn't follow links.
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                // Deleted since the directory was read: as if it had never been there.
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let name = entry.file_name();
            let (hidden, placeholder) = flags(&name, &meta);
            entries.push(DirEntry {
                kind: kind(&meta),
                size: if meta.is_dir() { 0 } else { meta.size() },
                modified: unix_time(meta.mtime(), meta.mtime_nsec()),
                changed: Some(unix_time(meta.ctime(), meta.ctime_nsec())),
                created: meta.created().ok().map(Timestamp::from_system_time),
                file_id: Some(FileId(u128::from(meta.ino()))),
                hidden,
                placeholder,
                mount_point: meta.is_dir() && meta.dev() != device,
                name,
            });
        }
        Ok(entries)
    }
}

fn kind(meta: &Metadata) -> EntryKind {
    let t = meta.file_type();
    if t.is_symlink() {
        EntryKind::Symlink
    } else if t.is_dir() {
        EntryKind::Dir
    } else if t.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    }
}

/// (OS-hidden, placeholder).
#[cfg(target_os = "macos")]
fn flags(name: &std::ffi::OsStr, meta: &Metadata) -> (bool, bool) {
    use std::os::macos::fs::MetadataExt;
    let flags = meta.st_flags();
    let hidden = flags & libc::UF_HIDDEN != 0;
    (hidden, super::macos::is_placeholder(name, flags))
}

#[cfg(not(target_os = "macos"))]
fn flags(_name: &std::ffi::OsStr, _meta: &Metadata) -> (bool, bool) {
    (false, false)
}

fn unix_time(secs: i64, nanos: i64) -> Timestamp {
    Timestamp(Timestamp::from_secs(secs).0.saturating_add(nanos))
}
