// SPDX-License-Identifier: GPL-3.0-or-later
//! *Which* file this is ([`Identity`]) versus whether its content may have changed
//! ([`ContentStamp`]). See CONTEXT.md.

use std::path::PathBuf;

use crate::time::Timestamp;

/// A volume's stable ID: a filesystem UUID on macOS and Linux, the volume GUID or serial on
/// Windows. Opaque to the core.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VolumeId(pub String);

/// A file's ID on its volume: the inode on macOS and Linux, the 128-bit file ID on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u128);

/// `volume_id + file_id`, with the path as fallback: stable across renames and moves on the same
/// volume, so marks, tags and journal entries follow a file around.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    pub volume: VolumeId,
    /// `None` where the volume has no stable file IDs (some network and FAT drives).
    pub file: Option<FileId>,
    pub path: PathBuf,
}

impl Identity {
    /// Whether `self` and `other` are the same file. With file IDs on both sides, the IDs decide
    /// (a renamed or moved file is still the same file; a new file at the old path is not).
    /// Without IDs on either side, only the path can tell. With an ID on one side only, it can't
    /// be told, so the answer is no: undo and reconcile never guess (ADR-0002).
    pub fn same_file(&self, other: &Identity) -> bool {
        if self.volume != other.volume {
            return false;
        }
        match (self.file, other.file) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.path == other.path,
            _ => false,
        }
    }
}

/// A 256-bit content hash. The index keeps two kinds (ARCHITECTURE §6.1), and only the same
/// kind can be compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentHash {
    /// Of the first and last blocks plus the size: a cheap pre-filter for duplicates.
    Quick([u8; 32]),
    /// Of the whole content.
    Full([u8; 32]),
}

/// size + mtime + ctime (+ hash when known): whether a file's *content* may have changed since
/// it was seen. Used to re-check a plan's files right before applying it (ADR-0002).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentStamp {
    pub size: u64,
    pub modified: Timestamp,
    /// ctime on macOS and Linux, ChangeTime on Windows. `None` where the OS doesn't give one.
    pub changed: Option<Timestamp>,
    pub hash: Option<ContentHash>,
}

impl ContentStamp {
    /// True unless the content is known to be unchanged. Size and mtime must match, and so must
    /// ctime: a ctime known on one side only can't rule out a change. A hash only counts when both
    /// sides have one of the same kind (a hash computed later doesn't make a file "changed").
    pub fn may_differ(&self, other: &ContentStamp) -> bool {
        let hashes_differ = match (self.hash, other.hash) {
            (Some(ContentHash::Quick(a)), Some(ContentHash::Quick(b)))
            | (Some(ContentHash::Full(a)), Some(ContentHash::Full(b))) => a != b,
            _ => false,
        };
        self.size != other.size
            || self.modified != other.modified
            || self.changed != other.changed
            || hashes_differ
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(volume: &str, file: Option<u128>, path: &str) -> Identity {
        Identity {
            volume: VolumeId(volume.into()),
            file: file.map(FileId),
            path: path.into(),
        }
    }

    #[test]
    fn same_file_prefers_file_ids_over_paths() {
        #[rustfmt::skip]
        let cases = [
            // (a, b, same, why)
            (id("v1", Some(7), "/a/x.pdf"), id("v1", Some(7), "/a/x.pdf"), true, "identical"),
            (id("v1", Some(7), "/a/x.pdf"), id("v1", Some(7), "/b/renamed.pdf"), true, "moved/renamed"),
            (id("v1", Some(7), "/a/x.pdf"), id("v1", Some(8), "/a/x.pdf"), false, "replaced at the same path"),
            (id("v1", Some(7), "/a/x.pdf"), id("v2", Some(7), "/a/x.pdf"), false, "same id, other volume"),
            (id("v1", None, "/a/x.pdf"), id("v1", None, "/a/x.pdf"), true, "no ids: same path"),
            (id("v1", None, "/a/x.pdf"), id("v1", None, "/a/y.pdf"), false, "no ids: other path"),
            (id("v1", Some(7), "/a/x.pdf"), id("v1", None, "/a/x.pdf"), false, "one id missing: can't tell, so no"),
            (id("v1", None, "/a/x.pdf"), id("v2", None, "/a/x.pdf"), false, "no ids, other volume"),
        ];
        for (a, b, same, why) in cases {
            assert_eq!(a.same_file(&b), same, "{why}");
            assert_eq!(b.same_file(&a), same, "{why} (symmetric)");
        }
    }

    fn stamp(
        size: u64,
        modified: i64,
        changed: Option<i64>,
        hash: Option<ContentHash>,
    ) -> ContentStamp {
        ContentStamp {
            size,
            modified: Timestamp(modified),
            changed: changed.map(Timestamp),
            hash,
        }
    }

    const Q9: Option<ContentHash> = Some(ContentHash::Quick([9; 32]));
    const Q5: Option<ContentHash> = Some(ContentHash::Quick([5; 32]));
    const F9: Option<ContentHash> = Some(ContentHash::Full([9; 32]));

    #[test]
    fn may_differ_compares_what_both_sides_know() {
        let base = stamp(100, 1, Some(2), Q9);
        #[rustfmt::skip]
        let cases = [
            (stamp(100, 1, Some(2), Q9), false, "identical"),
            (stamp(101, 1, Some(2), Q9), true, "size"),
            (stamp(100, 3, Some(2), Q9), true, "mtime"),
            (stamp(100, 1, Some(4), Q9), true, "ctime (e.g. mtime was reset)"),
            (stamp(100, 1, Some(2), Q5), true, "hash"),
            (stamp(100, 1, None, Q9), true, "ctime known on one side only: can't rule a change out"),
            (stamp(100, 1, Some(2), None), false, "hash unknown on one side (computed later)"),
            (stamp(100, 1, Some(2), F9), false, "a quick and a full hash can't be compared"),
        ];
        for (other, differ, why) in cases {
            assert_eq!(base.may_differ(&other), differ, "{why}");
            assert_eq!(other.may_differ(&base), differ, "{why} (symmetric)");
        }
    }
}
