// SPDX-License-Identifier: GPL-3.0-or-later
//! Scanning a Location (ARCHITECTURE §7.1): a parallel walk, one folder at a time, compared with
//! that folder's rows in the index.
//!
//! Worker threads list folders (one bulk listing each, never opening a file) and queue the
//! subfolders they find. The calling thread is the only writer: for each listed folder it loads
//! only that folder's rows (`WHERE parent_path = ?`, never the whole Location, spike S4) and
//! writes what changed. Pause and cancel take effect between folders.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::sync::{Condvar, Mutex, PoisonError};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use kipple_core::category::TypeCategory;
use kipple_core::identity::{ContentStamp, Identity};
use kipple_core::item::{Item, ItemId, ItemKind};
use kipple_core::location::{Location, StorageKind};
use kipple_core::paths::is_within;
use kipple_core::time::Timestamp;
use kipple_index::IndexDb;
use kipple_platform::{DirEntry, Directories, EntryKind, PortError, ProtectedPaths, Volumes};

use crate::jobs::{JobContext, JobError};
use crate::locations::is_online;

/// How a scan reports progress and learns about pause and cancel: a job's [`JobContext`], or
/// `()` to run to the end.
pub trait Control: Sync {
    /// Called by the walker before each folder.
    fn checkpoint(&self) -> Result<(), JobError>;
    fn progress(&self, done: u64, total: Option<u64>);
}

impl Control for JobContext {
    fn checkpoint(&self) -> Result<(), JobError> {
        JobContext::checkpoint(self)
    }

    fn progress(&self, done: u64, total: Option<u64>) {
        JobContext::progress(self, done, total);
    }
}

impl Control for () {
    fn checkpoint(&self) -> Result<(), JobError> {
        Ok(())
    }

    fn progress(&self, _done: u64, _total: Option<u64>) {}
}

/// The OS facts a scan needs.
pub struct ScanPorts<'a> {
    pub directories: &'a dyn Directories,
    pub volumes: &'a dyn Volumes,
    pub protected: &'a ProtectedPaths,
}

impl std::fmt::Debug for ScanPorts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanPorts").finish_non_exhaustive()
    }
}

/// What the user told Kipple to leave alone: folders by path, and names by glob pattern
/// (`*.tmp`, `node_modules`). Skipped like protected paths, and dropped from the index.
#[derive(Debug, Clone, Default)]
pub struct Exclusions {
    folders: Vec<PathBuf>,
    names: GlobSet,
    case_sensitive: bool,
}

impl Exclusions {
    pub fn new(
        folders: Vec<PathBuf>,
        name_patterns: &[String],
        case_sensitive: bool,
    ) -> Result<Self, globset::Error> {
        let mut names = GlobSetBuilder::new();
        for pattern in name_patterns {
            names.add(
                GlobBuilder::new(pattern)
                    .case_insensitive(!case_sensitive)
                    .literal_separator(true)
                    .build()?,
            );
        }
        Ok(Self {
            folders,
            names: names.build()?,
            case_sensitive,
        })
    }

    fn excludes(&self, path: &Path, name: &OsStr) -> bool {
        self.names.is_match(Path::new(name))
            || self
                .folders
                .iter()
                .any(|folder| is_within(folder, path, self.case_sensitive))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Folders listed at once: see [`walker_threads`].
    pub threads: usize,
    pub exclusions: Exclusions,
}

/// How many folders to list at once on a volume of this kind: listings are latency-bound on
/// SSDs and network shares, seek-bound on spinning disks and USB sticks.
pub const fn walker_threads(storage: StorageKind) -> usize {
    match storage {
        StorageKind::Ssd => 8,
        StorageKind::Unknown | StorageKind::Network => 4,
        StorageKind::Hdd | StorageKind::Removable => 2,
    }
}

/// What a scan found and changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub folders: u64,
    /// Every Item seen, changed or not, except OS metadata (`._*`, `.DS_Store`…).
    pub items: u64,
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    /// Folders that couldn't be listed: their Items are kept as they were.
    pub unreadable: u64,
}

/// Why a scan stopped. Each has a stable code for the UI (ADR-0004).
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// Missing, or on another volume than the one it was added on: its Items are kept.
    #[error("location offline")]
    Offline,
    #[error("location unreadable")]
    Unreadable(#[source] PortError),
    #[error(transparent)]
    Index(#[from] kipple_index::Error),
    #[error("cancelled")]
    Cancelled,
}

impl ScanError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Offline => "scan_offline",
            Self::Unreadable(_) => "scan_unreadable",
            Self::Index(_) => "scan_index_failed",
            Self::Cancelled => "scan_cancelled",
        }
    }
}

impl From<ScanError> for JobError {
    fn from(error: ScanError) -> Self {
        match error {
            ScanError::Cancelled => JobError::Cancelled,
            other => JobError::Failed(other.code()),
        }
    }
}

/// Listings waiting for the writer: bounds memory when listing outpaces writing.
const LISTINGS_IN_FLIGHT: usize = 256;
/// Items written per transaction.
const BATCH: usize = 4096;
/// How long the writer waits for the next listing before committing what it has.
const IDLE: std::time::Duration = std::time::Duration::from_millis(200);

/// Scans `location` into `index`. An offline Location is left alone; a folder that can't be
/// listed keeps its Items; what's gone from a listed folder is removed, with everything under it.
pub fn scan(
    location: &Location,
    index: &mut IndexDb,
    ports: &ScanPorts,
    options: &ScanOptions,
    control: &dyn Control,
    now: Timestamp,
) -> Result<ScanSummary, ScanError> {
    if !is_online(location, ports.volumes) {
        return Err(ScanError::Offline);
    }
    let walk = Walk {
        location,
        ports,
        exclusions: &options.exclusions,
        queue: Mutex::new(Queue {
            folders: vec![location.path.clone()],
            listing: 0,
            stopped: false,
        }),
        changed: Condvar::new(),
        cancelled: AtomicBool::new(false),
    };
    let (listings, received) = sync_channel(LISTINGS_IN_FLIGHT);
    let summary = std::thread::scope(|s| {
        for _ in 0..options.threads.max(1) {
            let listings = listings.clone();
            let walk = &walk;
            s.spawn(move || walk.work(control, &listings));
        }
        drop(listings);
        let summary = write(location, index, received, control, now);
        if summary.is_err() {
            walk.stop();
        }
        summary
    })?;
    if walk.cancelled.load(Ordering::Acquire) {
        return Err(ScanError::Cancelled);
    }
    Ok(summary)
}

/// A listed entry, and what happens to what's inside it.
#[derive(Debug)]
struct Listed {
    item: Item,
    contents: Contents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contents {
    /// A folder the walk goes into.
    Walk,
    /// An online-only folder: not listed (that would fetch it from the cloud, or fail with
    /// downloads off), and what the index knows about its contents stays.
    Keep,
    /// Not this Location's (another volume mounted here), or not a folder at all: nothing of
    /// it belongs in the index.
    Drop,
}

#[derive(Debug)]
struct Listing {
    folder: PathBuf,
    entries: Result<Vec<Listed>, PortError>,
}

struct Queue {
    /// Waiting to be listed: a stack, so the walk goes deep first and the queue stays short.
    folders: Vec<PathBuf>,
    /// Being listed right now: they may add more.
    listing: usize,
    stopped: bool,
}

struct Walk<'a> {
    location: &'a Location,
    ports: &'a ScanPorts<'a>,
    exclusions: &'a Exclusions,
    queue: Mutex<Queue>,
    changed: Condvar,
    cancelled: AtomicBool,
}

/// A folder being listed. Dropped when its subfolders are queued, or when the listing panicked:
/// then the walk stops, so the other workers don't wait for it forever (the panic then fails
/// the job).
struct InFlight<'w, 'a>(&'w Walk<'a>);

impl Drop for InFlight<'_, '_> {
    fn drop(&mut self) {
        let mut queue = self.0.lock();
        queue.listing -= 1;
        if std::thread::panicking() {
            queue.stopped = true;
        }
        drop(queue);
        self.0.changed.notify_all();
    }
}

impl Walk<'_> {
    fn work(&self, control: &dyn Control, listings: &SyncSender<Listing>) {
        while let Some(folder) = self.next() {
            let listing = InFlight(self);
            if control.checkpoint().is_err() {
                self.cancelled.store(true, Ordering::Release);
                self.stop();
                return;
            }
            let found = self.list(folder);
            let subfolders = found
                .entries
                .iter()
                .flatten()
                .filter(|l| l.contents == Contents::Walk);
            self.lock()
                .folders
                .extend(subfolders.map(|l| l.item.identity.path.clone()));
            drop(listing);
            if listings.send(found).is_err() {
                // The writer gave up.
                self.stop();
                return;
            }
        }
    }

    /// The next folder to list; `None` once there's nothing left (or the walk stopped).
    fn next(&self) -> Option<PathBuf> {
        let mut queue = self.lock();
        loop {
            if queue.stopped {
                return None;
            }
            if let Some(folder) = queue.folders.pop() {
                queue.listing += 1;
                return Some(folder);
            }
            if queue.listing == 0 {
                return None;
            }
            queue = self
                .changed
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn stop(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The folder's entries as Items, without the ones Kipple must leave alone.
    fn list(&self, folder: PathBuf) -> Listing {
        let entries = self.ports.directories.list(&folder).map(|entries| {
            entries
                .into_iter()
                .filter_map(|entry| self.listed(&folder, entry))
                .collect()
        });
        Listing { folder, entries }
    }

    fn listed(&self, folder: &Path, entry: DirEntry) -> Option<Listed> {
        let path = folder.join(&entry.name);
        if entry.kind == EntryKind::Other
            || self.ports.protected.is_protected(&path)
            || self.exclusions.excludes(&path, &entry.name)
        {
            return None;
        }
        let name = entry.name.to_string_lossy();
        let (kind, contents) = match entry.kind {
            EntryKind::Dir if self.ports.protected.is_opaque_dir(&name) => {
                (ItemKind::Bundle, Contents::Drop)
            }
            EntryKind::Dir if entry.mount_point => (ItemKind::Dir, Contents::Drop),
            EntryKind::Dir if entry.placeholder => (ItemKind::Dir, Contents::Keep),
            EntryKind::Dir => (ItemKind::Dir, Contents::Walk),
            EntryKind::Symlink => (ItemKind::Symlink, Contents::Drop),
            EntryKind::File | EntryKind::Other => (ItemKind::File, Contents::Drop),
        };
        let caps = &self.location.caps;
        let item = Item {
            id: None,
            location: self.location.id,
            kind,
            identity: Identity {
                volume: self.location.volume.clone(),
                // FAT and exFAT IDs change: there the path is the identity.
                file: entry.file_id.filter(|_| caps.stable_file_ids),
                path,
            },
            content: ContentStamp {
                size: entry.size,
                modified: entry.modified,
                changed: entry.changed,
                hash: None,
            },
            created: entry.created,
            category: TypeCategory::of(kind, &name, false),
            is_hidden: entry.hidden || name.starts_with('.'),
            is_placeholder: entry.placeholder,
            is_screenshot: false,
            is_os_metadata: kind == ItemKind::File && is_os_metadata(&name),
        };
        Some(Listed { item, contents })
    }
}

/// Files the OS keeps for itself: AppleDouble companions (`._*`, created by macOS on FAT and
/// exFAT, spike S1), Finder and Explorer folder settings and thumbnail caches.
fn is_os_metadata(name: &str) -> bool {
    name.starts_with("._")
        || [".DS_Store", "Thumbs.db", "desktop.ini"]
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
}

/// The writer: compares each listing with the index and writes the difference.
fn write(
    location: &Location,
    index: &mut IndexDb,
    listings: Receiver<Listing>,
    control: &dyn Control,
    now: Timestamp,
) -> Result<ScanSummary, ScanError> {
    let mut summary = ScanSummary::default();
    let mut pending: Vec<Item> = Vec::new();
    loop {
        let listing = match listings.recv_timeout(IDLE) {
            Ok(listing) => listing,
            Err(RecvTimeoutError::Timeout) => {
                // Listing is slow (or paused): commit what's there meanwhile.
                flush(index, &mut pending, now)?;
                match listings.recv() {
                    Ok(listing) => listing,
                    Err(_) => break,
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let listed = match listing.entries {
            Ok(listed) => listed,
            Err(error) if listing.folder == location.path => {
                return Err(ScanError::Unreadable(error));
            }
            // Deleted since its parent was listed: the parent's next listing drops it.
            Err(PortError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                summary.unreadable += 1;
                continue;
            }
        };
        summary.folders += 1;
        summary.items += listed.iter().filter(|l| !l.item.is_os_metadata).count() as u64;
        let stored = index.items_in(location.id, &listing.folder)?;
        let changes = diff(listed, stored);
        if !changes.deleted.is_empty() || !changes.emptied.is_empty() {
            flush(index, &mut pending, now)?;
            // Contents first: if Kipple stops in between, the folder's row is still there and
            // the next scan finishes the job (the other way round, nothing would find them).
            for folder in &changes.emptied {
                summary.removed += index.delete_under(location.id, folder)? as u64;
            }
            summary.removed += index.delete_items(&changes.deleted)? as u64;
        }
        summary.added += changes.added;
        summary.updated += changes.updated;
        pending.extend(changes.upserts);
        if pending.len() >= BATCH {
            flush(index, &mut pending, now)?;
        }
        control.progress(summary.items, None);
    }
    flush(index, &mut pending, now)?;
    Ok(summary)
}

fn flush(index: &mut IndexDb, pending: &mut Vec<Item>, now: Timestamp) -> Result<(), ScanError> {
    if !pending.is_empty() {
        index.upsert_scanned(pending, now)?;
        pending.clear();
    }
    Ok(())
}

/// What to write for one folder.
#[derive(Debug, Default)]
struct Changes {
    upserts: Vec<Item>,
    added: u64,
    updated: u64,
    deleted: Vec<ItemId>,
    /// Folders whose contents leave the index: gone, no longer folders, or no longer walked.
    emptied: Vec<PathBuf>,
}

fn diff(listed: Vec<Listed>, stored: Vec<Item>) -> Changes {
    let mut stored: HashMap<PathBuf, Item> = stored
        .into_iter()
        .map(|item| (item.identity.path.clone(), item))
        .collect();
    let mut changes = Changes::default();
    for Listed { mut item, contents } in listed {
        let Some(old) = stored.remove(&item.identity.path) else {
            changes.added += 1;
            changes.upserts.push(item);
            continue;
        };
        if old.kind == ItemKind::Dir && contents == Contents::Drop {
            changes.emptied.push(old.identity.path.clone());
        }
        if unchanged(&item, &old) {
            continue;
        }
        // The screenshot flag is the enricher's call, not the walker's.
        if old.is_screenshot {
            item.is_screenshot = true;
            let name = item.identity.path.file_name().unwrap_or_default();
            item.category = TypeCategory::of(item.kind, &name.to_string_lossy(), true);
        }
        changes.updated += 1;
        changes.upserts.push(item);
    }
    for (path, old) in stored {
        changes.deleted.extend(old.id);
        if old.kind == ItemKind::Dir {
            changes.emptied.push(path);
        }
    }
    changes
}

/// Same file, same content stamp, same flags: nothing to write.
fn unchanged(new: &Item, old: &Item) -> bool {
    new.kind == old.kind
        && new.identity.file == old.identity.file
        && new.content.size == old.content.size
        && new.content.modified == old.content.modified
        && new.content.changed == old.content.changed
        && new.created == old.created
        && new.is_hidden == old.is_hidden
        && new.is_placeholder == old.is_placeholder
        && new.is_os_metadata == old.is_os_metadata
}
