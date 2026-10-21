// SPDX-License-Identifier: GPL-3.0-or-later
//! Lexical path checks shared by Locations, protected paths and the validator.

use std::ffi::OsStr;
use std::path::{Component, Path};

/// Whether `path` is `root` or lies inside it, compared component by component, ignoring case
/// when `case_sensitive` is false.
///
/// Purely lexical, and strict about it: a path with `..` anywhere is never inside, and an empty
/// or relative root contains nothing. Callers resolve symlinks first and pass Unicode in NFC
/// (macOS hands out NFD names), because that isn't normalised here. Names that aren't valid
/// UTF-8 are compared as bytes only, never "lossily".
pub fn is_within(root: &Path, path: &Path, case_sensitive: bool) -> bool {
    if !root.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return false;
    }
    let mut inner = path.components();
    root.components().all(|r| {
        inner
            .next()
            .is_some_and(|c| same_name(r.as_os_str(), c.as_os_str(), case_sensitive))
    })
}

/// Whether two file names are the same, ignoring case when `case_sensitive` is false.
pub fn same_name(a: &OsStr, b: &OsStr, case_sensitive: bool) -> bool {
    if a == b {
        return true;
    }
    match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) => !case_sensitive && a.to_lowercase() == b.to_lowercase(),
        _ => false,
    }
}
