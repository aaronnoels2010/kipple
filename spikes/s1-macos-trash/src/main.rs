// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike S1 (K-0.4): can Kipple undo a "Move to Trash" on macOS without Full Disk Access?
//!
//! For each base directory given on the command line (a folder in $HOME, mounted test volumes…)
//! it runs a set of scenarios and prints a Markdown report. Throwaway code, never shipped.
//!
//!   kipple-spike-trash <base-dir> [<base-dir>…]

use std::ffi::{c_char, c_int, c_uint, CString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2_foundation::{NSFileManager, NSString, NSURL};

extern "C" {
    /// macOS rename with flags (<stdio.h>). RENAME_EXCL fails with EEXIST instead of overwriting.
    fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
}
const RENAME_EXCL: c_uint = 0x0000_0004;

/// Moves `path` to the Trash via NSFileManager and returns where the item ended up.
fn trash(path: &Path) -> Result<PathBuf, String> {
    let fm = NSFileManager::defaultManager();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let mut resulting: Option<Retained<NSURL>> = None;
    #[allow(unused_unsafe)]
    let res = unsafe { fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting)) };
    res.map_err(|e| e.localizedDescription().to_string())?;
    let url = resulting.ok_or("no resultingItemURL returned")?;
    let p = url.path().ok_or("resultingItemURL has no path")?;
    Ok(PathBuf::from(p.to_string()))
}

/// Restores an item without ever overwriting an existing file (RENAME_EXCL).
fn restore_excl(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    let f = CString::new(from.as_os_str().as_bytes()).unwrap();
    let t = CString::new(to.as_os_str().as_bytes()).unwrap();
    // SAFETY: both pointers are valid NUL-terminated C strings for the duration of the call.
    let rc = unsafe { renamex_np(f.as_ptr(), t.as_ptr(), RENAME_EXCL) };
    if rc == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
}

/// Fallback for file systems without RENAME_EXCL (e.g. ExFAT): NSFileManager's move also
/// refuses to overwrite ("an item with the same name already exists").
fn restore_nsfm(from: &Path, to: &Path) -> Result<(), String> {
    let fm = NSFileManager::defaultManager();
    let src = NSURL::fileURLWithPath(&NSString::from_str(&from.to_string_lossy()));
    let dst = NSURL::fileURLWithPath(&NSString::from_str(&to.to_string_lossy()));
    #[allow(unused_unsafe)]
    let res = unsafe { fm.moveItemAtURL_toURL_error(&src, &dst) };
    res.map_err(|e| format!("{} (NSError {})", e.localizedDescription(), e.code()))
}

/// What the Kipple adapter would do: RENAME_EXCL, falling back to NSFileManager on ENOTSUP.
fn restore(from: &Path, to: &Path) -> Result<&'static str, String> {
    match restore_excl(from, to) {
        Ok(()) => Ok("renamex_np(RENAME_EXCL)"),
        Err(e) if e.raw_os_error() == Some(45) => restore_nsfm(from, to).map(|()| "NSFileManager move (ENOTSUP fallback)"),
        Err(e) => Err(e.to_string()),
    }
}

fn count_visible(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|d| d.filter_map(Result::ok).filter(|e| !e.file_name().to_string_lossy().starts_with("._")).count())
        .unwrap_or(0)
}

struct Report {
    rows: Vec<(String, String, String)>,
    leftovers: Vec<(PathBuf, PathBuf)>, // (in trash, restore target) for cleanup
}

impl Report {
    fn row(&mut self, scenario: &str, ok: bool, detail: impl Into<String>) {
        let mark = if ok { "✅" } else { "❌" };
        self.rows.push((scenario.into(), mark.into(), detail.into()));
    }
}

fn short(p: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    p.display().to_string().replacen(&home, "~", 1)
}

fn write(path: &Path, content: &str) {
    fs::write(path, content).expect("create test file");
}

fn run(base: &Path, r: &mut Report) {
    let work = base.join(format!("kipple-s1-{}", std::process::id()));
    fs::create_dir_all(&work).expect("create work dir");

    // a) Trash a file and get its location.
    let a = work.join("report.txt");
    write(&a, "first");
    let ino_before = fs::metadata(&a).map(|m| (m.dev(), m.ino())).ok();
    let first = match trash(&a) {
        Ok(p) => {
            r.row("a. trash file → resultingItemURL", true, short(&p));
            r.leftovers.push((p.clone(), work.join("cleanup-1.txt")));
            Some(p)
        }
        Err(e) => {
            r.row("a. trash file → resultingItemURL", false, e);
            let _ = fs::remove_dir_all(&work);
            return;
        }
    };

    // b) Is the trashed item still the same file (identity preserved)?
    if let Some(p) = &first {
        match fs::symlink_metadata(p) {
            Ok(m) => {
                let same = ino_before == Some((m.dev(), m.ino()));
                r.row("b. stat trashed item / identity (dev+inode) kept", same,
                      if same { "same dev+inode".to_string() } else { "identity changed".to_string() });
            }
            Err(e) => r.row("b. stat trashed item / identity (dev+inode) kept", false, e.to_string()),
        }
    }

    // c) Trash a second file with the same name: does the trash rename it?
    write(&a, "second");
    let second = match trash(&a) {
        Ok(p) => {
            let differs = Some(&p) != first.as_ref();
            r.row("c. same name trashed twice → distinct name", differs,
                  format!("{}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            r.leftovers.push((p.clone(), work.join("cleanup-2.txt")));
            Some(p)
        }
        Err(e) => { r.row("c. same name trashed twice → distinct name", false, e); None }
    };

    // d) Restore (undo) the second one to its original path.
    if let Some(p) = &second {
        match restore(p, &a) {
            Ok(how) => {
                let content = fs::read_to_string(&a).unwrap_or_default();
                r.row("d. restore from Trash (no FDA)", content == "second", format!("content = {content:?} · {how}"));
                r.leftovers.retain(|(t, _)| t != p);
            }
            Err(e) => r.row("d. restore from Trash (no FDA)", false, e),
        }
    }

    // e) Restore the first one while the original path is taken: must not overwrite.
    if let Some(p) = &first {
        match restore(p, &a) {
            Ok(how) => r.row("e. restore onto existing name refuses", false, format!("OVERWROTE existing file · {how}")),
            Err(e) => {
                let kept = fs::read_to_string(&a).unwrap_or_default() == "second";
                r.row("e. restore onto existing name refuses", kept, e);
                let alt = work.join("report (restored).txt");
                match restore(p, &alt) {
                    Ok(how) => { r.row("e2. restore to alternative name", true, format!("{} · {how}", short(&alt))); r.leftovers.retain(|(t, _)| t != p); }
                    Err(e) => r.row("e2. restore to alternative name", false, e),
                }
            }
        }
    }

    // f) Folder: trash + restore keeps contents (ignoring macOS "._" AppleDouble files on FAT/ExFAT).
    let dir = work.join("Holiday photos");
    fs::create_dir_all(&dir).unwrap();
    for i in 1..=3 { write(&dir.join(format!("IMG_{i}.jpg")), "x"); }
    match trash(&dir) {
        Ok(p) => match restore(&p, &dir) {
            Ok(how) => {
                let n = count_visible(&dir);
                let all = fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
                r.row("f. folder trash + restore", n == 3, format!("{n} files back (+{} ._ files) · {how}", all - n));
            }
            Err(e) => { r.row("f. folder trash + restore", false, e); r.leftovers.push((p, dir.clone())); }
        },
        Err(e) => r.row("f. folder trash + restore", false, e),
    }

    // g) Can we list the Trash folder itself (needed for "Trash size" without FDA)?
    if let Some(trash_dir) = first.as_ref().or(second.as_ref()).and_then(|p| p.parent()) {
        match fs::read_dir(trash_dir) {
            Ok(d) => r.row("g. list Trash folder (Trash size)", true, format!("{} entries in {}", d.count(), short(trash_dir))),
            Err(e) => r.row("g. list Trash folder (Trash size)", false, format!("{e} ({})", short(trash_dir))),
        }
    }

    // h) User empties the Trash before undo: simulate by deleting the trashed item.
    let h = work.join("gone.txt");
    write(&h, "bye");
    if let Ok(p) = trash(&h) {
        let deleted = fs::remove_file(&p);
        match deleted {
            Ok(()) => match restore(&p, &h) {
                Err(e) if e.contains("os error 2") || e.contains("NSError 4)") => r.row("h. undo after Trash emptied → clean NotFound", true, e),
                other => r.row("h. undo after Trash emptied → clean NotFound", false, format!("{other:?}")),
            },
            Err(e) => {
                r.row("h. delete inside Trash (simulate emptying)", false, e.to_string());
                r.leftovers.push((p, work.join("cleanup-3.txt")));
            }
        }
    }

    // i) Contrast: std::fs::rename silently overwrites (why the executor must use RENAME_EXCL).
    let (x, y) = (work.join("x.txt"), work.join("y.txt"));
    write(&x, "new");
    write(&y, "old");
    let overwrote = fs::rename(&x, &y).is_ok() && fs::read_to_string(&y).unwrap_or_default() == "new";
    r.row("i. std::fs::rename overwrites existing file", true,
          if overwrote { "yes, silently (never use it for restores)" } else { "no" });

    // j) The NSFileManager fallback must also refuse to overwrite.
    let (s, d) = (work.join("s.txt"), work.join("d.txt"));
    write(&s, "src");
    write(&d, "dst");
    match restore_nsfm(&s, &d) {
        Ok(()) => r.row("j. NSFileManager move refuses overwrite", false, "OVERWROTE existing file"),
        Err(e) => r.row("j. NSFileManager move refuses overwrite", fs::read_to_string(&d).unwrap_or_default() == "dst", e),
    }

    // Cleanup: bring leftovers back out of the Trash, then remove the work dir.
    for (t, target) in std::mem::take(&mut r.leftovers) {
        if restore(&t, &target).is_err() {
            r.row("cleanup", false, format!("left in Trash: {}", short(&t)));
        }
    }
    let _ = fs::remove_dir_all(&work);
}

fn main() {
    let bases: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if bases.is_empty() {
        eprintln!("usage: kipple-spike-trash <base-dir> [<base-dir>…]");
        std::process::exit(2);
    }
    let fda = fs::read_dir(PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".Trash")).is_ok();
    println!("Context: Full Disk Access = {}\n", if fda { "YES (results may be optimistic)" } else { "no" });
    for base in bases {
        let mut r = Report { rows: vec![], leftovers: vec![] };
        run(&base, &mut r);
        println!("### {}\n", short(&base));
        println!("| Scenario | | Detail |\n|---|---|---|");
        for (s, m, d) in &r.rows { println!("| {s} | {m} | {} |", d.replace('|', "\\|")); }
        println!();
    }
}
