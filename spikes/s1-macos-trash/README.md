# Spike S1 (K-0.4): undo "Move to Trash" on macOS without Full Disk Access

**Question:** can Kipple move items to the macOS Trash **and reliably put them back** (undo) without Full Disk Access, the way Finder's "Put Back" does? And what happens on external drives?

**Answer: yes.** It works on internal disks and on APFS, Mac OS Extended, ExFAT and FAT32 drives, provided Kipple:
1. uses `NSFileManager trashItemAtURL:resultingItemURL:` and **stores the resulting location**, and
2. restores with a **no-overwrite** move: `renamex_np(RENAME_EXCL)`, falling back to `NSFileManager moveItemAtURL:toURL:` on ExFAT.

## How to run

```sh
./run.sh     # builds, creates 4 throwaway disk images, runs all scenarios, writes out/report.md
```

All test files live in a `kipple-s1-<pid>` folder that is removed afterwards. Trashed test items are restored before cleanup, so nothing is left in your Trash.

## Results (macOS 27.0.1, Apple Silicon, 2026-10-20, **no Full Disk Access**)

| Scenario | Home (`~/.Trash`) | APFS | Mac OS Ext. | ExFAT | FAT32 |
|---|---|---|---|---|---|
| a. Trash returns the resulting location | ✅ `~/.Trash/…` | ✅ `<vol>/.Trashes/501/…` | ✅ | ✅ | ✅ |
| b. Item keeps its identity (dev + inode) in the Trash | ✅ | ✅ | ✅ | ✅ | ✅ |
| c. Same name trashed twice gets a **different name** | ✅ `report.txt 16-17-29-376.txt` | ✅ | ✅ | ✅ | ✅ |
| d. Restore (undo) from the Trash | ✅ | ✅ | ✅ | ✅ via fallback | ✅ |
| e. Restore onto an existing name **refuses** (no overwrite) | ✅ EEXIST | ✅ | ✅ | ✅ | ✅ |
| f. Folder trash + restore | ✅ | ✅ | ✅ | ✅ (+3 `._` files) | ✅ (+3 `._` files) |
| g. List the Trash folder (for "Trash size") | ❌ **EPERM** | ✅ | ✅ | ✅ | ✅ |
| h. Undo after the Trash was emptied gives a clean NotFound | ✅ | ✅ | ✅ | ✅ | ✅ |
| i. `std::fs::rename` **silently overwrites** | ⚠️ yes | ⚠️ yes | ⚠️ yes | ⚠️ yes | ⚠️ yes |
| j. NSFileManager move refuses to overwrite (NSError 516) | ✅ | ✅ | ✅ | ✅ | ✅ |

**Not tested:** network volumes (SMB/NFS). These are covered by spike S9 (K-0.6). The Validator already treats unknown trash support as "no Trash".

## Findings → architecture

1. **Always use the returned location.** macOS renames clashing items in the Trash by appending a timestamp *after* the extension (`report.txt 16-17-29-376.txt`), so the location can't be guessed.
2. **Never use `std::fs::rename` for restores or moves.** It silently overwrites. Every Move, Rename and Restore needs a **no-overwrite primitive**:
   - **macOS:** `renamex_np(RENAME_EXCL)`, which returns **ENOTSUP on ExFAT** when the target doesn't exist yet. Fall back to `NSFileManager moveItemAtURL:toURL:`, which also refuses to overwrite.
   - **Linux:** `renameat2(RENAME_NOREPLACE)`, with a fallback still to be designed in K-4.5 for file systems that don't support it.
   - **Windows:** `MoveFileExW` *without* `MOVEFILE_REPLACE_EXISTING`.
3. **"Trash size" for the home Trash needs Full Disk Access** (`~/.Trash` can't be listed). Trashes on external volumes (`<vol>/.Trashes/<uid>`) can be listed, so their size can be shown.
4. **Crash recovery in the home Trash** must rely on the journal: without Full Disk Access Kipple can't search `~/.Trash`. A crash *between* the trash call and the journal commit leaves the outcome `unknown`, and the user is pointed to Finder (*Put Back*). The window is tiny, but it is reported honestly.
5. **FAT and ExFAT drives get `._*` AppleDouble companion files.** The scanner should treat `._*` as OS metadata: hide it from counts, never call a folder that only contains `._*` files non-empty, and include it in the v1 `os_junk` detector.
6. **Identity (dev + inode) survives trashing on the same volume**, so identity-based reconciliation (ARCHITECTURE §7.2) works for Trash entries too.
