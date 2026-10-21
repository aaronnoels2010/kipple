// SPDX-License-Identifier: GPL-3.0-or-later
//! `<app data dir>/locations.json`: the Locations the user chose. The source of truth: index.db
//! is a disposable cache, so its `location` table only mirrors this file (ADR-0003). Kept in the
//! data folder, not the (possibly synced) config folder, because paths and volume IDs belong to
//! this computer.

use std::io;
use std::path::{Path, PathBuf};

use kipple_core::identity::VolumeId;
use kipple_core::location::{
    Location, LocationId, LocationKind, StorageKind, TrashSupport, VolumeCaps,
};
use kipple_core::time::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::own_files::{Access, set_aside, write_atomic};

const VERSION: u32 = 1;

/// What's stored: the Locations this version understands, plus anything it doesn't (written
/// back unchanged), and the next ID to hand out.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub locations: Vec<Location>,
    /// Entries with codes this version doesn't know (a newer Kipple wrote them): kept, not lost.
    pub unknown: Vec<Value>,
    /// IDs are never reused: history and marks may still refer to a removed Location's ID.
    pub next_id: i64,
}

impl Default for Stored {
    /// No Locations yet: IDs start at 1.
    fn default() -> Self {
        Self {
            locations: Vec::new(),
            unknown: Vec::new(),
            next_id: 1,
        }
    }
}

impl Stored {
    /// Hands out a new ID.
    pub fn take_id(&mut self) -> LocationId {
        let id = self.next_id;
        self.next_id = id + 1;
        LocationId(id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(default)]
    next_id: i64,
    locations: Vec<Value>,
}

/// One Location, with the stable codes from `kipple-core`. Online state isn't stored: it's
/// checked at runtime.
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    id: i64,
    path: PathBuf,
    kind: String,
    enabled: bool,
    volume_id: String,
    last_seen_online: Option<i64>,
    trash: String,
    case_sensitive: bool,
    stable_file_ids: bool,
    storage: String,
    in_sync_root: bool,
}

/// The stored Locations, all offline until checked. A missing file means none. A file that
/// isn't valid JSON is set aside (`locations.json.bad`) and Kipple starts without. Any other
/// read error (locked, no permission) is returned: the caller must not treat it as "none" and
/// overwrite the file.
pub fn load(path: &Path) -> io::Result<Stored> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Stored::default()),
        Err(e) => return Err(e),
    };
    let file: File = match serde_json::from_str(&text) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!(file = %path.display(), error = %e, "unreadable locations file, set aside");
            set_aside(path);
            return Ok(Stored::default());
        }
    };
    let mut stored = Stored {
        next_id: file.next_id,
        ..Stored::default()
    };
    for raw in file.locations {
        match serde_json::from_value::<Entry>(raw.clone())
            .ok()
            .and_then(|e| from_entry(&e))
        {
            Some(location) => stored.locations.push(location),
            None => {
                tracing::warn!(
                    "kept a location this version can't read (written by a newer Kipple?)"
                );
                stored.unknown.push(raw);
            }
        }
    }
    let highest = stored.locations.iter().map(|l| l.id.0).max().unwrap_or(0);
    let highest_unknown = stored
        .unknown
        .iter()
        .filter_map(|v| v.get("id")?.as_i64())
        .max()
        .unwrap_or(0);
    stored.next_id = stored.next_id.max(highest.max(highest_unknown) + 1);
    Ok(stored)
}

fn from_entry(e: &Entry) -> Option<Location> {
    Some(Location {
        id: LocationId(e.id),
        path: e.path.clone(),
        kind: LocationKind::from_code(&e.kind)?,
        enabled: e.enabled,
        volume: VolumeId(e.volume_id.clone()),
        online: false,
        last_seen_online: e.last_seen_online.map(Timestamp),
        caps: VolumeCaps {
            trash: TrashSupport::from_code(&e.trash)?,
            case_sensitive: e.case_sensitive,
            stable_file_ids: e.stable_file_ids,
            storage: StorageKind::from_code(&e.storage)?,
        },
        in_sync_root: e.in_sync_root,
    })
}

fn to_entry(l: &Location) -> Entry {
    Entry {
        id: l.id.0,
        path: l.path.clone(),
        kind: l.kind.code().into(),
        enabled: l.enabled,
        volume_id: l.volume.0.clone(),
        last_seen_online: l.last_seen_online.map(|t| t.0),
        trash: l.caps.trash.code().into(),
        case_sensitive: l.caps.case_sensitive,
        stable_file_ids: l.caps.stable_file_ids,
        storage: l.caps.storage.code().into(),
        in_sync_root: l.in_sync_root,
    }
}

pub fn save(path: &Path, stored: &Stored) -> io::Result<()> {
    let mut locations = Vec::with_capacity(stored.locations.len() + stored.unknown.len());
    for location in &stored.locations {
        locations.push(serde_json::to_value(to_entry(location))?);
    }
    locations.extend(stored.unknown.iter().cloned());
    let file = File {
        version: VERSION,
        next_id: stored.next_id,
        locations,
    };
    let text = serde_json::to_string_pretty(&file)? + "\n";
    write_atomic(path, text.as_bytes(), Access::Normal)
}

#[cfg(test)]
mod tests {
    use kipple_core::location::UserFolder;

    use super::*;

    fn sample() -> Vec<Location> {
        let caps = VolumeCaps {
            trash: TrashSupport::Yes,
            case_sensitive: false,
            stable_file_ids: true,
            storage: StorageKind::Ssd,
        };
        vec![
            Location {
                id: LocationId(1),
                path: "/Users/zoë/Downloads".into(),
                kind: LocationKind::UserFolder(UserFolder::Downloads),
                enabled: true,
                volume: VolumeId("ED3A-UUID".into()),
                online: false,
                last_seen_online: Some(Timestamp(1_700_000_000_000_000_000)),
                caps,
                in_sync_root: false,
            },
            Location {
                id: LocationId(4),
                path: "/Volumes/USB stick".into(),
                kind: LocationKind::ExternalDrive,
                enabled: false,
                volume: VolumeId("usb".into()),
                online: false,
                last_seen_online: None,
                caps: VolumeCaps {
                    trash: TrashSupport::No,
                    storage: StorageKind::Removable,
                    ..caps
                },
                in_sync_root: true,
            },
        ]
    }

    fn stored() -> Stored {
        Stored {
            locations: sample(),
            unknown: Vec::new(),
            next_id: 9,
        }
    }

    #[test]
    fn round_trips_and_loads_offline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locations.json");
        let mut s = stored();
        s.locations[0].online = true; // runtime state, not stored
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), stored());
    }

    #[test]
    fn a_missing_file_means_no_locations() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load(&dir.path().join("locations.json")).unwrap();
        assert!(loaded.locations.is_empty());
        assert_eq!(loaded.next_id, 1);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_not_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be: reading fails with something other than NotFound.
        #[allow(clippy::disallowed_methods)] // test fixture
        std::fs::create_dir(dir.path().join("locations.json")).unwrap();
        assert!(load(&dir.path().join("locations.json")).is_err());
    }

    #[test]
    fn a_damaged_file_is_set_aside_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locations.json");
        write_atomic(&path, b"{\"version\": 1, \"locations\": [", Access::Normal).unwrap();
        assert!(load(&path).unwrap().locations.is_empty());
        assert!(dir.path().join("locations.json.bad").exists());
    }

    #[test]
    fn entries_from_a_newer_kipple_are_kept_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locations.json");
        save(&path, &stored()).unwrap();
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"downloads\"", "\"from-the-future\"");
        write_atomic(&path, text.as_bytes(), Access::Normal).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.locations, sample()[1..]);
        assert_eq!(loaded.unknown.len(), 1);
        save(&path, &loaded).unwrap();
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("from-the-future"),
            "written back"
        );
    }

    #[test]
    fn ids_are_never_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locations.json");
        let mut s = stored();
        assert_eq!(s.take_id(), LocationId(9));
        s.locations.retain(|l| l.id != LocationId(4)); // remove the highest stored ID
        save(&path, &s).unwrap();
        let mut loaded = load(&path).unwrap();
        assert_eq!(loaded.take_id(), LocationId(10));
        // An old file without next_id still starts after the highest ID.
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\"next_id\": 10", "\"next_id\": 0");
        write_atomic(&path, text.as_bytes(), Access::Normal).unwrap();
        assert_eq!(load(&path).unwrap().take_id(), LocationId(2));
    }
}
