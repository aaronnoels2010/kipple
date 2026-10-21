// SPDX-License-Identifier: GPL-3.0-or-later
//! In-memory fakes of the ports, for engine and app tests on any machine (ADR-0005).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{Placeholders, PortError, PortResult, SyncRoot, SyncRoots, VolumeInfo, Volumes};

/// Volumes by mount point: a path belongs to the longest mount point it lies within.
#[derive(Debug, Default, Clone)]
pub struct FakeVolumes {
    pub volumes: Vec<VolumeInfo>,
}

impl FakeVolumes {
    /// One local, writable volume `id` mounted at `mount_point`.
    pub fn single(id: &str, mount_point: PathBuf, caps: kipple_core::location::VolumeCaps) -> Self {
        Self {
            volumes: vec![VolumeInfo {
                id: kipple_core::identity::VolumeId(id.into()),
                mount_point,
                fs_type: "fakefs".into(),
                read_only: false,
                caps,
            }],
        }
    }
}

impl Volumes for FakeVolumes {
    fn volume_of(&self, path: &Path) -> PortResult<VolumeInfo> {
        self.volumes
            .iter()
            .filter(|v| kipple_core::paths::is_within(&v.mount_point, path, v.caps.case_sensitive))
            .max_by_key(|v| v.mount_point.components().count())
            .cloned()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound).into())
    }
}

/// Placeholders by path; `None` makes the port `Unsupported` (like Linux).
#[derive(Debug, Default, Clone)]
pub struct FakePlaceholders {
    pub placeholders: Option<BTreeMap<PathBuf, bool>>,
}

impl Placeholders for FakePlaceholders {
    fn is_placeholder(&self, path: &Path) -> PortResult<bool> {
        let known = self.placeholders.as_ref().ok_or(PortError::Unsupported)?;
        Ok(known.get(path).copied().unwrap_or(false))
    }
}

/// A fixed list of sync roots; `None` makes the port `Unsupported`.
#[derive(Debug, Default, Clone)]
pub struct FakeSyncRoots {
    pub roots: Option<Vec<SyncRoot>>,
}

impl SyncRoots for FakeSyncRoots {
    fn sync_roots(&self) -> PortResult<Vec<SyncRoot>> {
        self.roots.clone().ok_or(PortError::Unsupported)
    }
}
