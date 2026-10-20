// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike S4 (K-0.5): how fast can Kipple scan 100k / 1M files, including file IDs, into SQLite?
//! Throwaway code, never shipped.
//!
//!   kipple-spike-scan gen   <dir> <files>
//!   kipple-spike-scan bench <dir> <mode> [threads]
//!
//! Modes: `lstat` (Unix: walk + lstat, inode for free)
//!        `std` (Windows: walk + find-data metadata, NO file id)
//!        `handle` (Windows: + open a handle per file for the file id)
//!        `dirinfo` (Windows: own walk, FileIdBothDirectoryInfo per directory: ids without per-file opens)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::time::Instant;

#[cfg(windows)]
mod win;

/// One scanned entry, as the index would store it.
#[derive(Clone, Debug, PartialEq)]
pub struct Rec {
    pub path: String,
    pub size: i64,
    pub mtime: i64, // ns (Unix) or 100ns ticks (Windows)
    pub ctime: i64, // status change time; 0 when the mode can't get it
    pub ino: i64,   // file id; 0 when the mode can't get it
    pub dir: bool,
}

/// Bundles and VCS folders: recorded as one item, never descended into (ARCHITECTURE §7.1).
pub fn is_opaque(name: &str) -> bool {
    name == ".git" || name == ".svn" || name == ".hg" || name.ends_with(".app") || name.ends_with(".photoslibrary")
}

// ---------------------------------------------------------------- generator

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 { self.next() % n.max(1) }
}

const NAMES: [(&str, &str); 14] = [
    ("IMG_", "JPG"), ("DSC", "jpg"), ("Screenshot 2026-03-", "png"), ("Schermafbeelding 2026-04-", "png"),
    ("invoice-2025-", "pdf"), ("report_v", "docx"), ("budget ", "xlsx"), ("notes-", "md"),
    ("setup-", "dmg"), ("archive-", "zip"), ("mod_", "rs"), ("component", "tsx"), ("track ", "mp3"), ("clip_", "mp4"),
];

fn gen(root: &Path, target: u64) -> std::io::Result<()> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    fs::create_dir_all(root)?;
    let mut made = 0u64;
    let file = |dir: &Path, rng: &mut Rng, made: &mut u64| -> std::io::Result<()> {
        let (pre, ext) = NAMES[rng.below(NAMES.len() as u64) as usize];
        let p = dir.join(format!("{pre}{made:07}.{ext}"));
        let f = fs::File::create(p)?;
        f.set_len(rng.below(4096))?; // tiny or sparse: size realism doesn't matter for metadata scans
        *made += 1;
        Ok(())
    };

    // Special cases the scanner must handle: an opaque .git, an .app bundle, hidden files.
    for special in [".git/objects/ab", "Tools/Example.app/Contents/MacOS"] {
        let d = root.join(special);
        fs::create_dir_all(&d)?;
        for _ in 0..50 { file(&d, &mut rng, &mut made)?; }
    }
    fs::File::create(root.join(".DS_Store"))?;
    made += 1;

    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0u32)]);
    let mut dir_no = 0u64;
    while made < target {
        let (dir, depth) = queue.pop_front().unwrap_or_else(|| (root.to_path_buf(), 0));
        // Mostly small folders, sometimes a "Downloads-like" folder with hundreds of files.
        let files = if rng.below(20) == 0 { 200 + rng.below(400) } else { rng.below(40) };
        for _ in 0..files {
            if made >= target { break; }
            file(&dir, &mut rng, &mut made)?;
        }
        let subdirs = if depth < 9 { 1 + rng.below(5) } else { 0 };
        for _ in 0..subdirs {
            dir_no += 1;
            let d = dir.join(format!("folder {dir_no}"));
            fs::create_dir_all(&d)?;
            queue.push_back((d, depth + 1));
        }
    }
    println!("generated {made} files in {dir_no} folders under {}", root.display());
    Ok(())
}

// ---------------------------------------------------------------- scanning

fn rec_from_meta(path: &Path, m: &fs::Metadata) -> Rec {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Rec {
            path: path.to_string_lossy().into_owned(),
            size: m.size() as i64,
            mtime: m.mtime() * 1_000_000_000 + m.mtime_nsec(),
            ctime: m.ctime() * 1_000_000_000 + m.ctime_nsec(),
            ino: m.ino() as i64,
            dir: m.is_dir(),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Rec {
            path: path.to_string_lossy().into_owned(),
            size: m.file_size() as i64,
            mtime: m.last_write_time() as i64,
            ctime: 0,
            ino: 0,
            dir: m.is_dir(),
        }
    }
}

/// Walks `root` with the `ignore` parallel walker (all filters off) and sends every entry.
fn scan_walker(root: &Path, mode: &str, threads: usize, tx: SyncSender<Rec>) {
    let with_handle_id = mode == "handle";
    ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .threads(threads)
        .build_parallel()
        .run(|| {
            let tx = tx.clone();
            Box::new(move |res| {
                let Ok(entry) = res else { return ignore::WalkState::Continue };
                if entry.depth() == 0 { return ignore::WalkState::Continue; }
                let path = entry.path();
                #[cfg(unix)]
                let meta = fs::symlink_metadata(path); // explicit lstat: size, times, inode
                #[cfg(windows)]
                let meta = entry.metadata().map_err(std::io::Error::other); // from FindFirstFile data: cheap
                let Ok(meta) = meta else { return ignore::WalkState::Continue };
                #[allow(unused_mut)]
                let mut rec = rec_from_meta(path, &meta);
                #[cfg(windows)]
                if with_handle_id { rec.ino = win::file_id_by_handle(path).unwrap_or(0); }
                let _ = with_handle_id;
                let opaque = rec.dir && is_opaque(&entry.file_name().to_string_lossy());
                let _ = tx.send(rec);
                if opaque { ignore::WalkState::Skip } else { ignore::WalkState::Continue }
            })
        });
}

fn scan(root: &Path, mode: &str, threads: usize, tx: SyncSender<Rec>) {
    match mode {
        #[cfg(windows)]
        "dirinfo" => win::scan_dirinfo(root, threads, tx),
        _ => scan_walker(root, mode, threads, tx),
    }
}

// ---------------------------------------------------------------- index (SQLite)

fn open_db(path: &Path, fresh: bool) -> rusqlite::Connection {
    if fresh {
        for suffix in ["", "-wal", "-shm"] { let _ = fs::remove_file(format!("{}{suffix}", path.display())); }
    }
    let c = rusqlite::Connection::open(path).expect("open db");
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS item(path TEXT PRIMARY KEY, size INTEGER, mtime INTEGER,
           ctime INTEGER, ino INTEGER, dir INTEGER) WITHOUT ROWID;",
    ).expect("schema");
    c
}

fn write_batch(c: &mut rusqlite::Connection, recs: &[Rec]) {
    let tx = c.transaction().unwrap();
    {
        let mut st = tx.prepare_cached("INSERT OR REPLACE INTO item VALUES (?1,?2,?3,?4,?5,?6)").unwrap();
        for r in recs { st.execute(rusqlite::params![r.path, r.size, r.mtime, r.ctime, r.ino, r.dir]).unwrap(); }
    }
    tx.commit().unwrap();
}

#[derive(Default)]
struct Outcome { entries: u64, new: u64, changed: u64, deleted: u64 }

fn first_scan(c: &mut rusqlite::Connection, rx: Receiver<Rec>) -> Outcome {
    let mut out = Outcome::default();
    let mut batch = Vec::with_capacity(5000);
    for r in rx {
        out.entries += 1;
        batch.push(r);
        if batch.len() == 5000 { write_batch(c, &batch); batch.clear(); }
    }
    write_batch(c, &batch);
    out.new = out.entries;
    out
}

fn rescan(c: &mut rusqlite::Connection, rx: Receiver<Rec>) -> Outcome {
    let mut known: HashMap<String, (i64, i64, i64, i64)> = HashMap::new();
    {
        let mut st = c.prepare("SELECT path, size, mtime, ctime, ino FROM item").unwrap();
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))).unwrap();
        for row in rows { let (p, v) = row.unwrap(); known.insert(p, v); }
    }
    let mut out = Outcome::default();
    let mut dirty = Vec::new();
    for r in rx {
        out.entries += 1;
        match known.remove(&r.path) {
            None => { out.new += 1; dirty.push(r); }
            Some(v) if v != (r.size, r.mtime, r.ctime, r.ino) => { out.changed += 1; dirty.push(r); }
            Some(_) => {}
        }
    }
    out.deleted = known.len() as u64;
    write_batch(c, &dirty);
    let tx = c.transaction().unwrap();
    for p in known.keys() { tx.execute("DELETE FROM item WHERE path = ?1", [p]).unwrap(); }
    tx.commit().unwrap();
    out
}

/// Appends a byte to every 100th file (≈1%), like a user editing files between scans.
fn mutate(c: &rusqlite::Connection) -> u64 {
    let mut st = c.prepare("SELECT path FROM item WHERE dir = 0 ORDER BY path").unwrap();
    let paths: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().filter_map(Result::ok).collect();
    let mut n = 0;
    for p in paths.iter().step_by(100) {
        use std::io::Write;
        if let Ok(mut f) = fs::OpenOptions::new().append(true).open(p) { let _ = f.write_all(b"x"); n += 1; }
    }
    n
}

// ---------------------------------------------------------------- reporting

fn peak_rss_mb() -> f64 {
    #[cfg(unix)]
    {
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: getrusage writes into the zeroed struct we own.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
        let raw = ru.ru_maxrss as f64;
        if cfg!(target_os = "macos") { raw / 1_048_576.0 } else { raw / 1024.0 } // bytes vs KiB
    }
    #[cfg(windows)]
    { win::peak_working_set_mb() }
}

fn phase(name: &str, root: &Path, mode: &str, threads: usize, c: &mut rusqlite::Connection, first: bool) -> String {
    let (tx, rx) = sync_channel::<Rec>(20_000);
    let t = Instant::now();
    let root2 = root.to_path_buf();
    let mode2 = mode.to_string();
    let walker = std::thread::spawn(move || scan(&root2, &mode2, threads, tx));
    let out = if first { first_scan(c, rx) } else { rescan(c, rx) };
    walker.join().unwrap();
    let secs = t.elapsed().as_secs_f64();
    format!(
        "| {name} | {mode} | {threads} | {} | {secs:.2} s | {:.0}/s | {} / {} / {} | {:.0} MB |",
        out.entries, out.entries as f64 / secs, out.new, out.changed, out.deleted, peak_rss_mb()
    )
}

fn bench(root: &Path, mode: &str, threads: usize) {
    let db = PathBuf::from(format!("{}.{mode}.db", root.display()));
    let mut c = open_db(&db, true);
    let mut rows = vec![
        "| Phase | Mode | Threads | Entries | Time | Rate | New / changed / deleted | Peak RSS |".to_string(),
        "|---|---|---|---|---|---|---|---|".to_string(),
    ];
    rows.push(phase("first scan", root, mode, threads, &mut c, true));
    rows.push(phase("rescan, unchanged", root, mode, threads, &mut c, false));
    let touched = mutate(&c);
    rows.push(phase(&format!("rescan, {touched} files changed"), root, mode, threads, &mut c, false));
    println!("{}", rows.join("\n"));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let default_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    match args.get(1).map(String::as_str) {
        Some("gen") if args.len() >= 4 => gen(Path::new(&args[2]), args[3].parse().expect("files")).expect("generate"),
        Some("bench") if args.len() >= 4 => {
            let threads = args.get(4).and_then(|t| t.parse().ok()).unwrap_or(default_threads);
            bench(Path::new(&args[2]), &args[3], threads)
        }
        _ => {
            eprintln!("usage: kipple-spike-scan gen <dir> <files> | bench <dir> <lstat|std|handle|dirinfo> [threads]");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_dirs_are_recognised() {
        assert!(is_opaque(".git"));
        assert!(is_opaque("Example.app"));
        assert!(!is_opaque("apps"));
    }

    #[test]
    fn rescan_detects_new_changed_and_deleted() {
        let dir = std::env::temp_dir().join(format!("kipple-s4-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("sub/b.txt"), "b").unwrap();
        fs::write(dir.join("gone.txt"), "g").unwrap();
        let mode = if cfg!(windows) { "std" } else { "lstat" };
        let db = dir.with_extension("db");
        let mut c = open_db(&db, true);

        let (tx, rx) = sync_channel(100);
        scan(&dir, mode, 2, tx);
        assert_eq!(first_scan(&mut c, rx).entries, 4); // a.txt, sub, sub/b.txt, gone.txt

        fs::write(dir.join("a.txt"), "changed!").unwrap();
        fs::remove_file(dir.join("gone.txt")).unwrap();
        fs::write(dir.join("new.txt"), "n").unwrap();
        let (tx, rx) = sync_channel(100);
        scan(&dir, mode, 2, tx);
        let out = rescan(&mut c, rx);
        assert_eq!((out.new, out.changed, out.deleted), (1, 1, 1));

        let _ = fs::remove_dir_all(&dir);
        for s in ["", "-wal", "-shm"] { let _ = fs::remove_file(format!("{}{s}", db.display())); }
    }
}
