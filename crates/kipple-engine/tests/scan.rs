// SPDX-License-Identifier: GPL-3.0-or-later
//! The scanner against real temp trees: first scans, rescans with every kind of change, and the
//! folders it must never enter.
// Test fixtures change files.
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kipple_core::category::TypeCategory;
use kipple_core::enrichment::{Enrichment, MediaMeta};
use kipple_core::identity::{ContentHash, VolumeId};
use kipple_core::item::{Item, ItemKind};
use kipple_core::location::{
    Location, LocationId, LocationKind, StorageKind, TrashSupport, VolumeCaps,
};
use kipple_core::time::Timestamp;
use kipple_engine::jobs::JobError;
use kipple_engine::scan::{Control, Exclusions, ScanError, ScanOptions, ScanPorts, scan};
use kipple_index::IndexDb;
use kipple_platform::fake::FakeVolumes;
use kipple_platform::{DirEntry, Directories, NativeDirectories, Os, PortResult, ProtectedPaths};

const CAPS: VolumeCaps = VolumeCaps {
    trash: TrashSupport::Yes,
    case_sensitive: true,
    stable_file_ids: true,
    storage: StorageKind::Ssd,
};

/// A Location on a temp folder (which doubles as the home folder for the protection rules),
/// and an index for it.
struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    location: Location,
    index: IndexDb,
    volumes: FakeVolumes,
    protected: ProtectedPaths,
}

fn world() -> World {
    world_with(CAPS)
}

fn world_with(caps: VolumeCaps) -> World {
    let dir = tempfile::tempdir().unwrap();
    // Kipple's canonical form: no `\\?\` prefix on Windows, unlike `fs::canonicalize`.
    let root = kipple_platform::canonical(dir.path()).unwrap();
    let location = Location {
        id: LocationId(1),
        path: root.clone(),
        kind: LocationKind::Custom,
        enabled: true,
        volume: VolumeId("temp".into()),
        online: true,
        last_seen_online: None,
        caps,
        in_sync_root: false,
    };
    let mut index = IndexDb::open_in_memory().unwrap();
    index.put_location(&location).unwrap();
    let volumes = FakeVolumes::single("temp", root.clone(), caps);
    // Linux rules with the temp folder as home: they protect `<home>/.ssh` and don't match any
    // OS's temp folder (macOS's rules protect /private, where its temp folder is).
    let protected = ProtectedPaths::for_os(Os::Linux, &root, Path::new("/"), &[]);
    World {
        _dir: dir,
        root,
        location,
        index,
        volumes,
        protected,
    }
}

impl World {
    fn file(&self, rel: &str, content: &str) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn dir(&self, rel: &str) {
        fs::create_dir_all(self.root.join(rel)).unwrap();
    }

    fn scan_with(
        &mut self,
        directories: &dyn Directories,
        options: &ScanOptions,
    ) -> Result<kipple_engine::scan::ScanSummary, ScanError> {
        let ports = ScanPorts {
            directories,
            volumes: &self.volumes,
            protected: &self.protected,
        };
        scan(
            &self.location,
            &mut self.index,
            &ports,
            options,
            &(),
            Timestamp(1),
        )
    }

    fn scan(&mut self) -> kipple_engine::scan::ScanSummary {
        self.scan_with(&NativeDirectories, &options(Exclusions::default()))
            .unwrap()
    }

    /// Every indexed Item, by path relative to the root (`/`-separated).
    fn indexed(&self) -> BTreeMap<String, Item> {
        let mut all = BTreeMap::new();
        let mut folders = vec![self.root.clone()];
        while let Some(folder) = folders.pop() {
            for item in self.index.items_in(LocationId(1), &folder).unwrap() {
                if item.kind == ItemKind::Dir {
                    folders.push(item.identity.path.clone());
                }
                let rel = item.identity.path.strip_prefix(&self.root).unwrap();
                let rel: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
                all.insert(rel.join("/"), item);
            }
        }
        all
    }

    fn paths(&self) -> Vec<String> {
        self.indexed().into_keys().collect()
    }
}

fn options(exclusions: Exclusions) -> ScanOptions {
    ScanOptions {
        threads: 4,
        exclusions,
    }
}

#[test]
fn a_first_scan_indexes_files_and_folders_but_not_inside_opaque_ones() {
    let mut w = world();
    w.file("a.pdf", "12345");
    w.file("Photos/2025/IMG_1.jpg", "jpg");
    w.file(".hidden", "");
    w.file("repo/.git/HEAD", "ref");
    w.file("repo/main.rs", "fn main() {}");
    w.dir("Empty");

    let summary = w.scan();
    assert_eq!(
        w.paths(),
        [
            ".hidden",
            "Empty",
            "Photos",
            "Photos/2025",
            "Photos/2025/IMG_1.jpg",
            "a.pdf",
            "repo",
            "repo/.git",
            "repo/main.rs",
        ]
    );
    let items = w.indexed();
    let pdf = &items["a.pdf"];
    assert_eq!(pdf.kind, ItemKind::File);
    assert_eq!(pdf.content.size, 5);
    assert!(pdf.identity.file.is_some() && !pdf.is_hidden);
    assert_eq!(items["repo/.git"].kind, ItemKind::Bundle);
    assert_eq!(items["Photos"].kind, ItemKind::Dir);
    assert!(items[".hidden"].is_hidden, "dotfiles are hidden");
    assert_eq!((summary.added, summary.updated, summary.removed), (9, 0, 0));
    assert_eq!(summary.unreadable, 0);
}

#[test]
fn a_rescan_without_changes_writes_nothing() {
    let mut w = world();
    w.file("a.pdf", "x");
    w.file("Sub/b.pdf", "y");
    w.scan();
    let before = w.indexed();
    let again = w.scan();
    assert_eq!((again.added, again.updated, again.removed), (0, 0, 0));
    assert_eq!(again.items, 3);
    assert_eq!(w.indexed(), before, "same rows, same IDs");
}

#[test]
fn a_rescan_picks_up_every_kind_of_change() {
    let mut w = world();
    w.file("grows.txt", "1");
    w.file("same.txt", "1");
    w.file("gone.txt", "1");
    w.file("Old/inside.txt", "1");
    w.file("Old/Deeper/deep.txt", "1");
    w.file("becomes-folder", "1");
    w.file("Folder/x.txt", "1");
    w.scan();
    // Hashes, as the hasher would have stored them.
    let mut hashed: Vec<Item> = w
        .indexed()
        .into_values()
        .filter(|i| i.kind == ItemKind::File)
        .collect();
    for item in &mut hashed {
        item.content.hash = Some(ContentHash::Quick([1; 32]));
    }
    w.index.upsert_items(&hashed, Timestamp(1)).unwrap();
    let ids = w.indexed();

    w.file("grows.txt", "1234");
    fs::remove_file(w.root.join("gone.txt")).unwrap();
    fs::remove_dir_all(w.root.join("Old")).unwrap();
    fs::remove_file(w.root.join("becomes-folder")).unwrap();
    w.file("becomes-folder/new.txt", "1");
    fs::remove_dir_all(w.root.join("Folder")).unwrap();
    w.file("Folder", "now a file");
    w.file("new.txt", "1");

    let summary = w.scan();
    assert_eq!(
        w.paths(),
        [
            "Folder",
            "becomes-folder",
            "becomes-folder/new.txt",
            "grows.txt",
            "new.txt",
            "same.txt",
        ]
    );
    let items = w.indexed();
    assert_eq!(items["grows.txt"].content.size, 4);
    assert_eq!(
        items["grows.txt"].content.hash, None,
        "changed: hash dropped"
    );
    assert_eq!(
        items["same.txt"].content.hash,
        Some(ContentHash::Quick([1; 32])),
        "unchanged: hash kept"
    );
    assert_eq!(
        items["grows.txt"].id, ids["grows.txt"].id,
        "updated in place"
    );
    assert_eq!(items["Folder"].kind, ItemKind::File);
    assert_eq!(items["becomes-folder"].kind, ItemKind::Dir);
    // Added: new.txt, becomes-folder/new.txt. Updated: grows.txt, Folder, becomes-folder.
    // Removed: gone.txt, Old, Old/inside.txt, Old/Deeper, Old/Deeper/deep.txt, Folder/x.txt.
    assert_eq!((summary.added, summary.updated, summary.removed), (2, 3, 6));
}

#[test]
fn the_screenshot_flag_survives_a_change() {
    let mut w = world();
    w.file("Schermafbeelding.png", "1");
    w.scan();
    let shot = w.indexed().remove("Schermafbeelding.png").unwrap();
    let learned = Enrichment {
        item: shot.id.unwrap(),
        file: shot.identity.file,
        content: shot.content.clone(),
        is_placeholder: false,
        mime: None,
        category: TypeCategory::Screenshots,
        is_screenshot: true,
        origin_url: None,
        media: MediaMeta::default(),
    };
    assert_eq!(
        w.index.apply_enrichment(&[learned], Timestamp(1)).unwrap(),
        1
    );
    w.file("Schermafbeelding.png", "12");
    w.scan();
    let shot = &w.indexed()["Schermafbeelding.png"];
    assert!(shot.is_screenshot, "the enricher's call stays");
    assert_eq!(shot.category, TypeCategory::Screenshots);
}

#[test]
fn protected_folders_are_skipped_wherever_they_are() {
    let mut w = world();
    w.file("a.txt", "1");
    w.file(".ssh/id_ed25519", "key"); // protected in the home folder
    w.file("Drive/$Recycle.Bin/x.txt", "1"); // a system folder, anywhere
    w.file("Drive/.Trashes/y.txt", "1");
    w.file("Drive/ok.txt", "1");
    w.scan();
    assert_eq!(w.paths(), ["Drive", "Drive/ok.txt", "a.txt"]);
}

#[test]
fn exclusions_skip_folders_and_names_and_drop_what_was_indexed() {
    let mut w = world();
    w.file("keep.txt", "1");
    w.file("scratch.tmp", "1");
    w.file("app/node_modules/lib/x.js", "1");
    w.file("Private/diary.txt", "1");
    w.scan();
    assert_eq!(w.indexed().len(), 8);

    let exclusions = Exclusions::new(
        vec![w.root.join("Private")],
        &["*.tmp".into(), "node_modules".into()],
        true,
    )
    .unwrap();
    let summary = w
        .scan_with(&NativeDirectories, &options(exclusions))
        .unwrap();
    assert_eq!(w.paths(), ["app", "keep.txt"]);
    assert_eq!(summary.removed, 6);
}

#[test]
fn bad_exclusion_patterns_are_refused() {
    assert!(Exclusions::new(Vec::new(), &["[".into()], true).is_err());
}

#[test]
fn os_metadata_files_are_marked() {
    let mut w = world();
    for name in [
        "._IMG_1.jpg",
        ".DS_Store",
        "Thumbs.db",
        "desktop.ini",
        "IMG_1.jpg",
    ] {
        w.file(name, "1");
    }
    w.scan();
    let marked: Vec<_> = w
        .indexed()
        .into_iter()
        .filter(|(_, item)| item.is_os_metadata)
        .map(|(path, _)| path)
        .collect();
    assert_eq!(
        marked,
        [".DS_Store", "._IMG_1.jpg", "Thumbs.db", "desktop.ini"]
    );
    assert_eq!(w.scan().items, 1, "left out of the counts");
}

#[test]
fn without_stable_file_ids_the_path_is_the_identity() {
    let mut w = world_with(VolumeCaps {
        stable_file_ids: false,
        ..CAPS
    });
    w.file("a.txt", "1");
    w.scan();
    assert_eq!(w.indexed()["a.txt"].identity.file, None);
}

#[cfg(unix)]
#[test]
fn links_are_indexed_not_followed() {
    let mut w = world();
    w.file("Real/x.txt", "1");
    std::os::unix::fs::symlink(w.root.join("Real"), w.root.join("Link")).unwrap();
    w.scan();
    assert_eq!(w.paths(), ["Link", "Real", "Real/x.txt"]);
    assert_eq!(w.indexed()["Link"].kind, ItemKind::Symlink);
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_keeps_its_items() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = world();
    w.file("Locked/secret.txt", "1");
    w.file("open.txt", "1");
    w.scan();
    fs::set_permissions(w.root.join("Locked"), fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(w.root.join("Locked")).is_ok() {
        return; // running as root, which reads everything
    }
    let summary = w.scan();
    fs::set_permissions(w.root.join("Locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(summary.unreadable, 1);
    assert_eq!(summary.removed, 0);
    assert!(w.paths().contains(&"Locked/secret.txt".to_string()));
}

#[test]
fn an_offline_location_is_left_alone() {
    let mut w = world();
    w.file("a.txt", "1");
    w.scan();
    w.volumes.volumes[0].id = VolumeId("another-drive".into());
    fs::remove_file(w.root.join("a.txt")).unwrap();
    assert!(matches!(
        w.scan_with(&NativeDirectories, &options(Exclusions::default())),
        Err(ScanError::Offline)
    ));
    assert_eq!(
        w.paths(),
        ["a.txt"],
        "missing on an offline Location is not deleted"
    );
}

/// Marks some paths as placeholders, as a cloud folder would.
struct CloudFolder(Vec<PathBuf>);

impl Directories for CloudFolder {
    fn list(&self, dir: &Path) -> PortResult<Vec<DirEntry>> {
        let mut entries = NativeDirectories.list(dir)?;
        for entry in &mut entries {
            entry.placeholder = self.0.contains(&dir.join(&entry.name));
        }
        Ok(entries)
    }
}

#[test]
fn placeholders_are_recorded_and_online_only_folders_keep_their_items() {
    // Online-only folders aren't listed: what the index knew about them stays.
    let mut w = world();
    w.file("Cloud/online-only.pdf", "1");
    w.file("Cloud/Folder/inside.pdf", "1");
    w.file("Cloud/local.pdf", "1");
    w.scan();
    let cloud = CloudFolder(vec![
        w.root.join("Cloud/online-only.pdf"),
        w.root.join("Cloud/Folder"),
    ]);
    w.scan_with(&cloud, &options(Exclusions::default()))
        .unwrap();
    let items = w.indexed();
    assert!(items["Cloud/online-only.pdf"].is_placeholder);
    assert!(!items["Cloud/local.pdf"].is_placeholder);
    assert!(items["Cloud/Folder"].is_placeholder);
    assert!(
        items.contains_key("Cloud/Folder/inside.pdf"),
        "an online-only folder isn't listed, and its Items stay"
    );
}

/// Cancels at the first checkpoint, and records progress.
struct CancelAfter {
    checkpoints: Mutex<usize>,
    limit: usize,
    progress: Mutex<Vec<u64>>,
}

impl Control for CancelAfter {
    fn checkpoint(&self) -> Result<(), JobError> {
        let mut n = self.checkpoints.lock().unwrap();
        *n += 1;
        if *n > self.limit {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn progress(&self, done: u64, _total: Option<u64>) {
        self.progress.lock().unwrap().push(done);
    }
}

#[test]
fn a_scan_stops_at_a_folder_boundary_when_cancelled_and_reports_progress() {
    let mut w = world();
    for i in 0..20 {
        w.file(&format!("Folder {i}/x.txt"), "1");
    }
    let ports = ScanPorts {
        directories: &NativeDirectories,
        volumes: &w.volumes,
        protected: &w.protected,
    };
    let full = CancelAfter {
        checkpoints: Mutex::new(0),
        limit: usize::MAX,
        progress: Mutex::default(),
    };
    let opts = options(Exclusions::default());
    scan(
        &w.location,
        &mut w.index,
        &ports,
        &opts,
        &full,
        Timestamp(1),
    )
    .unwrap();
    let progress = full.progress.lock().unwrap().clone();
    assert_eq!(progress.last(), Some(&40), "every item counted");
    assert!(progress.windows(2).all(|p| p[0] <= p[1]), "never goes back");
    assert_eq!(*full.checkpoints.lock().unwrap(), 21, "one per folder");

    let cancel = CancelAfter {
        checkpoints: Mutex::new(0),
        limit: 3,
        progress: Mutex::default(),
    };
    assert!(matches!(
        scan(
            &w.location,
            &mut w.index,
            &ports,
            &opts,
            &cancel,
            Timestamp(2)
        ),
        Err(ScanError::Cancelled)
    ));
}

/// Panics when listing one folder, like an adapter bug would.
struct Broken(PathBuf);

impl Directories for Broken {
    fn list(&self, dir: &Path) -> PortResult<Vec<DirEntry>> {
        assert!(dir != self.0, "adapter bug");
        NativeDirectories.list(dir)
    }
}

#[test]
fn a_panicking_listing_ends_the_scan_instead_of_hanging_it() {
    let mut w = world();
    for i in 0..10 {
        w.file(&format!("Folder {i}/x.txt"), "1");
    }
    let broken = Broken(w.root.join("Folder 3"));
    let (done, finished) = std::sync::mpsc::channel();
    // Detached, so a regression fails this test instead of hanging the run.
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.scan_with(&broken, &options(Exclusions::default()))
        }));
        let _ = done.send(result.is_err());
    });
    assert_eq!(
        finished.recv_timeout(std::time::Duration::from_secs(10)),
        Ok(true),
        "the panic reaches the caller (the job turns it into a failure)"
    );
}
