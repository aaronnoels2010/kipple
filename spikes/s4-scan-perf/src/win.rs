// SPDX-License-Identifier: GPL-3.0-or-later
//! Windows-only paths for spike S4: file ids via a per-file handle vs. per-directory enumeration.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileIdBothDirectoryInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::{is_opaque, Rec};

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn open(p: &Path, access: u32) -> Option<HANDLE> {
    let w = wide(p);
    // SAFETY: `w` is a NUL-terminated UTF-16 path that outlives the call.
    let h = unsafe {
        CreateFileW(
            w.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE { None } else { Some(h) }
}

/// The "slow path": open every file to read its NTFS file id.
pub fn file_id_by_handle(p: &Path) -> Option<i64> {
    let h = open(p, FILE_READ_ATTRIBUTES)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `h` is a valid handle; `info` is a properly sized out-struct.
    let ok = unsafe { GetFileInformationByHandle(h, &mut info) };
    unsafe { CloseHandle(h) };
    (ok != 0).then(|| (((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64) as i64)
}

/// Lists one directory with FileIdBothDirectoryInfo: names, sizes, times AND file ids in bulk.
fn list_dir(dir: &Path, mut each: impl FnMut(Rec)) {
    let Some(h) = open(dir, FILE_LIST_DIRECTORY) else { return };
    let mut buf = vec![0u64; 8192]; // 64 KiB, 8-byte aligned as the API requires
    loop {
        // SAFETY: valid directory handle and a writable buffer of the given size.
        let ok = unsafe {
            GetFileInformationByHandleEx(h, FileIdBothDirectoryInfo, buf.as_mut_ptr() as *mut c_void, (buf.len() * 8) as u32)
        };
        if ok == 0 {
            let _done = unsafe { GetLastError() } == ERROR_NO_MORE_FILES;
            break;
        }
        let base = buf.as_ptr() as *const u8;
        let mut off = 0usize;
        loop {
            // SAFETY: the API fills the buffer with a chain of FILE_ID_BOTH_DIR_INFO records.
            let info = unsafe { &*(base.add(off) as *const FILE_ID_BOTH_DIR_INFO) };
            let name_len = info.FileNameLength as usize / 2;
            let name_ptr = info.FileName.as_ptr();
            let name = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name_ptr, name_len) });
            if name != "." && name != ".." {
                each(Rec {
                    path: dir.join(&name).to_string_lossy().into_owned(),
                    size: info.EndOfFile,
                    mtime: info.LastWriteTime,
                    ctime: info.ChangeTime,
                    ino: info.FileId,
                    dir: info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
                        && info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0,
                });
            }
            if info.NextEntryOffset == 0 { break; }
            off += info.NextEntryOffset as usize;
        }
    }
    unsafe { CloseHandle(h) };
}

/// The "fast path": own parallel walk, one bulk enumeration per directory, no per-file opens.
pub fn scan_dirinfo(root: &Path, threads: usize, tx: SyncSender<Rec>) {
    let queue = Mutex::new(VecDeque::from([root.to_path_buf()]));
    let pending = AtomicUsize::new(1);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            let tx = tx.clone();
            let (queue, pending) = (&queue, &pending);
            s.spawn(move || loop {
                let next = queue.lock().unwrap().pop_front();
                match next {
                    Some(dir) => {
                        list_dir(&dir, |r| {
                            let name = Path::new(&r.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            if r.dir && !is_opaque(&name) {
                                pending.fetch_add(1, Ordering::SeqCst);
                                queue.lock().unwrap().push_back(PathBuf::from(&r.path));
                            }
                            let _ = tx.send(r);
                        });
                        pending.fetch_sub(1, Ordering::SeqCst);
                    }
                    None if pending.load(Ordering::SeqCst) == 0 => break,
                    None => std::thread::yield_now(),
                }
            });
        }
    });
}

pub fn peak_working_set_mb() -> f64 {
    let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: pseudo-handle of the current process and a correctly sized struct.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) };
    pmc.PeakWorkingSetSize as f64 / 1_048_576.0
}
