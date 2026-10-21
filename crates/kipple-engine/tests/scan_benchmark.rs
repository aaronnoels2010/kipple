// SPDX-License-Identifier: GPL-3.0-or-later
//! K-2.6 benchmarks on generated trees (the targets come from spike S4, K-0.5):
//! - 100k files: first scan < 60 s, rescan < 5 s;
//! - 1M files: peak memory stays below 150 MB through a first scan and a rescan.
//!
//! Ignored by default (generating 1M files takes minutes); run them in release:
//!   cargo test -p kipple-engine --release --test scan_benchmark -- --ignored --nocapture --test-threads 1
// The tree generator writes files.
#![allow(clippy::disallowed_methods)]

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kipple_core::location::{
    Location, LocationId, LocationKind, StorageKind, TrashSupport, VolumeCaps,
};
use kipple_core::time::Timestamp;
use kipple_engine::scan::{ScanOptions, ScanPorts, ScanSummary, scan, walker_threads};
use kipple_index::IndexDb;
use kipple_platform::{NativeDirectories, NativeVolumes, Os, ProtectedPaths, Volumes};

/// A deterministic pseudo-random sequence (no extra crates).
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

const NAMES: [(&str, &str); 8] = [
    ("IMG_", "JPG"),
    ("Schermafbeelding 2026-04-", "png"),
    ("invoice-2025-", "pdf"),
    ("report_v", "docx"),
    ("budget ", "xlsx"),
    ("setup-", "dmg"),
    ("archive-", "zip"),
    ("clip_", "mp4"),
];

/// `files` empty files in a realistic shape (spike S4): mostly small folders, some
/// Downloads-like ones with hundreds of files, up to 10 levels deep, plus a `.git` folder.
fn generate(root: &Path, files: u64) {
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    let mut made = 0u64;
    let mut folders = 0u64;
    fs::create_dir_all(root.join("repo/.git/objects")).unwrap();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0u32)]);
    while made < files {
        let (dir, depth) = queue.pop_front().unwrap_or_else(|| (root.to_path_buf(), 0));
        let here = if next(&mut seed).is_multiple_of(20) {
            200 + next(&mut seed) % 400
        } else {
            next(&mut seed) % 40
        };
        for _ in 0..here.min(files - made) {
            let (prefix, ext) = NAMES[(next(&mut seed) % NAMES.len() as u64) as usize];
            fs::write(dir.join(format!("{prefix}{made:07}.{ext}")), "").unwrap();
            made += 1;
        }
        if depth < 9 {
            for _ in 0..1 + next(&mut seed) % 5 {
                folders += 1;
                let sub = dir.join(format!("folder {folders}"));
                fs::create_dir(&sub).unwrap();
                queue.push_back((sub, depth + 1));
            }
        }
    }
}

struct Bench {
    _dir: tempfile::TempDir,
    location: Location,
    index: IndexDb,
    volumes: NativeVolumes,
    protected: ProtectedPaths,
}

fn bench(files: u64) -> Bench {
    let dir = tempfile::tempdir().unwrap();
    let base = kipple_platform::canonical(dir.path()).unwrap();
    let root = base.join("tree");
    let started = Instant::now();
    generate(&root, files);
    eprintln!("generated {files} files in {:.1?}", started.elapsed());
    let home: PathBuf = base.clone();
    let volumes = NativeVolumes::new(home.clone());
    let volume = volumes.volume_of(&root).unwrap();
    let location = Location {
        id: LocationId(1),
        path: root,
        kind: LocationKind::Custom,
        enabled: true,
        volume: volume.id,
        online: true,
        last_seen_online: None,
        caps: VolumeCaps {
            // The SSD walker on any machine: what users with SSDs get.
            storage: StorageKind::Ssd,
            trash: TrashSupport::Unknown,
            ..volume.caps
        },
        in_sync_root: false,
    };
    let mut index = IndexDb::open_for_scan(&base.join("index.db")).unwrap();
    index.put_location(&location).unwrap();
    Bench {
        _dir: dir,
        location,
        index,
        volumes,
        // Linux rules don't touch any OS's temp folder (macOS's protect /private).
        protected: ProtectedPaths::for_os(Os::Linux, &home, Path::new("/"), &[]),
    }
}

impl Bench {
    fn scan(&mut self, what: &str) -> (ScanSummary, Duration) {
        let ports = ScanPorts {
            directories: &NativeDirectories,
            volumes: &self.volumes,
            protected: &self.protected,
        };
        let options = ScanOptions {
            threads: walker_threads(self.location.caps.storage),
            exclusions: Default::default(),
        };
        let started = Instant::now();
        let summary = scan(
            &self.location,
            &mut self.index,
            &ports,
            &options,
            &(),
            Timestamp(1),
        )
        .unwrap();
        let took = started.elapsed();
        eprintln!(
            "{what}: {} items in {took:.2?} ({:.0}/s), added {}, updated {}, peak memory {} MB",
            summary.items,
            summary.items as f64 / took.as_secs_f64(),
            summary.added,
            summary.updated,
            peak_mb()
        );
        (summary, took)
    }
}

fn peak_mb() -> u64 {
    kipple_platform::peak_memory().unwrap_or(0) / 1_048_576
}

#[test]
#[ignore = "generates 100k files; run in release with --ignored"]
fn a_100k_tree_scans_within_the_targets() {
    let mut b = bench(100_000);
    let (first, took) = b.scan("first scan");
    assert!(first.items > 100_000 && first.added == first.items);
    assert!(took < Duration::from_secs(60), "first scan took {took:?}");
    let (again, took) = b.scan("rescan");
    assert_eq!((again.added, again.updated, again.removed), (0, 0, 0));
    assert!(took < Duration::from_secs(5), "rescan took {took:?}");
}

#[test]
#[ignore = "generates 1M files; run in release with --ignored"]
fn memory_stays_flat_at_1m_files() {
    let mut b = bench(1_000_000);
    b.scan("first scan");
    let (again, _) = b.scan("rescan");
    assert_eq!((again.added, again.updated, again.removed), (0, 0, 0));
    let peak = peak_mb();
    assert!(peak < 150, "peak memory {peak} MB");
}
