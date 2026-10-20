# Spike S4 (K-0.5): scan performance at 100k and 1M files

**Question:** can Kipple meet its performance targets (FUNCTIONAL_PLAN §0) on all three OSes?
- first scan of 100k files < 60 s,
- rescan of 100k unchanged files < 5 s,
- up to 1M files.

The scan includes **file IDs**, which identity tracking needs (ARCHITECTURE §6), and writes to SQLite the way the real index will.

**Windows question:** Rust's standard library can't read NTFS file IDs without opening every file. Is that too slow, and does reading IDs in bulk per folder (`FileIdBothDirectoryInfo`) fix it?

## What it does

```sh
./run.sh                    # generate 100k + 1M file trees in out/, then benchmark
SIZES="100000" ./run.sh     # quicker
```

`kipple-spike-scan gen` builds a deterministic, realistic tree:
- names like `IMG_…JPG`, `Schermafbeelding …png`, `invoice-…pdf`;
- mostly small folders, plus some "Downloads-like" folders with hundreds of files;
- up to 10 levels deep;
- an opaque `.git` folder, an `.app` bundle, and hidden files.

File sizes are tiny or sparse, so 1M files take about 40 MB of disk.

`bench` then measures three phases:
1. **first scan:** walk, read metadata, insert into a fresh SQLite index;
2. **rescan, unchanged:** load the index, walk again, compare;
3. **rescan after ~1% of files changed.**

Modes:
| Mode | OS | How metadata and file ID are read |
|---|---|---|
| `lstat` | macOS, Linux | `ignore` parallel walker + `lstat` (the inode comes for free) |
| `std` | Windows | walker + find-data metadata, **no file ID** (baseline) |
| `handle` | Windows | + open a handle per file → `GetFileInformationByHandle` |
| `dirinfo` | Windows | own parallel walk, `GetFileInformationByHandleEx(FileIdBothDirectoryInfo)` per folder, **no per-file opens** |

Windows, Linux and macOS runners: **Actions → "Spike S4 · scan benchmark" → Run workflow** (`.github/workflows/spike-scan-bench.yml`). On Linux the page cache is dropped before each first scan, so that scan is **cold**.

## Results

### macOS 27.0.1 · Apple M-series · 8 threads · warm cache (local, 2026-10-20)

| Tree | Phase | Entries | Time | Rate | Peak RSS |
|---|---|---|---|---|---|
| 100k | first scan | 106,889 | **0.38 s** | 281k/s | 14 MB |
| 100k | rescan, unchanged | 106,889 | **0.22 s** | 489k/s | 42 MB |
| 100k | rescan, 999 changed | 106,889 | 0.25 s | 433k/s | 42 MB |
| 1M | first scan | 1,039,290 | **5.48 s** | 190k/s | 20 MB |
| 1M | rescan, unchanged | 1,039,290 | **4.34 s** | 239k/s | **393 MB** |
| 1M | rescan, 9,999 changed | 1,039,290 | 4.56 s | 228k/s | 404 MB |

Generating the 1M tree took 99 s.

*Warm cache:* the tree had just been written. A cold first scan needs `sudo purge` on macOS, so cold numbers come from the Linux CI run.

### GitHub Actions runners (2026-10-20, [run 39439434545](https://github.com/aaronnoels2010/kipple/actions/runs/39439434545))

| OS (runner) | Mode | 100k first scan | 100k rescan | 1M first scan | 1M rescan | 1M peak RSS |
|---|---|---|---|---|---|---|
| Linux x86_64, 4 threads, **cold cache** | `lstat` | **0.65 s** | 0.24 s | **6.31 s** | 2.56 s | 316 MB |
| Windows x86_64, 4 threads | `std` (no file ID) | 0.38 s | 0.24 s | 4.53 s | 3.21 s | 317 MB |
| Windows | `handle` (per-file open) | 1.02 s | 0.94 s | 9.40 s | 9.73 s | 317 MB |
| Windows | **`dirinfo`** (bulk per folder) | **0.31 s** | **0.22 s** | **3.36 s** | **3.38 s** | 316 MB |
| macOS arm64 VM, 3 threads | `lstat` | 1.60 s | 0.84 s | 78.1 s | 38.3 s | 387 MB |

Generating the trees took 2 s / 28 s on Linux, 8 s / 76 s on Windows and 10 s / 102 s on macOS.

## Findings

1. **The targets are met everywhere with a wide margin.** The worst 100k first scan is 1.6 s (target 60 s), and the worst 100k rescan is 0.94 s (target 5 s), on the slowest strategy.
2. **Windows: use `FileIdBothDirectoryInfo` per directory (`dirinfo`).** It is the fastest strategy, faster even than the baseline that skips file IDs, and it delivers **file ID + ChangeTime + size + mtime in one bulk call per folder**. Opening every file for its ID (`handle`) is about 3× slower. The fallback is `std` + `handle` when the call isn't supported (some network shares). On FAT/ExFAT, where `FileId` isn't stable, fall back to the path as identity.
3. **Memory in the rescan diff:** a whole-tree `HashMap` costs **about 320–400 MB at 1M files** on every OS. The real scanner (K-2.6) must diff **per directory** instead. When the walker enters a folder, it loads only that folder's rows (`WHERE parent_path = ?`) and compares them, so memory stays flat. Storing `parent_path` + `name` instead of full paths also shrinks the index.
4. **The slow macOS VM:** 1M files took 78 s for the first scan and 38 s for a rescan on a 3-core CI VM with limited RAM, so the metadata no longer fitted in the cache. That's still acceptable for a *first* scan with progress UI, but frequent **full rescans of 1M-file trees are too slow on modest Macs**. For v1 watch mode, persist the **FSEvents event ID** (macOS) and the **NTFS USN journal position** (Windows), so a restart only replays changes instead of rescanning. Linux keeps periodic rescans, which are fast there.
5. **NTFS stale metadata (likely):** in the Windows `std` run, directly after generating the tree, an "unchanged" rescan reported 135 (100k) / 1,716 (1M) *changed* files. The likely cause is that NTFS updates directory-entry metadata lazily, so enumeration can briefly show old size/mtime for recently written files. Consequences:
   - false "changed" results are harmless (re-hash);
   - a briefly *missed* change is covered by the Validator **re-stat-ing every item at apply time** (ARCHITECTURE §7.2);
   - **duplicates are re-hashed before trashing.**
6. **The SQLite insert isn't a bottleneck:** batches of 5,000 rows in WAL mode keep up with the walker on every OS.
