# Kipple: Functional Plan

**Kipple** is a free, open-source, cross-platform (Linux, macOS, Windows) desktop app that helps you **find, understand, organise, rename and safely clean up** your files.

Priority tags: **[MVP]** first release · **[v1]** full product · **[Later]** nice to have

---

## 0. Guiding principles

1. **Safe by default.** Nothing is permanently deleted without explicit consent. Every action can be undone.
2. **Explain every suggestion.** Each cleanup suggestion says *why*, e.g. "Duplicate of `~/Documents/cv.pdf`" or "Installer, app already installed, 214 days old".
3. **Review, then act.** The app proposes changes, you review them, and you apply them in one click.
4. **Local and private.** File contents never leave the machine unless an optional feature (such as AI) is explicitly enabled.
5. **Fast on big folders.** Scanning is incremental and cached, and the UI never freezes.
6. **Nothing automatic unless you switch it on.** Rules only run when you created or explicitly enabled them (§6).
7. **Free for everyone.** No paid tiers, licence keys or ads.

### Non-goals [MVP]
To keep the scope clear and the app trustworthy, this is **not a system cleaner**:
- No registry cleaning, and no app, browser or system cache cleaning
- No "speed up your PC", RAM boosting or startup-item management
- No touching of system folders, application bundles or other apps' internal data
- No secure-erase / shredding (unreliable on SSDs; users should rely on OS disk encryption)

### Privacy promise [MVP]
- **Nothing leaves your computer.** There are no accounts, no telemetry and no analytics.
- The only network call is the optional update check (§11), which can be disabled.
- Optional AI features (§10) state clearly what would be sent, and they are off by default.
- A short, plain-language privacy statement ships in-app and with every release (also needed for app/package stores).

### Performance targets [MVP]
Targets to validate with benchmarks on a mid-range laptop with an SSD:

| Scenario | Target |
|---|---|
| Index size supported | up to **1 million files** |
| First scan of 100k files (metadata only, hashing deferred) | < **60 s** |
| Re-scan of 100k unchanged files | < **5 s** |
| List scroll, filtering, selection | < **100 ms** response, no frozen UI |
| Open quick preview | < **300 ms** for common types |
| Memory use while idle in the tray | < **200 MB** |

---

## 1. Locations & scanning

| Feature | Priority |
|---|---|
| **Managed locations**: by default the user folders (Desktop, Downloads, Documents, Pictures, Music, Videos), pre-selected in onboarding and individually switchable | MVP |
| **Add any other folder or drive** as a managed location (e.g. `D:\Projects`, `/mnt/data`, a whole external disk). System paths inside it stay protected | MVP |
| Exclusion list (folders and glob patterns) and built-in protection for system/app folders | MVP |
| Background scan with progress, pause and cancel | MVP |
| Incremental re-scan from a local index cache, so only changed files are re-read | MVP |
| Live folder watching that updates the index as files change | v1 |
| External / removable drives as scan targets | v1 |
| **Network drives / NAS** (SMB, NFS, mapped drives) as scan targets, with slower-scan handling and offline detection | v1 |
| **Cloud-placeholder awareness** (OneDrive Files On-Demand, iCloud "Optimise Storage", Dropbox online-only). Never hash or preview these in a way that triggers a download | MVP |

**Metadata collected per file:** path, size, type (by extension *and* content sniffing), created, modified, last-opened, download origin, image/media metadata (EXIF, dimensions, duration), and content hash (computed lazily).

> ⚠️ "Last accessed" (atime) is unreliable on all three OSes. Determine "last used" from several signals combined:
> - macOS: Spotlight `kMDItemLastUsedDate`, `kMDItemWhereFroms`
> - Windows: Recent Items / jump lists, `Zone.Identifier` (download origin)
> - Linux: `~/.local/share/recently-used.xbel`
> - Everywhere: modified date, plus the app's own record of when you last opened a file through it

---

## 2. Categorisation

Each file has exactly one **type** category, plus any number of other groupings (status, origin, tags, cleanup reasons). The views are filters over one index.

**By type** [MVP]: Images, Screenshots, Videos, Audio, Documents, Spreadsheets, Presentations, PDFs, Archives, Installers, Code/Projects, Fonts, Ebooks, Other.

**By status / lifecycle** [MVP]:
- 🟢 **Active**: used recently
- 🟡 **Stale**: not used in X months (configurable)
- 🔴 **Cleanup candidate**: matches one or more cleanup rules (see §3)
- 📌 **Pinned / keep**: you marked it as never to be suggested again

**By origin** [v1]: downloaded from the web (with source URL), screenshot, camera import, created locally.

**By content** [Later, AI/OCR]: invoice, receipt, contract, ID document, payslip, meme, and similar.

---

## 3. Cleanup engine ("what can be deleted")

Each detector produces candidates with a **confidence level** (High / Medium / Low), a **reason**, and the **space reclaimed**.

| Detector | Example reason | Priority |
|---|---|---|
| **Exact duplicates** (size, then partial hash, then full hash) | "Same content as 2 other files" with a suggested original to keep | MVP |
| **Old installers** (`.dmg .pkg .exe .msi .deb .rpm .AppImage`) | "Installer, 180 days old, app appears installed" | MVP |
| **Extracted archives** (`foo.zip` next to `foo/`) | "Already extracted" | MVP |
| **Incomplete / temp downloads** (`.crdownload .part .tmp .download`) | "Unfinished download" | MVP |
| **Empty files & empty folders** | "0 bytes" | MVP |
| **Large & old files** | "1.4 GB, not opened in 2 years" | MVP |
| **Old screenshots** | "Screenshot older than 30 days" | MVP |
| **Similar images** (perceptual hash: burst shots, near-identical screenshots) | "4 near-identical photos, keep the sharpest" | v1 |
| **Broken shortcuts / symlinks / `.lnk`** | "Target no longer exists" | v1 |
| **Developer junk** (`node_modules`, `target/`, `.venv`, build caches in inactive projects), opt-in | "Rebuildable, project untouched 9 months" | v1 |
| **OS junk** (`.DS_Store`, `Thumbs.db`, `desktop.ini` in managed folders) | "System metadata file" | v1 |
| **Versions check**: clusters like `report_v1`, `report_v2`, `report_final`, `report_FINAL(2)` | "Older version of `report_FINAL(2).docx`, keep latest only" | v1 |

### Cleanup review UX
- **Cleanup dashboard**: total reclaimable space, broken down per detector.
- **Review list** with a preview pane. Select all per category, or go one by one.
- **Quick triage mode** [v1]: one file at a time. Press `K` to keep, `D` to delete, `A` to archive, `→` to skip.
- **Duplicate resolver**: groups side by side, with an auto-pick rule for which copy to keep (oldest, shortest path, in Documents, etc.).
- **"Never suggest again"** per file, folder or rule.

### Safety net
- Deletion goes to the **OS Trash / Recycle Bin** (freedesktop trash spec on Linux). [MVP]
- **Action journal + Undo** for every move, rename and delete, including bulk operations. [MVP]
- **Quarantine / "holding area"** [v1]: move candidates to a holding folder for N days before trashing, so you can see whether you miss anything.
- **Dry-run summary** before any bulk action: "142 files, 3.2 GB → Trash". [MVP]
- Protected paths that the app will never touch (system directories, app bundles, app internals, `.git` folders). [MVP]
- **Cloud-synced folders** (e.g. a Desktop or Documents folder moved into OneDrive, or iCloud Drive) can be managed, but Kipple shows a clear notice that changes sync to all your devices. Online-only files are never downloaded. [MVP]
- **Drives without a Trash** (network shares, many USB sticks): Kipple refuses to delete there unless you explicitly confirm "delete permanently" for that batch. Automatic rules can **never** delete permanently. [MVP]
- **"Replace" in a name conflict** moves the existing file to the Trash first, so it can be undone too. [MVP]
- **Recovery after interrupted bulk actions** [MVP]: bulk operations are written to the journal *before* they run. If the app crashes, the laptop sleeps or a drive disconnects halfway, the next start offers **Resume** or **Roll back**.
- **Name conflicts** when moving or renaming [MVP]: *Keep both* (auto-suffix), *Replace*, or *Skip*, with "apply to all remaining".
- **Locked & inaccessible files** [MVP]: files that are open in another app, read-only or blocked by permissions are skipped and reported clearly ("12 files skipped, 3 open in Word, 9 no permission"), with *Retry* and *Show files*. Nothing fails silently.
- **Trash reminder** [MVP]: after a cleanup, show "3.2 GB is now in your Trash. Space is freed once the Trash is emptied", with a button to open the OS Trash. The app never empties the Trash itself without explicit confirmation.
- **History screen** [MVP]: a browsable timeline of the action journal ("Tuesday 14:02 · moved 38 files to Archive/2025"), searchable and filterable, with undo per action or per batch. The journal survives app restarts.

---

## 4. Preview

| Type | Preview | Priority |
|---|---|---|
| Images (JPG, PNG, GIF, WebP, SVG; HEIC where the OS supports it: always on macOS, on Windows with the HEIF extension, on Linux with libheif) | Viewer with zoom and EXIF panel | MVP |
| PDF | Page viewer with page thumbnails | MVP |
| Text / Markdown / code / JSON / CSV | Syntax-highlighted, rendered Markdown, CSV as table | MVP |
| Video / audio | Inline player with scrubbing | MVP |
| Archives (zip, tar, 7z) | Contents listing without extracting | v1 |
| Office docs (docx, xlsx, pptx) | OS-native thumbnail/Quick Look where available, text extraction fallback | v1 |
| RAW photos, fonts, ebooks | Embedded thumbnail / glyph sheet / cover | Later |

- **Spacebar quick preview** (Quick Look style) from any list. [MVP]
- **Metadata side panel**: size, dates, path, origin URL, dimensions, duration, page count. [MVP]
- **Thumbnail grid view** with a size slider. [MVP]
- "Open with default app" and "Reveal in Finder / Explorer / file manager". [MVP]

---

## 5. Rename

### Single rename [MVP]
- Inline rename (`F2` / `Enter`), with the extension protected by default.
- Live validation for illegal characters per OS and name collisions.

### Batch rename [MVP]
A rule pipeline with a **live before → after preview table** and conflict highlighting:
- Find & replace (plain text or regex)
- Add prefix / suffix
- Sequence numbering (`{n:03}`), with start value and step
- Change case (lower, UPPER, Title, kebab-case, snake_case)
- Insert date tokens: `{created}`, `{modified}`, `{exif.date}`, with a format of your choice
- Remove characters, trim spaces, normalise accents and unicode
- Change extension (with a warning)
- Save rule sets as **rename presets**, e.g. "Photos → `2026-10-20_Holiday_001.jpg`"

### Smart rename [v1 / Later]
- Suggest a name from content: PDF title/metadata, EXIF date + location, the first heading of a document.
- Clean up ugly download names such as `IMG_4821 (3).JPG` and `document-final-v2-FINAL(1).pdf`.
- AI suggestions (optional, opt-in). [Later]

---

## 6. Organise (move & structure)

| Feature | Priority |
|---|---|
| Drag & drop and cut/paste moves inside the app, with undo. MVP covers moves **within the same drive**; moves to another drive come in v1 with a verified copy-then-trash flow | MVP |
| **One-click tidy presets**: "Sort Desktop by type", "Archive Downloads by year/month", "Photos by date taken" | MVP |
| Preview of the resulting folder tree before applying | MVP |
| **Rules engine** (Hazel-style): *IF* location + conditions (type, age, name, size, origin) *THEN* actions (move, rename, tag, trash, archive). See "Rules & automation" below | v1 |
| **Recommended rules library**, all **off by default** | v1 |
| **Tags & colour labels**, synced with macOS Finder tags where possible and stored in the app index elsewhere | v1 |
| Favourites / quick-access locations | v1 |
| Compress-to-archive for old project folders ("archive instead of delete") | v1 |
| **Extract archives** (zip, tar, 7z) into a folder, with the option to trash the archive afterwards | v1 |
| **Right-click menu in Finder / Explorer / Linux file managers**: "Rename with Kipple…", "Clean up with Kipple…", "Tidy with Kipple…" | v1 |

### Rules & automation [v1]

**Decision:** rules may run automatically, but **only rules the user created or explicitly enabled**. Out of the box, nothing runs on its own. The MVP tidy presets are manual, one-click actions.

**Recommended rules library** (each one disabled until the user turns it on):
- Trash unfinished downloads (`.crdownload`, `.part`) older than 7 days
- Move installers in Downloads older than 30 days to the Trash
- Move screenshots from the Desktop to `Pictures/Screenshots/{year}-{month}`
- Sort new files in Downloads into subfolders by type
- Archive Desktop files untouched for 90 days into `Desktop/Archive/{year}`
- Remove empty folders in Downloads

**Enabling a rule**
1. The user sees a **dry-run preview** of what the rule would do *right now* ("would move 38 files, 1.2 GB").
2. The user picks a trigger: **manual**, **on a schedule**, or **on new files** (watch mode).
3. The user picks a mode: **Run automatically**, or **Ask me first** (matches go to a review list).

**Safety for automatic runs**
- Automatic rules use the same safety net as everything else (§3): Trash only (never permanent delete), journaled, undoable, and they respect protected paths, pinned files and locked files.
- **Bulk limit:** if a run would touch more than a set number of files (default 100) or a set size, it pauses and asks first.
- **Summary notification** after each automatic run ("Screenshots tidy moved 12 files"). Clicking it opens the History screen, where the run can be undone in one click. Desktop notifications can't carry action buttons on all OSes.
- Imported or synced rules always arrive **disabled**, even if they were enabled on the other computer.
- **Pause all automation**: a global switch in settings and in the tray.
- **Ordering & conflicts:** rules run in a user-defined order. The app warns about loops (rule A moves files into a folder that rule B watches) and about two rules matching the same file.
- Every automatic run appears in the History screen with the name of the rule that did it.

---

## 7. Browse, search & insights

| Feature | Priority |
|---|---|
| File browser with list, grid and column views, plus sorting and filtering | MVP |
| Instant search by name, with filters for type, size, date range, category and tag | MVP |
| **Storage visualisation**: treemap or sunburst of space per folder and type | v1 |
| **Smart folders / saved searches**, e.g. "PDFs from last month" | v1 |
| Desktop clutter score + weekly trend ("Downloads grew by 2.1 GB this week") | v1 |
| **"Space saved" counter**: "You've reclaimed 14 GB and tidied 3,210 files since you started" | v1 |
| **Show hidden files** toggle (dotfiles, hidden attribute), off by default | v1 |
| Full-text content search inside documents | Later |
| OCR for screenshots and scanned PDFs, making them searchable | Later |

---

## 8. Background & notifications

| Feature | Priority |
|---|---|
| System tray / menu bar icon with quick stats and a "Tidy now" button | v1 |
| Scheduled scans (daily or weekly) | v1 |
| Gentle notifications ("12 old installers in Downloads, 4.3 GB. Review?") | v1 |
| Launch at login (opt-in) | v1 |

---

## 9. Settings, accessibility & platform

| Feature | Priority |
|---|---|
| Light / dark / system theme | MVP |
| **Languages**: English and Dutch at launch, ready for more (see "Translation readiness" below) | MVP |
| Full keyboard navigation and a shortcut cheat-sheet | MVP |
| Configurable thresholds (what counts as "stale", screenshot age, etc.) | MVP |
| Onboarding wizard: pick folders, run the first scan, see quick wins | MVP |
| Guided OS permission requests (macOS Full Disk Access / folder prompts) | MVP |
| Auto-update (see §11; no own server) | v1 |
| Export / import settings, rules and rename presets as a file, to share them between computers | v1 |
| Screen-reader labels and high-contrast support | v1 |
| **Export diagnostics**: local rotating log files and a button that bundles logs, app version, OS info and settings into one `.zip` to attach to a GitHub issue or email. File names and paths are **redacted by default** (opt-in to include them). This replaces crash reporting, since there is no server | MVP |
| **Built-in help**: offline help pages, tooltips and a link to report an issue | v1 |
| **"What's new" screen** shown once after each update, generated from the release changelog | v1 |

### Translation readiness [MVP]

**Decision:** English (`en`) and Dutch (`nl`) at launch. The app is built so that adding a language means adding one locale file, with no code changes.
- **No hardcoded text:** every user-visible string lives in locale files. This includes UI, notifications, error messages, right-click menu labels, recommended rule names, help pages, "What's new" and installer texts.
- **No string concatenation.** Use full messages with placeholders and ICU MessageFormat for plurals ("1 file" / "12 files", "1 bestand" / "12 bestanden").
- **Locale-aware formatting** for dates, numbers and file sizes (`1.5 GB` in English vs `1,5 GB` in Dutch).
- **Language choice:** follows the OS language by default, can be switched in settings without a restart, and falls back to English for missing keys.
- **Layouts tolerate longer text.** Dutch strings are often longer than English ones, so no fixed-width buttons or truncated labels.
- **Quality checks in CI:** missing or unused keys fail the build, and a **pseudo-localisation** mode (e.g. `[Ŧíďý Ðéšķţöþ!!!]`) makes untranslated or truncated text visible during testing.
- [Later] Community translations through a free open-source plan on Weblate or Crowdin, and right-to-left layout support.

---

## 10. Optional AI layer [Later]

Only if you want it, and opt-in per feature:
- Content-based categorisation (invoice, contract, receipt, and so on)
- Smart rename suggestions
- Natural-language rules: "move all invoices older than a year to Archive/Invoices/{year}"
- Natural-language search: "the PDF about my car insurance from last spring"

Privacy options: a local model (offline) or a cloud API, with a clear indicator of what data is sent.

---

## 11. Distribution & installation

**Decisions:**
- The app is distributed to other computers, and **there is no central hosting**: no backend, no accounts, no telemetry server. Everything runs locally, and the app works fully offline.
- The app is **free**.
- There is **no budget for code signing**, so releases are unsigned (see "Unsigned releases" below).

### Installers per OS [MVP]
| OS | Formats | Notes |
|---|---|---|
| macOS | `.dmg` (universal: Apple Silicon + Intel) | Signed with a free **self-signed certificate** (a stable identity, so macOS keeps folder permissions across updates; to be confirmed in spike S2) but **not notarised**, so Gatekeeper asks the user to approve it once |
| Windows | NSIS `.exe` setup **per user** (no admin) as the main installer, an optional `.msi` for admins, plus a **portable `.exe`** (no install, no auto-update) | Unsigned, so SmartScreen warns once ("Windows protected your PC") |
| Linux | **AppImage** (portable, runs on most distros), plus `.deb` and `.rpm` | No signing required; optional GPG signature and checksum |

- Every release publishes **SHA-256 checksums**, so people can verify the files they receive.
- Installs **per user**, without admin rights where the OS allows it.
- **Clean uninstall:** the app's local index and settings live in the standard per-user app-data folder and are removed or offered for removal on uninstall. Formats without an uninstaller (`.dmg`, AppImage, portable) use **Settings → Remove all Kipple data**. [MVP]

### Where the files live (free, nothing to run yourself)
- **GitHub Releases** (or any static file location) holds the installers. Sharing the app means sending a link or the file itself, for example on a USB stick or a network share.
- [Later] Free package managers:
  - **Flathub** (Linux): no signing needed.
  - **winget** (Windows): accepts unsigned installers after Microsoft's automated malware scan.
  - **Homebrew** (macOS): our own *tap* (just a GitHub repo). The official cask repository is phasing out apps that don't pass Gatekeeper, so it isn't an option without notarisation.

### Auto-update without a server [v1]
- The app checks a static `latest.json` attached to the newest GitHub Release, downloads the update and installs it on restart.
- Updates are **signed with our own update key**. This is separate from OS code signing and **costs nothing**, so the app only installs updates that come from us, even though the installers themselves are unsigned.
- The update check can be disabled in settings and fails silently when offline.

### Unsigned releases (no signing budget) [MVP]
The app works fine unsigned. Users just have to approve it **once** on first launch, so the job is to make that step painless and trustworthy.

**First-launch experience per OS** (documented in an install guide with screenshots, in EN and NL, linked from the download page and README):

| OS | What the user sees | What they do (once) |
|---|---|---|
| macOS | "Apple could not verify '…' is free of malware" | Open **System Settings → Privacy & Security** and click **Open Anyway**. If macOS reports the app as "damaged", run one documented terminal command to remove the quarantine flag |
| Windows | SmartScreen: "Windows protected your PC" | Click **More info → Run anyway** |
| Linux | Nothing (AppImage: mark as executable) | Nothing extra |

**Building trust without paid certificates:**
- **SHA-256 checksums** for every file, published on the release page.
- **Public CI builds**: installers are built by GitHub Actions from the public source, never on a personal machine.
- **GitHub build provenance attestations** (free for public repos), so anyone can verify that a file was built from a specific commit.
- **VirusTotal scan link** for each Windows installer on the release page, to reassure users when SmartScreen warns.

**Free signing options to revisit later:**
- **Windows:** the SignPath Foundation offers free code signing for open-source projects, and the Microsoft Store (free individual developer accounts) signs packages itself. Either would remove the SmartScreen warning. Both need investigation, and SignPath requires the project to be open source.
- **macOS:** there is no free route to notarisation. It would only change with the €99/year Apple Developer membership.

### Build pipeline [MVP]
- **GitHub Actions** builds macOS (self-signed), Windows and Linux installers from one tag push, generates checksums and provenance attestations, signs the update bundles with the update key, and attaches everything to a draft release.
- The update signing key is stored only as a GitHub Actions secret, with an offline backup. **Losing it means existing installs can no longer auto-update.**
- Version number and changelog come from the git tag.

### Sharing between computers (still no server) [v1]
- Export / import rules, rename presets and settings as a single `.json` file.
- Optional: point the app's config file at a folder you already sync yourself (Dropbox, OneDrive, Syncthing), so presets follow you across machines.

---

## 12. Suggested release slicing

**MVP (v0.1): "See it, clean it, rename it"**
Folder selection → scan & index → type and age categories → cleanup detectors (duplicates, installers, temp files, empty items, large & old, screenshots, extracted archives) → review UI with preview → trash with undo → single and batch rename → tidy presets → file browser with search.
Trust & robustness: recovery of interrupted bulk actions, name-conflict handling, locked/inaccessible file reporting, Trash reminder, history screen, export diagnostics, privacy statement, non-goals respected, performance targets met.
Platform: English + Dutch with full translation readiness, user folders + custom locations, unsigned installers for all three OSes with an install guide, checksums and provenance.

**v1.0: "Keep it clean automatically"**
Rules engine + watch mode (opt-in only), recommended rules library (off by default), similar-image detection, versions check, quarantine, storage treemap, tray app, schedules and notifications, tags, quick triage mode, archive previews, extract archives, right-click menu integration, network drives / NAS, hidden-files toggle, "space saved" counter, smart folders, built-in help, "What's new" screen, auto-update.

**Later: "Understand my files"**
AI categorisation and rename, OCR, full-text search, natural-language rules and search.

---

## 13. Decisions & open questions

### Decided
| Topic | Decision | See |
|---|---|---|
| Audience | Distributed to other computers, no central hosting | §11 |
| Price | Free | §0, §11 |
| Code signing | No budget, so unsigned releases with a guided first launch | §11 |
| Scope | User folders by default, plus any folder or drive the user adds | §1 |
| Automation | Rules may run automatically, but only when created or explicitly enabled by the user. Recommended rules are off by default | §6 |
| Languages | English + Dutch at launch, translation-ready | §9 |
| Name | **Kipple** (see "Brand" below) | — |
| Licence | **GPL-3.0-or-later**, open source in a public GitHub repo. Every fork and redistribution must stay open source under the same licence. This enables free Windows signing via SignPath (§11), free Weblate/Crowdin plans (§9), and free CI minutes and attestations | §11 |

### Licence consequences (GPL-3.0)
- **Dependencies must be GPL-3.0-compatible.** MIT, BSD, Apache-2.0, MPL-2.0, LGPL and GPL-3.0 are fine. GPL-2.0-*only*, proprietary and "non-commercial" licences are not. A licence check in CI (`cargo-deny` for Rust, a license checker for npm) fails the build on an incompatible dependency.
- **Contributions** use the **DCO** (a `Signed-off-by` line in each commit) instead of a CLA. It's lightweight and standard for GPL projects.
- **Source files** carry an SPDX header: `// SPDX-License-Identifier: GPL-3.0-or-later`. The same identifier goes in `Cargo.toml` (`license = "GPL-3.0-or-later"`) and `package.json`. "Or later" lets the project move to a future GPL version without asking every contributor.
- **Releases** link to the exact source tag, which satisfies the GPL's source-availability requirement automatically through GitHub.
- **Distribution channels:** GitHub Releases, Flathub, winget, Homebrew taps and the Microsoft Store are all fine with GPL. The Apple Mac App Store is not (its terms conflict with GPL), but it isn't planned anyway.

### Still open
1. **AI**: wanted at all? If so, local-only or cloud allowed? (Only affects "Later", so it doesn't block the MVP.)

### Brand
- **Name:** **Kipple**. In Philip K. Dick's *Do Androids Dream of Electric Sheep?* (the basis for *Blade Runner*), kipple is the useless junk that piles up by itself: *"Kipple drives out nonkipple."* Kipple is the app that fights back.
- **Mascot:** a little chicken (*kip* is Dutch for chicken) that pecks away the clutter. Used for the app icon, onboarding, empty states and the "space saved" moments.
- **Tagline:** *"Fight the kipple."* (EN) / *"Weg met de kipple."* (NL)
- **Tone:** playful and warm in the copy, but the safety texts (deleting, undo, warnings) stay calm, clear and serious.
- **Identifiers:** app/bundle ID e.g. `app.kipple.desktop`, used consistently for the installers, the right-click menu, the app-data folder and the update key.

**Name checks still to do before the first public release:**
- [ ] GitHub organisation / repo name (e.g. `kipple-app/kipple`)
- [ ] Domain (e.g. `kipple.app` or `getkipple.app`), only needed for a landing page, which can be hosted free on GitHub Pages
- [ ] Trademark search in the Benelux (BOIP), the EU (EUIPO) and the US (USPTO) for the software classes. A word from a novel isn't protected by copyright, but someone could have registered it as a trademark
- [ ] Package names on Flathub, winget and Homebrew

---

## 14. Tech direction

> The full architecture is in [ARCHITECTURE.md](ARCHITECTURE.md), with decisions in [adr/](adr/) and domain terms in [CONTEXT.md](CONTEXT.md). Summary:

- **Tauri 2** (Rust core + web UI in React/TypeScript): small binaries (~10–20 MB versus ~100 MB+ for Electron), fast native scanning and hashing, a single codebase for all three OSes. It ships with installer bundling for every format in §11, a signed updater that works with a static `latest.json`, and an official GitHub Action for multi-OS builds. That fits "distribute without hosting" well. Main alternative: Electron (larger footprint, more mature preview tooling).
- Local index in **SQLite**.
- Core logic (scanner, detectors, rename engine, rules engine, action journal) as a pure, OS-agnostic module behind thin per-OS adapters (trash, last-used signals, permissions, thumbnails). This keeps it testable with TDD.
