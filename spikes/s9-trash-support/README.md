# Spike S9 (K-0.6): which volumes have a Trash, and can Kipple tell in advance?

**Question:** "Move to Trash" is only safe if the volume really has a Trash or Recycle Bin. Where it doesn't, the OS may refuse, or it may **delete the file permanently**. Can Kipple predict this *before* acting (`Trash::supports(volume)`, ARCHITECTURE §7.2 `NoTrashOnVolume`)?

**Method:** for every volume, the probe:
1. **predicts** "has Trash / no Trash" with the rule Kipple would use,
2. **trashes a throwaway file** and classifies what really happened: 🗑️ trashed, ⛔ refused, or 💀 permanently deleted.

The safety property is **never predict "has Trash" when the outcome isn't "trashed"**.

```sh
./run-macos.sh                      # local: home + APFS / ExFAT / FAT32 / read-only disk images
# Linux + Windows + macOS runners:  Actions → "Spike S9 · trash support" → Run workflow
```

## Results (2026-10-20, [run 39442818598](https://github.com/aaronnoels2010/kipple/actions/runs/39442818598), re-run after the Linux fix: [39443140609](https://github.com/aaronnoels2010/kipple/actions/runs/39443140609))

**Unsafe predictions: 0 on every OS.**

| OS | Volume | Predicted | Actual |
|---|---|---|---|
| macOS (local + CI) | home (APFS) | has Trash | 🗑️ `~/.Trash` |
| | APFS / ExFAT / FAT32 drive | has Trash | 🗑️ `<vol>/.Trashes/<uid>` |
| | read-only drive | no Trash | ⛔ refused |
| Linux (Ubuntu 24.04) | home, /tmp (ext4) | has Trash ¹ | 🗑️ `~/.local/share/Trash` |
| | ext4 / vfat / exFAT / tmpfs mounts | has Trash | 🗑️ `<mount>/.Trash-<uid>` |
| | **NFS4** share | no Trash | 🗑️ `<mount>/.Trash-<uid>` (works, but treated as no Trash ²) |
| | **SMB3** share | no Trash | 🗑️ `<mount>/.Trash-<uid>` (same) |
| Windows (Server 2025) | C:, NTFS / FAT32 / exFAT fixed disks | has Trash | 🗑️ Recycle Bin of that drive |
| | **SMB share as drive letter `S:`** | no Trash | 💀 **PERMANENTLY DELETED**, and the call reported *success* |
| | **SMB share as UNC path `\\host\share`** | no Trash | 💀 **PERMANENTLY DELETED**, and the call reported *success* |

¹ The first run predicted "no Trash" for the Linux home folder: `~/.local/share/Trash` didn't exist yet on the fresh runner, and the check didn't look at the nearest existing ancestor. That was safe but wrong; it is fixed and was confirmed in a re-run.
² Conservative on purpose (see finding 3).

**Not covered:**
- Windows **removable** drives (USB sticks): CI can't simulate them. Windows has no Recycle Bin on `DRIVE_REMOVABLE`, so they're predicted "no Trash".
- macOS network shares: they aren't `MNT_LOCAL`, so they're predicted "no Trash".

## Findings → architecture

1. **This confirms the review's critical finding (C1): on Windows network drives, "move to Trash" is a silent permanent delete.** `IFileOperation` via the `trash` crate returns **success** and the file is gone. Kipple **must never call trash there**. The pre-check is the only protection: `NoTrashOnVolume` is blocking, with explicit per-batch consent for a permanent delete, and rules can never pass it.
2. **The `supports(volume)` rules that passed:**
   - **macOS:** the volume is `MNT_LOCAL` and not `MNT_RDONLY`, and `URLForDirectory(NSTrashDirectory, appropriateForURL: <dir>, create: **YES**)` succeeds. ⚠️ With `create: NO`, external volumes report "unsupported" until `<vol>/.Trashes/<uid>` exists; `create: YES` makes it, just like Finder does.
   - **Windows:** `GetDriveTypeW` is `DRIVE_FIXED` **and** `SHQueryRecycleBinW(<root>)` returns `S_OK`. FAT32 and exFAT fixed disks do have a Recycle Bin. Remote, removable and anything else count as no Trash.
   - **Linux:** the fs type is not a network fs (nfs, cifs, smb3, sshfs, …) and the mount isn't read-only. Then either it's on the same device as the home Trash (checked via its nearest existing ancestor), or `<mount>/.Trash-<uid>` exists or `<mount>` is writable (topdir trash, freedesktop spec).
3. **Linux network shares do support the freedesktop topdir Trash** (`.Trash-<uid>` on the share). For the MVP Kipple still treats them as "no Trash", for two reasons: other OSes using the same NAS won't see that Trash, and it fills the server silently. **Revisit for v1 NAS support:** allow it with a clear notice.
4. **The home Trash may not exist yet** on a fresh Linux account. Prediction must not depend on it existing.
