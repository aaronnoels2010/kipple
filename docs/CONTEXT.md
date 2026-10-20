# Kipple: Domain Language

These terms are used consistently in code, docs, UI copy keys and issues. If a term changes, update it here first.

| Term | Meaning | Not to be confused with |
|---|---|---|
| **Kipple** | (1) The app. (2) Useless clutter that piles up by itself (from Philip K. Dick). | |
| **Location** | A root folder the user lets Kipple manage: a user folder (Desktop, Downloads…), a custom folder, an external drive or a network drive. It is **online** or **offline**, checked by volume ID. Items on an offline Location are kept, never treated as deleted. | **Destination**, which is any folder picked via a native dialog (a destination token) |
| **Destination token** | A backend-issued reference to a folder the user picked in a native dialog. The frontend never sends raw paths to act on. | |
| **Volume capabilities** | Per-volume facts: has Trash, case-sensitive, stable file IDs, kind (SSD, HDD, network, removable). | |
| **Sync root** | A folder managed by a cloud sync client (OneDrive, iCloud Drive, Dropbox). Changes there sync to all the user's devices, so Kipple shows a notice. | |
| **Item** | A file, folder, symlink or **bundle** recorded in the index. | |
| **Bundle** | A folder that the OS presents as a single file (`.app`, `.photoslibrary`, `.fcpbundle`). Kipple treats it as one opaque Item and never descends into it. | |
| **Placeholder** | A cloud file that is online-only (OneDrive Files On-Demand, iCloud optimised storage, Dropbox online-only). Its contents are **never read**, because reading would trigger a download. | |
| **Protected path** | A path Kipple will never change: system dirs, app internals, Kipple's own data. | **Mark**, which is a user decision |
| **Index** | The local cache of Items and their metadata (`index.db`). It can be rebuilt by rescanning. | **Journal**, which can't be rebuilt |
| **Scan** | Walking Locations and updating the Index. | **Enrich** |
| **Enrich** | Background step that adds slower metadata: last-used, download origin, EXIF, hashes. | |
| **Last used** | The best estimate of when the user last opened an Item, combined from several **signals** (Spotlight, Recent Items, `recently-used.xbel`, atime, Kipple's own opens). It always records which signal it came from. | `modified_at` |
| **Type category** | A type bucket (Images, Screenshots, Installers…). Every Item has exactly one. | **Facet** |
| **Facet** | Any other grouping an Item can have several of: status, origin, tags, cleanup reasons. Together with the type category these make up the plan's "categories". | |
| **Tag** (v1) | A user label (+ colour) on an Item, stored in `state.db` (and as Finder tags on macOS). | **Mark** |
| **Status** | Lifecycle state: Active, Stale, Cleanup candidate, Pinned. | |
| **Detector** | A pure function that finds cleanup Candidates of one kind (`duplicate`, `installer_old`…). | **Rule** |
| **Candidate** | A cleanup suggestion: Item + detector + confidence + **reason code** + reclaimable bytes. | |
| **Reason code** | A stable identifier plus parameters (`installer_old {days: 214}`) that the UI turns into a translated sentence. The backend never produces user-facing text. | |
| **Duplicate group** | Items with identical content (same full hash), plus the suggested copy to keep. | **Similar group**: near-identical images (perceptual hash, v1) |
| **Mark** | A user decision that survives rescans and moves: *pinned* (keep, never suggest) or *never suggest* for a file, a folder, a detector or a rule. | |
| **Plan** | An ordered list of Operations that *would* change files. It is pure data, built, stored and validated by the backend, referenced by `plan_id`, re-validated at apply time, and shown as a dry run. | **Batch** |
| **Operation** | One step in a Plan: Move, Rename, Trash, CreateDir, RemoveEmptyDir, Extract, Compress, Tag. | |
| **Issue** | A Validator finding on a Plan: Conflict, Protected, Invalid name, Locked, Placeholder, Outside locations, No trash on volume, Cross-volume (MVP), Sync root notice. | |
| **Conflict** | Two items would end up with the same name. It is resolved with *keep both*, *replace* or *skip*. *Replace* trashes the existing file as its own journaled operation, so it can be undone. | |
| **No trash on volume** | A blocking Issue: the target volume has no Trash or Recycle Bin (network shares, many USB drives). Only an explicit per-batch "delete permanently" consent gets past it, and rules never can. | |
| **Apply** | Executing a Plan through the Executor. This is the **only** way Kipple changes files. | |
| **Apply policy** | Limits for an Apply (max operations, max bytes) and what to do when they're exceeded (ask or abort). | |
| **Batch** | A Plan that was applied, as recorded in the Journal, with its status (completed, partial, interrupted, rolled back). | |
| **Journal** | The write-ahead log of Batches and their entries (`state.db`). It powers History, Undo and Recovery. It is precious data. | Logs, which are for diagnostics |
| **Undo** | An inverse Plan built from Journal entries. Per entry it checks **identity**: it skips the entry if the file at the recorded location is no longer the same file, and warns if only its content changed. | **Rollback** |
| **Recovery** | On startup, finishing (*Resume*) or reverting (*Roll back*) a Batch that was interrupted by a crash or a disconnected drive. | |
| **Identity** | `volume_id + file_id` (path as fallback): *which* file this is, stable across renames and moves on the same volume. | **Content stamp** |
| **Content stamp** | size + mtime + ctime (+ hash when relevant): whether a file's *content* may have changed. | Identity |
| **Tidy preset** | A built-in, manually triggered organise action ("Sort Desktop by type"). It produces a Plan. | **Rule** |
| **Rule** (v1) | User-defined or recommended automation: conditions → actions, with a trigger (manual, schedule, new files) and a mode (auto, ask). It produces Plans. Only runs once enabled. | **Detector** |
| **Recommended rule** | A rule template that ships with Kipple. It is **disabled** until the user enables it. | |
| **Stability check** | Waiting until a new file has stopped changing (and isn't a partial download) before a rule acts on it. | |
| **Review inbox** | Where Plans from rules in *ask* mode wait for the user. | Cleanup review list |
| **Holding area** (v1) | Quarantine folder where items wait N days before going to the Trash. | Trash |
