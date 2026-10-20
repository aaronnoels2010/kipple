// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows: predict via GetDriveTypeW + SHQueryRecycleBinW; trash via the `trash` crate;
//! detect the outcome by the Recycle Bin item count of that drive.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW};
use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};

use crate::Prediction;

const DRIVE_TYPES: [&str; 7] = ["unknown", "no root", "removable", "fixed", "remote (network)", "cd-rom", "ram disk"];

pub struct Ctx { items: Option<i64> }

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

fn root_of(dir: &Path) -> String {
    let w: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: valid input string and output buffer of the given length.
    let ok = unsafe { GetVolumePathNameW(w.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if ok == 0 { return dir.display().to_string(); }
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
}

fn drive_type(root: &str) -> u32 { unsafe { GetDriveTypeW(wide(root).as_ptr()) } }

fn fs_name(root: &str) -> String {
    let mut name = vec![0u16; 64];
    // SAFETY: valid root and output buffer; other outputs are optional.
    let ok = unsafe {
        GetVolumeInformationW(wide(root).as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(),
                              std::ptr::null_mut(), name.as_mut_ptr(), name.len() as u32)
    };
    if ok == 0 { return "?".into(); }
    String::from_utf16_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(0)])
}

fn recycle_items(root: &str) -> Result<i64, i32> {
    let mut info: SHQUERYRBINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHQUERYRBINFO>() as u32;
    // SAFETY: valid root string and a correctly sized struct.
    let hr = unsafe { SHQueryRecycleBinW(wide(root).as_ptr(), &mut info) };
    if hr == 0 { Ok(info.i64NumItems) } else { Err(hr) }
}

pub fn describe(dir: &Path) -> String {
    let root = root_of(dir);
    let t = drive_type(&root) as usize;
    format!("{} on {root} · {}", fs_name(&root), DRIVE_TYPES.get(t).copied().unwrap_or("?"))
}

pub fn predict(dir: &Path) -> Prediction {
    let root = root_of(dir);
    let t = drive_type(&root);
    let q = recycle_items(&root);
    Prediction {
        has_trash: t == 3 && q.is_ok(), // DRIVE_FIXED and the Recycle Bin answers
        why: format!("drive type {}; SHQueryRecycleBin {}", DRIVE_TYPES.get(t as usize).copied().unwrap_or("?"),
                     match q { Ok(n) => format!("ok ({n} items)"), Err(hr) => format!("failed 0x{hr:08x}") }),
    }
}

pub fn before(dir: &Path) -> Ctx { Ctx { items: recycle_items(&root_of(dir)).ok() } }

pub fn trash(file: &Path) -> Result<(), String> {
    trash::delete(file).map_err(|e| e.to_string())
}

pub fn locate(dir: &Path, _name: &str, ctx: &Ctx) -> Option<String> {
    let root = root_of(dir);
    match (ctx.items, recycle_items(&root).ok()) {
        (Some(a), Some(b)) if b > a => Some(format!("Recycle Bin of {root} ({a} → {b} items)")),
        _ => None,
    }
}

pub fn cleanup(_trashed: &str) {
    // Throwaway CI runner: the test file stays in its Recycle Bin.
}
