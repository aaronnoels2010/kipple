// SPDX-License-Identifier: GPL-3.0-or-later
//! Lexical path checks shared by Locations, protected paths and the validator.

use std::ffi::OsStr;
use std::path::{Component, Path, Prefix};

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
            .is_some_and(|c| same_component(r, c, case_sensitive))
    })
}

fn same_component(a: Component, b: Component, case_sensitive: bool) -> bool {
    match (a, b) {
        (Component::Prefix(a), Component::Prefix(b)) => same_prefix(a.kind(), b.kind()),
        _ => same_name(a.as_os_str(), b.as_os_str(), case_sensitive),
    }
}

/// Windows drive and share prefixes, with the verbatim forms (`\\?\C:`, `\\?\UNC\nas\share`,
/// which canonicalize returns) equal to the plain ones. Drive letters and server names are
/// always compared without case.
fn same_prefix(a: Prefix, b: Prefix) -> bool {
    fn plain(p: Prefix) -> Prefix {
        match p {
            Prefix::VerbatimDisk(d) => Prefix::Disk(d),
            Prefix::VerbatimUNC(server, share) => Prefix::UNC(server, share),
            other => other,
        }
    }
    match (plain(a), plain(b)) {
        (Prefix::Disk(x), Prefix::Disk(y)) => x.eq_ignore_ascii_case(&y),
        (Prefix::UNC(s1, h1), Prefix::UNC(s2, h2)) => {
            same_name(s1, s2, false) && same_name(h1, h2, false)
        }
        (x, y) => x == y,
    }
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn verbatim_and_plain_windows_prefixes_are_the_same_place() {
        assert!(is_within(
            Path::new(r"C:\Windows"),
            Path::new(r"\\?\C:\Windows\x.dll"),
            false
        ));
        assert!(is_within(
            Path::new(r"\\?\C:\Users"),
            Path::new(r"c:\users\zoe"),
            false
        ));
        assert!(is_within(
            Path::new(r"\\nas\share"),
            Path::new(r"\\?\UNC\nas\share\a.pdf"),
            false
        ));
        assert!(!is_within(
            Path::new(r"C:\Windows"),
            Path::new(r"\\?\D:\Windows\x"),
            false
        ));
    }
}
