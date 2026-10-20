// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux: predict via /proc/self/mountinfo (fs type, ro flag) + freedesktop Trash rules;
//! trash via the `trash` crate.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::Prediction;

const NETWORK_FS: [&str; 12] = [
    "nfs", "nfs4", "cifs", "smb3", "smbfs", "fuse.sshfs", "9p", "afs", "ceph", "glusterfs", "fuse.rclone", "davfs",
];

pub struct Ctx;

struct Mount { point: PathBuf, fstype: String, ro: bool }

fn mount_of(dir: &Path) -> Option<Mount> {
    let canon = fs::canonicalize(dir).ok()?;
    let info = fs::read_to_string("/proc/self/mountinfo").ok()?;
    let mut best: Option<Mount> = None;
    for line in info.lines() {
        // id parent maj:min root mountpoint options [optional…] - fstype source superoptions
        let (left, right) = line.split_once(" - ")?;
        let f: Vec<&str> = left.split(' ').collect();
        let point = PathBuf::from(f.get(4)?.replace("\\040", " "));
        let ro = f.get(5).is_some_and(|o| o.split(',').any(|x| x == "ro"));
        let fstype = right.split(' ').next().unwrap_or_default().to_string();
        if canon.starts_with(&point) && best.as_ref().is_none_or(|b| point.as_os_str().len() >= b.point.as_os_str().len()) {
            best = Some(Mount { point, fstype, ro });
        }
    }
    best
}

fn home_trash() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"))
        .join("Trash")
}

fn writable(p: &Path) -> bool {
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: valid C string.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

fn uid() -> u32 { unsafe { libc::getuid() } }

pub fn describe(dir: &Path) -> String {
    match mount_of(dir) {
        Some(m) => format!("{} at {}{}", m.fstype, m.point.display(), if m.ro { " · read-only" } else { "" }),
        None => "unknown mount".into(),
    }
}

pub fn predict(dir: &Path) -> Prediction {
    let Some(m) = mount_of(dir) else { return Prediction { has_trash: false, why: "unknown mount".into() } };
    if NETWORK_FS.contains(&m.fstype.as_str()) {
        return Prediction { has_trash: false, why: format!("network fs ({})", m.fstype) };
    }
    if m.ro {
        return Prediction { has_trash: false, why: "read-only mount".into() };
    }
    let home = home_trash();
    let home_dev = fs::metadata(home.parent().unwrap_or(&home)).map(|x| x.dev()).ok();
    let dir_dev = fs::metadata(dir).map(|x| x.dev()).ok();
    if home_dev.is_some() && home_dev == dir_dev {
        return Prediction { has_trash: true, why: "same device as home Trash".into() };
    }
    let shared = m.point.join(".Trash");
    let own = m.point.join(format!(".Trash-{}", uid()));
    if own.is_dir() && writable(&own) || shared.join(uid().to_string()).is_dir() || writable(&m.point) {
        Prediction { has_trash: true, why: format!("topdir Trash possible at {}", m.point.display()) }
    } else {
        Prediction { has_trash: false, why: "topdir not writable, no .Trash-$uid".into() }
    }
}

pub fn before(_dir: &Path) -> Ctx { Ctx }

pub fn trash(file: &Path) -> Result<(), String> {
    trash::delete(file).map_err(|e| e.to_string())
}

pub fn locate(dir: &Path, name: &str, _ctx: &Ctx) -> Option<String> {
    let mut candidates = vec![home_trash().join("files").join(name)];
    if let Some(m) = mount_of(dir) {
        candidates.push(m.point.join(format!(".Trash-{}", uid())).join("files").join(name));
        candidates.push(m.point.join(".Trash").join(uid().to_string()).join("files").join(name));
    }
    candidates.into_iter().find(|c| c.exists()).map(|c| c.display().to_string())
}

pub fn cleanup(trashed: &str) {
    let p = Path::new(trashed);
    let _ = fs::remove_file(p);
    if let (Some(files), Some(n)) = (p.parent(), p.file_name()) {
        if let Some(root) = files.parent() {
            let _ = fs::remove_file(root.join("info").join(format!("{}.trashinfo", n.to_string_lossy())));
        }
    }
}
