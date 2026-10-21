// SPDX-License-Identifier: GPL-3.0-or-later
//! Locations: the root folders the user lets Kipple manage, and the facts about their volume
//! (CONTEXT.md).

use std::path::{Path, PathBuf};

use crate::identity::VolumeId;
use crate::time::Timestamp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocationId(pub i64);

/// The per-user folders Kipple offers by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserFolder {
    Desktop,
    Downloads,
    Documents,
    Pictures,
    Music,
    Videos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocationKind {
    UserFolder(UserFolder),
    /// Any other folder the user added through the native folder dialog.
    Custom,
    ExternalDrive,
    NetworkDrive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageKind {
    Ssd,
    Hdd,
    Network,
    Removable,
    Unknown,
}

/// Whether deleting on a volume can go to the Trash / Recycle Bin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrashSupport {
    Yes,
    No,
    /// Couldn't tell. Treated as **No**: spike S9 found Windows network shares delete
    /// permanently without a word, so guessing "yes" is unsafe.
    Unknown,
}

// Stable codes stored in the index: never change an existing one.

impl LocationKind {
    pub const ALL: &'static [LocationKind] = &[
        Self::UserFolder(UserFolder::Desktop),
        Self::UserFolder(UserFolder::Downloads),
        Self::UserFolder(UserFolder::Documents),
        Self::UserFolder(UserFolder::Pictures),
        Self::UserFolder(UserFolder::Music),
        Self::UserFolder(UserFolder::Videos),
        Self::Custom,
        Self::ExternalDrive,
        Self::NetworkDrive,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::UserFolder(UserFolder::Desktop) => "desktop",
            Self::UserFolder(UserFolder::Downloads) => "downloads",
            Self::UserFolder(UserFolder::Documents) => "documents",
            Self::UserFolder(UserFolder::Pictures) => "pictures",
            Self::UserFolder(UserFolder::Music) => "music",
            Self::UserFolder(UserFolder::Videos) => "videos",
            Self::Custom => "custom",
            Self::ExternalDrive => "external",
            Self::NetworkDrive => "network",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.code() == code)
    }
}

impl StorageKind {
    pub const ALL: &'static [StorageKind] = &[
        Self::Ssd,
        Self::Hdd,
        Self::Network,
        Self::Removable,
        Self::Unknown,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Ssd => "ssd",
            Self::Hdd => "hdd",
            Self::Network => "network",
            Self::Removable => "removable",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.code() == code)
    }
}

impl TrashSupport {
    pub const ALL: &'static [TrashSupport] = &[Self::Yes, Self::No, Self::Unknown];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.code() == code)
    }
}

/// Per-volume facts (CONTEXT.md, "Volume capabilities").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VolumeCaps {
    pub trash: TrashSupport,
    pub case_sensitive: bool,
    /// File IDs survive renames and remounts, so [`crate::identity::Identity`] can use them.
    pub stable_file_ids: bool,
    pub storage: StorageKind,
}

impl VolumeCaps {
    /// Only an explicit "has a Trash" counts; anything else needs the permanent-delete consent.
    pub fn can_trash(&self) -> bool {
        self.trash == TrashSupport::Yes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub id: LocationId,
    pub path: PathBuf,
    pub kind: LocationKind,
    pub enabled: bool,
    pub volume: VolumeId,
    /// Checked by volume ID. Items on an offline Location are kept, never treated as deleted.
    pub online: bool,
    pub last_seen_online: Option<Timestamp>,
    pub caps: VolumeCaps,
    /// Inside a OneDrive / iCloud Drive / Dropbox folder: changes sync to the user's devices.
    pub in_sync_root: bool,
}

impl Location {
    /// Whether `path` is this Location's root or lies inside it: see [`crate::paths::is_within`]
    /// for the (strict, lexical) rules.
    pub fn contains(&self, path: &Path) -> bool {
        crate::paths::is_within(&self.path, path, self.caps.case_sensitive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(trash: TrashSupport, case_sensitive: bool) -> VolumeCaps {
        VolumeCaps {
            trash,
            case_sensitive,
            stable_file_ids: true,
            storage: StorageKind::Ssd,
        }
    }

    /// The table's Unix-style absolute paths, made absolute on Windows too (`C:/Users/…`).
    fn absolute(path: &str) -> String {
        if cfg!(windows) && path.starts_with('/') {
            format!("C:{path}")
        } else {
            path.to_string()
        }
    }

    fn location(root: &str, case_sensitive: bool) -> Location {
        Location {
            id: LocationId(1),
            path: root.into(),
            kind: LocationKind::UserFolder(UserFolder::Downloads),
            enabled: true,
            volume: VolumeId("v1".into()),
            online: true,
            last_seen_online: None,
            caps: caps(TrashSupport::Yes, case_sensitive),
            in_sync_root: false,
        }
    }

    #[test]
    fn codes_round_trip_and_are_unique() {
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            all: &[T],
            code: fn(T) -> &'static str,
            from: fn(&str) -> Option<T>,
        ) {
            let codes: std::collections::BTreeSet<_> = all.iter().map(|v| code(*v)).collect();
            assert_eq!(codes.len(), all.len(), "duplicate codes");
            for v in all {
                assert_eq!(from(code(*v)), Some(*v));
            }
            assert_eq!(from("nope"), None);
        }
        check(
            LocationKind::ALL,
            LocationKind::code,
            LocationKind::from_code,
        );
        check(StorageKind::ALL, StorageKind::code, StorageKind::from_code);
        check(
            TrashSupport::ALL,
            TrashSupport::code,
            TrashSupport::from_code,
        );
        assert_eq!(
            LocationKind::ALL.len(),
            9,
            "every LocationKind needs a code"
        );
    }

    #[test]
    fn only_a_known_trash_counts() {
        assert!(caps(TrashSupport::Yes, false).can_trash());
        assert!(!caps(TrashSupport::No, false).can_trash());
        assert!(!caps(TrashSupport::Unknown, false).can_trash());
    }

    #[test]
    fn contains_compares_whole_components() {
        #[rustfmt::skip]
        let cases = [
            // (root, case_sensitive, path, inside)
            ("/Users/zoe/Downloads", true, "/Users/zoe/Downloads", true),
            ("/Users/zoe/Downloads", true, "/Users/zoe/Downloads/a/b.pdf", true),
            ("/Users/zoe/Downloads", true, "/Users/zoe/Downloads2/b.pdf", false),
            ("/Users/zoe/Downloads", true, "/Users/zoe", false),
            ("/Users/zoe/Downloads", true, "/Users/zoe/downloads/b.pdf", false),
            ("/Users/zoe/Downloads", false, "/Users/zoe/downloads/b.pdf", true),
            ("/Users/zoe/Downloads", false, "/USERS/ZOE/DOWNLOADS", true),
            ("/Users/zoe/Downloads/", true, "/Users/zoe/Downloads/b.pdf", true),
            ("/Users/zoë/Downloads", false, "/Users/ZOË/Downloads/b.pdf", true),
            // `..` could climb out: never "inside", whatever comes after it.
            ("/Users/zoe/Downloads", true, "/Users/zoe/Downloads/../../../etc/passwd", false),
            ("/Users/zoe/Downloads", true, "/Users/zoe/Downloads/a/../b.pdf", false),
            // An empty or relative root contains nothing.
            ("", true, "/etc/passwd", false),
            ("Downloads", true, "Downloads/b.pdf", false),
            // Unicode isn't normalised here: callers pass NFC (macOS hands out NFD names).
            ("/Users/zoë/Downloads", false, "/Users/zoe\u{0308}/Downloads/b.pdf", false),
        ];
        for (root, case_sensitive, path, inside) in cases {
            let (root, path) = (absolute(root), absolute(path));
            assert_eq!(
                location(&root, case_sensitive).contains(Path::new(&path)),
                inside,
                "{root} (case-sensitive: {case_sensitive}) ∋ {path}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn names_that_are_not_utf8_compare_as_bytes() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let root = Path::new(OsStr::from_bytes(b"/mnt/usb/\xff"));
        let mut loc = location("/", false);
        loc.path = root.into();
        assert!(loc.contains(&root.join("a")));
        assert!(!loc.contains(Path::new(OsStr::from_bytes(b"/mnt/usb/\xfe/secret"))));
    }
}
