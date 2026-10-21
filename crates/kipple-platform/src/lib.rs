// SPDX-License-Identifier: GPL-3.0-or-later
//! OS port traits with macOS, Windows and Linux adapters, plus in-memory fakes (ADR-0005).
//! The core and the engine only see the traits; the app picks [`current`] adapters.

// FFI to OS APIs lives here, so `unsafe` is allowed in this crate; clippy requires a
// `// SAFETY:` comment on every unsafe block (undocumented_unsafe_blocks).

pub mod fake;
mod os;
mod protected;

use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::VolumeCaps;

pub use os::{NativePlaceholders, NativeSyncRoots, NativeVolumes};
pub use protected::ProtectedPaths;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    MacOs,
    Windows,
    Linux,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PortError {
    /// This OS doesn't offer it: the UI hides the feature instead of failing (ADR-0005).
    #[error("not supported on this platform")]
    Unsupported,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type PortResult<T> = Result<T, PortError>;

/// The volume a path is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    /// Stable across reboots and remounts (a filesystem UUID or volume GUID where available).
    pub id: VolumeId,
    /// Where the volume is mounted. On macOS this needn't be a prefix of the paths on it:
    /// `/Users` and `/private/var` are firmlinks into the Data volume at `/System/Volumes/Data`.
    pub mount_point: PathBuf,
    /// The filesystem type as the OS names it (`apfs`, `NTFS`, `ext4`…), for logs and rules.
    pub fs_type: String,
    pub read_only: bool,
    pub caps: VolumeCaps,
}

pub trait Volumes: Send + Sync {
    /// The volume holding `path` (which must exist).
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo>;
}

pub trait Placeholders: Send + Sync {
    /// Whether `path` is an online-only cloud file (its contents must never be read: that
    /// would download it). `Unsupported` where the OS has no such files (Linux).
    fn is_placeholder(&self, path: &Path) -> PortResult<bool>;
}

/// Who syncs a folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SyncProvider {
    ICloudDrive,
    OneDrive,
    Dropbox,
    GoogleDrive,
    /// Another sync client, by the name the OS gives it.
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncRoot {
    pub path: PathBuf,
    pub provider: SyncProvider,
}

pub trait SyncRoots: Send + Sync {
    /// The folders cloud sync clients manage for this user. `Unsupported` on Linux.
    fn sync_roots(&self) -> PortResult<Vec<SyncRoot>>;
}

/// The sync root `path` lies in, if any.
pub fn sync_root_of<'a>(
    path: &Path,
    roots: &'a [SyncRoot],
    case_sensitive: bool,
) -> Option<&'a SyncRoot> {
    roots
        .iter()
        .find(|root| kipple_core::paths::is_within(&root.path, path, case_sensitive))
}
