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

### Linux / Windows (CI)

*Pending: run the workflow and paste the job summaries here.*

## Findings so far

1. **Speed is fine on macOS:** about 100× faster than the target at 100k, and the 1M rescan stays under 5 s even at 10× the target size.
2. **Memory in the rescan diff:** loading every known path into one in-memory `HashMap` costs **about 400 MB at 1M files**. The real scanner (K-2.6) should diff **per directory** instead. When the walker enters a folder, it loads only that folder's rows (`WHERE parent_path = ?`, already indexed in ARCHITECTURE §6.1) and compares them, which keeps memory flat regardless of tree size. Storing `parent_path` + `name` instead of full paths also shrinks the index.
3. **The SQLite insert isn't a bottleneck:** batches of 5,000 rows in WAL mode keep up with the walker (first scan ≈ walk speed).
4. The **Windows decision** (`handle` vs `dirinfo`) waits for the CI results.
