// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike S9 (K-0.6): for each folder, PREDICT whether its volume has a Trash (the way Kipple's
//! `Trash::supports(volume)` would), then ACTUALLY trash a throwaway file and observe the outcome.
//! The safety property: never predict "has Trash" when the real outcome isn't "trashed".
//!
//!   kipple-spike-trashprobe <label>=<dir> [<label>=<dir> …]
//!
//! Throwaway code, never shipped.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod os;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod os;
#[cfg(windows)]
#[path = "windows.rs"]
mod os;

/// What really happened when we asked the OS to trash a file.
#[derive(Debug)]
pub enum Outcome {
    /// The file left its folder and was found in a Trash / Recycle Bin.
    Trashed(String),
    /// The OS refused; the file is still there. Safe.
    Refused(String),
    /// The file is gone and was NOT found in any Trash: a silent permanent delete.
    Gone(String),
}

pub struct Prediction {
    pub has_trash: bool,
    pub why: String,
}

fn main() {
    let specs: Vec<(String, PathBuf)> = std::env::args()
        .skip(1)
        .filter_map(|a| a.split_once('=').map(|(l, d)| (l.to_string(), PathBuf::from(d))))
        .collect();
    if specs.is_empty() {
        eprintln!("usage: kipple-spike-trashprobe <label>=<dir> …");
        std::process::exit(2);
    }
    println!("| Volume | File system / kind | Predicted | Why | Actual outcome | Detail | Safe? |");
    println!("|---|---|---|---|---|---|---|");
    let mut unsafe_rows = 0;
    for (label, dir) in specs {
        let kind = os::describe(&dir);
        let pred = os::predict(&dir);
        let outcome = probe(&dir, &label);
        let (actual, detail) = match &outcome {
            Outcome::Trashed(w) => ("🗑️ trashed", w.clone()),
            Outcome::Refused(e) => ("⛔ refused", e.clone()),
            Outcome::Gone(w) => ("💀 PERMANENTLY DELETED", w.clone()),
        };
        // Unsafe = we would have called "trash" (predicted yes) but it did not end up in a Trash.
        let safe = !pred.has_trash || matches!(outcome, Outcome::Trashed(_));
        if !safe { unsafe_rows += 1; }
        println!(
            "| {label} | {kind} | {} | {} | {actual} | {} | {} |",
            if pred.has_trash { "has Trash" } else { "no Trash" },
            pred.why.replace('|', "/"),
            detail.replace('|', "/"),
            if safe { "✅" } else { "❌ UNSAFE" }
        );
    }
    println!();
    println!("Unsafe predictions: {unsafe_rows}");
    if unsafe_rows > 0 { std::process::exit(1); }
}

/// Creates a uniquely named throwaway file in `dir`, asks the OS to trash it, and classifies.
fn probe(dir: &Path, label: &str) -> Outcome {
    let name = format!("kipple-s9-{}-{}.txt", std::process::id(), label.replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
    let file = dir.join(&name);
    if let Err(e) = fs::write(&file, "kipple spike s9: safe to delete") {
        return Outcome::Refused(format!("could not create test file: {e}"));
    }
    let ctx = os::before(dir);
    let result = os::trash(&file);
    let still_there = file.exists();
    match (result, still_there) {
        (_, true) => {
            let _ = fs::remove_file(&file);
            Outcome::Refused("file still in place".into())
        }
        (Err(e), false) => match os::locate(dir, &name, &ctx) {
            Some(w) => Outcome::Trashed(format!("{w} (but the call reported: {e})")),
            None => Outcome::Gone(format!("call failed ({e}) yet the file is gone")),
        },
        (Ok(()), false) => match os::locate(dir, &name, &ctx) {
            Some(w) => {
                os::cleanup(&w);
                Outcome::Trashed(w)
            }
            None => Outcome::Gone("call succeeded, file not found in any Trash".into()),
        },
    }
}
