// SPDX-License-Identifier: GPL-3.0-or-later
//! K-2.2 benchmark: name search stays under 100 ms at 1M Items (ARCHITECTURE §6.1).
//! Ignored by default (it builds a 1M-row index); run it in release:
//!   cargo test -p kipple-index --release --test search_benchmark -- --ignored --nocapture

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kipple_core::category::TypeCategory;
use kipple_core::identity::{ContentStamp, FileId, Identity, VolumeId};
use kipple_core::item::{Item, ItemKind};
use kipple_core::location::{
    Location, LocationId, LocationKind, StorageKind, TrashSupport, UserFolder, VolumeCaps,
};
use kipple_core::time::Timestamp;
use kipple_index::IndexDb;

const ITEMS: u64 = 1_000_000;
const PAGE: usize = 100;
const BUDGET: Duration = Duration::from_millis(100);

const WORDS: &[&str] = &[
    "report",
    "invoice",
    "holiday",
    "photo",
    "scan",
    "contract",
    "notes",
    "draft",
    "budget",
    "meeting",
    "receipt",
    "presentation",
    "backup",
    "screenshot",
    "export",
    "final",
    "copy",
    "aangifte",
    "factuur",
    "vakantie",
    "verslag",
    "offerte",
    "planning",
    "foto",
    "kopie",
];
const EXTS: &[&str] = &[
    "jpg", "pdf", "docx", "png", "xlsx", "mp4", "zip", "txt", "heic", "dmg",
];

/// A deterministic pseudo-random sequence (no extra crates).
fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn item(n: u64, seed: &mut u64) -> Item {
    let word = |s: &mut u64| WORDS[(next(s) % WORDS.len() as u64) as usize];
    let ext = EXTS[(next(seed) % EXTS.len() as u64) as usize];
    let name = format!(
        "{} {} {:05}.{ext}",
        word(seed),
        word(seed),
        next(seed) % 100_000
    );
    let path = PathBuf::from(format!(
        "/Users/zoe/Documents/folder {:04}/{name}",
        n % 2000
    ));
    Item {
        id: None,
        location: LocationId(1),
        kind: ItemKind::File,
        category: TypeCategory::of(ItemKind::File, &name, false),
        identity: Identity {
            volume: VolumeId("v".into()),
            file: Some(FileId(n.into())),
            path,
        },
        content: ContentStamp {
            size: next(seed) % 10_000_000,
            modified: Timestamp(1_700_000_000_000_000_000),
            changed: None,
            hash: None,
        },
        created: None,
        is_hidden: false,
        is_placeholder: false,
        is_screenshot: false,
    }
}

/// The best of 5 runs (the first warms the cache).
fn time(mut f: impl FnMut() -> usize) -> (Duration, usize) {
    (0..5)
        .map(|_| {
            let start = Instant::now();
            let found = f();
            (start.elapsed(), found)
        })
        .min_by_key(|(t, _)| *t)
        .unwrap()
}

#[test]
#[ignore = "builds a 1M-row index; run in release with --ignored"]
fn search_stays_under_100_ms_at_1m_items() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = IndexDb::open(&dir.path().join("index.db")).unwrap();
    db.put_location(&Location {
        id: LocationId(1),
        path: "/Users/zoe/Documents".into(),
        kind: LocationKind::UserFolder(UserFolder::Documents),
        enabled: true,
        volume: VolumeId("v".into()),
        online: true,
        last_seen_online: None,
        caps: VolumeCaps {
            trash: TrashSupport::Yes,
            case_sensitive: false,
            stable_file_ids: true,
            storage: StorageKind::Ssd,
        },
        in_sync_root: false,
    })
    .unwrap();

    let start = Instant::now();
    let mut seed = 0x9E37_79B9_7F4A_7C15;
    for batch in 0..ITEMS / 10_000 {
        let items: Vec<_> = (0..10_000)
            .map(|i| item(batch * 10_000 + i, &mut seed))
            .collect();
        db.upsert_items(&items, Timestamp(1)).unwrap();
    }
    println!("inserted {ITEMS} items in {:.1?}", start.elapsed());

    let mut slowest = Duration::ZERO;
    for query in [
        "vakantie aangifte",
        "invoice",
        "screenshot",
        ".jpg",
        "fo",
        "zz-nothing",
    ] {
        let (took, found) = time(|| db.search(query, None, PAGE).unwrap().items.len());
        println!("search {query:<20} {found:>3} hits  {took:>9.2?}");
        slowest = slowest.max(took);
    }
    let first = db.search("invoice", None, PAGE).unwrap();
    let (took, found) = time(|| {
        db.search("invoice", first.next.as_ref(), PAGE)
            .unwrap()
            .items
            .len()
    });
    println!(
        "search {:<20} {found:>3} hits  {took:>9.2?}",
        "invoice (page 2)"
    );
    slowest = slowest.max(took);

    assert!(
        slowest < BUDGET,
        "slowest search took {slowest:?}, budget {BUDGET:?}"
    );
}
