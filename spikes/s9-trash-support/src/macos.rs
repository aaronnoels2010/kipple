// SPDX-License-Identifier: GPL-3.0-or-later
//! macOS: predict via `URLForDirectory(NSTrashDirectory, appropriateForURL:)` + statfs flags;
//! trash via `trashItemAtURL:resultingItemURL:`.

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use objc2::rc::Retained;
use objc2_foundation::{NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask, NSString, NSURL};

use crate::Prediction;

const MNT_RDONLY: u32 = 0x0000_0001;
const MNT_LOCAL: u32 = 0x0000_1000;

thread_local! { static LAST: RefCell<Option<String>> = const { RefCell::new(None) }; }

pub struct Ctx;

fn statfs(dir: &Path) -> Option<(String, u32)> {
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: valid C string and an owned, zeroed out-struct.
    if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 { return None; }
    let fstype = unsafe { CStr::from_ptr(s.f_fstypename.as_ptr()) }.to_string_lossy().into_owned();
    Some((fstype, s.f_flags))
}

fn url(p: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))
}

pub fn describe(dir: &Path) -> String {
    match statfs(dir) {
        Some((t, f)) => format!(
            "{t}{}{}",
            if f & MNT_LOCAL != 0 { " · local" } else { " · NOT local" },
            if f & MNT_RDONLY != 0 { " · read-only" } else { "" }
        ),
        None => "statfs failed".into(),
    }
}

pub fn predict(dir: &Path) -> Prediction {
    let (local, rdonly) = statfs(dir).map(|(_, f)| (f & MNT_LOCAL != 0, f & MNT_RDONLY != 0)).unwrap_or((false, true));
    let fm = NSFileManager::defaultManager();
    let ask = |create: bool| {
        #[allow(unused_unsafe)]
        let r = unsafe {
            fm.URLForDirectory_inDomain_appropriateForURL_create_error(
                NSSearchPathDirectory::TrashDirectory,
                NSSearchPathDomainMask::UserDomainMask,
                Some(&url(dir)),
                create,
            )
        };
        r.map(|u| u.path().map(|p| p.to_string()).unwrap_or_default()).map_err(|e| e.localizedDescription().to_string())
    };
    // create:NO only finds an existing per-user Trash; create:YES makes <vol>/.Trashes/<uid> like Finder does.
    let (api_ok, api) = match (ask(false), ask(true)) {
        (Ok(p), _) => (true, format!("existing trash dir {p}")),
        (Err(_), Ok(p)) => (true, format!("trash dir created {p} (create:NO said unsupported)")),
        (Err(e), Err(_)) => (false, format!("no trash dir ({e})")),
    };
    Prediction {
        has_trash: api_ok && local && !rdonly,
        why: format!("{api}; local={local}; read-only={rdonly}"),
    }
}

pub fn before(_dir: &Path) -> Ctx { Ctx }

pub fn trash(file: &Path) -> Result<(), String> {
    let fm = NSFileManager::defaultManager();
    let mut resulting: Option<Retained<NSURL>> = None;
    #[allow(unused_unsafe)]
    let res = unsafe { fm.trashItemAtURL_resultingItemURL_error(&url(file), Some(&mut resulting)) };
    LAST.with(|l| *l.borrow_mut() = resulting.and_then(|u| u.path()).map(|p| p.to_string()));
    res.map_err(|e| e.localizedDescription().to_string())
}

pub fn locate(_dir: &Path, _name: &str, _ctx: &Ctx) -> Option<String> {
    LAST.with(|l| l.borrow().clone()).filter(|p| Path::new(p).exists())
}

pub fn cleanup(trashed: &str) {
    // Bring the item out of the Trash again (allowed without Full Disk Access) and delete it.
    let p = Path::new(trashed);
    let tmp = std::env::temp_dir().join(p.file_name().unwrap_or_default());
    if std::fs::rename(p, &tmp).is_ok() { let _ = std::fs::remove_file(tmp); }
}
