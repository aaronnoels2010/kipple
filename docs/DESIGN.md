# Kipple: Design System

Status: **v0.1, draft**. The visual mockup is [design.html](design.html). Open it in a browser and use the EN/NL and theme switches at the top.
Related: [FUNCTIONAL_PLAN.md](FUNCTIONAL_PLAN.md) (what), [ARCHITECTURE.md](ARCHITECTURE.md) (how), [CONTEXT.md](CONTEXT.md) (terms).

> **Review helpers in `design.html`:** `?theme=light|dark|system`, `?lang=en|nl`, `?screen=overview|cleanup|rename`, `?only=foundations|components|patterns|screens`.
> Example: `design.html?theme=dark&lang=nl&screen=cleanup&only=screens`.

---

## 1. Design direction

**"Eggshell & yolk by day, night coop by night."** Warm, calm and a little cheeky. Kipple deals with people's files, so the base is quiet and trustworthy. The personality (the hen, the warm serif, the occasional joke) appears only where nothing is at stake.

| Principle | Means in practice |
|---|---|
| **Calm when it matters** | Deleting, permanent delete, recovery and errors use plain, serious language. No jokes, no mascot, no exclamation marks |
| **Cheeky when it doesn't** | Empty states, onboarding, success and space-saved moments may be playful ("Not a speck of kipple.") |
| **Show the reason** | Every suggestion shows *why* (reason text + confidence eggs) next to the item, never hidden behind a click |
| **Data speaks mono** | Paths, sizes, dates, counts and tokens are set in Martian Mono, so the user sees at a glance what is data and what is UI |
| **Everything undoable, and visibly so** | After every change: a toast with **Undo** and a History entry. Destructive buttons always state the count |
| **One accent** | Yolk is the only brand accent. Comb (red) is reserved for destructive actions |

---

## 2. Brand

- **Name:** Kipple. **Wordmark:** "kipple" in Fraunces 700 (SOFT 100, WONK 1), lowercase, with the **dot of the i replaced by a yolk-coloured egg**. In HTML it is built from a dotless `ı` plus a pseudo-element; in the app it is an SVG asset.
- **Mascot "Kip" (the hen):** geometric, thick outline, cream body, red comb, orange beak and legs. States:
  | State | Used for |
  |---|---|
  | Idle | Onboarding, About |
  | Happy (closed smiling eye + sparkles) | Space saved, "all clean" empty states |
  | Sleeping (zz) | Automation paused, tray idle |
  | Alert (wide eye + "!") | Recovery available, drive offline (in banners only, never in delete dialogs) |
  | Nesting (with eggs) | Scanning, empty lists |
- **Never** use the mascot in: delete or permanent-delete confirmations, error reports, or permission prompts.
- **App icon:** the hen's head on a yolk egg shape. **Tray icon:** a monochrome hen silhouette (macOS "template" image, 16/32 px, light and dark variants for Windows and Linux).
- **Taglines:** *"Fight the kipple."* / *"Weg met de kipple."*

### Voice & tone

| Situation | ✅ Do | ❌ Don't |
|---|---|---|
| Suggestion | "Installer, 214 days old. Zoom appears installed." | "This file is junk!" |
| Destructive button | "Move 142 files to Trash" | "OK", "Yes", "Clean!" |
| Permanent delete | "Files deleted here are gone for good and can't be undone." | "Are you sure? 😬" |
| Success | "Moved 142 files to Trash · Undo" | "Boom! Kipple destroyed!" |
| Empty state | "Not a speck of kipple." / "Geen spatje kipple." | |
| Error | "12 files were skipped. 3 are open in Word, 9 need permission." | "Something went wrong." |

- **Use the OS name for the trash:** *Trash* (macOS, Linux), *Recycle Bin* (Windows). In Dutch: *Prullenmand* (macOS) and *Prullenbak* (Windows, Linux). This comes from a platform-aware message key, `trash.name`.
- **Dutch** uses informal *je/jij*, which is common for consumer apps in Flanders and the Netherlands.

---

## 3. Theme & colour

Two themes: **Eggshell** (light) and **Night coop** (dark). By default Kipple **follows the OS**, and the user can override it in Settings (Light / Dark / System). All colours are **semantic tokens**. Components never use raw hex values.

### 3.1 Tokens

| Token | Role | Eggshell (light) | Night coop (dark) |
|---|---|---|---|
| `--bg` | Window background | `#FBF7EE` | `#14120E` |
| `--surface` | Cards, lists, dialogs | `#FFFFFF` | `#1C1914` |
| `--surface-2` | Inset areas, hover, inputs | `#F3EDE0` | `#25211A` |
| `--surface-3` | Tracks, skeletons | `#EAE2D2` | `#2E2920` |
| `--sidebar` | Sidebar, title bar | `#F5F0E4` | `#181611` |
| `--border` | Hairlines | `#E3DACA` | `#332D24` |
| `--border-strong` | Input outlines, checkboxes (≥ 3:1) | `#8F8574` | `#7A705F` |
| `--text` | Primary text | `#211C15` | `#F2ECDF` |
| `--text-muted` | Secondary text | `#6A6052` | `#A69C8B` |
| `--text-faint` | Placeholders, decorative only | `#9A907F` | `#756C5E` |
| `--yolk` | **Brand accent**: primary buttons, selection, meter | `#F6B91A` | `#FFC53D` |
| `--yolk-hover` | Primary hover | `#E8A90A` | `#FFD166` |
| `--yolk-soft` | Selected rows, highlights | `#FDF0C8` | `#3A2F12` |
| `--on-yolk` | Text on yolk | `#211C15` | `#1A150C` |
| `--yolk-ink` | Accent as text/icon | `#8A5800` | `#FFC53D` |
| `--comb` | **Destructive only** | `#C2372B` | `#FF7A6B` |
| `--comb-soft` | Destructive tint, conflicts | `#FBE3DF` | `#3D1D19` |
| `--on-comb` | Text on comb | `#FFFFFF` | `#14120E` |
| `--sage` / `--sage-soft` | Keep, success, space saved | `#2C7A4B` / `#DFF0E5` | `#62C48E` / `#16301F` |
| `--amber` / `--amber-soft` | Warning, stale, offline | `#955700` / `#FCEBD0` | `#F2A93B` / `#36270F` |
| `--sky` / `--sky-soft` | Info, links, online-only | `#2F62C9` / `#E1EAFB` | `#7FAEFF` / `#17243D` |
| `--focus` | Focus ring | `#2F62C9` | `#8DB6FF` |
| `--inverse-accent` / `--inverse-ok` | Accent and success on inverted surfaces (toast, action bar) | `#F6B91A` / `#62C48E` | `#8A5800` / `#2C7A4B` |

**File type colours** (chips and icon tiles only, always with an icon and a label):

| Type | Light | Dark |
|---|---|---|
| Images | `#B04A8C` | `#E58BC4` |
| Screenshots | `#5B6676` | `#A9B4C2` |
| Videos | `#7A4FC0` | `#B596F0` |
| Audio | `#2A7F80` | `#6CC9C9` |
| Documents | `#2F62C9` | `#7FAEFF` |
| Archives | `#86662A` | `#D6B36A` |
| Installers | `#B5561E` | `#F09A5E` |
| Code | `#4D6A2C` | `#A6C77A` |

### 3.2 Usage rules
1. **Yolk = primary action, at most one per view**, plus selection and the brand.
2. **Comb = destructive only** (Trash, delete, conflicts that would destroy data). Never decorative.
3. **Confidence is shown with eggs** (●●● High, ●●○ Medium, ●○○ Low) and a label, never with red/green.
4. **Status never relies on colour alone.** Every badge has a dot or icon plus text.
5. Tints are generated with `color-mix(in srgb, var(--c) 13%, transparent)`, so one token drives both themes.

### 3.3 Contrast (WCAG 2.2, verified)

Values are measured on `--bg` / `--surface-2`.

| Pair | Light | Ratio | Dark | Ratio |
|---|---|---|---|---|
| Text | `#211C15` | 15.8 / 14.5 | `#F2ECDF` | 15.9 / 13.6 |
| Text muted | `#6A6052` | 5.8 / 5.3 | `#A69C8B` | 6.9 / 5.9 |
| Yolk ink | `#8A5800` | 5.6 / 5.2 | `#FFC53D` | 11.9 / 10.1 |
| Comb | `#C2372B` | 5.1 / 4.6 | `#FF7A6B` | 7.3 / 6.3 |
| Sage | `#2C7A4B` | 4.9 / 4.5 | `#62C48E` | 8.7 / 7.5 |
| Amber | `#955700` | 5.4 / 4.9 | `#F2A93B` | 9.4 / 8.0 |
| Sky | `#2F62C9` | 5.3 / 4.9 | `#7FAEFF` | 8.4 / 7.2 |
| Text on yolk | | 9.6 | | 11.5 |
| Text on comb | white | 5.4 | ink | 7.3 |
| Focus ring vs bg | | 5.3 | | 9.1 |
| Border-strong vs surface (non-text, ≥ 3:1) | | 3.6 | | 3.6 |

All text pairs pass **AA (≥ 4.5:1)**. `--text-faint` is for placeholders and decoration only. A **high-contrast theme** is planned for v1.

---

## 4. Typography

| Role | Font | Size / line | Weight & axes |
|---|---|---|---|
| Display (onboarding, hero) | **Fraunces** | 48/1.0 | 650 · SOFT 100 · WONK 1 · opsz 144 |
| Page title | Fraunces | 26–28/1.15 | 600 · SOFT 100 |
| Dialog title | Fraunces | 21/1.2 | 600 |
| Big number (space saved, meter) | Fraunces | 34–40 | 700 · tabular numbers |
| Heading | **Instrument Sans** | 16/1.4 | 650 |
| Body (desktop density) | Instrument Sans | 14/1.5 | 400 |
| Small / meta | Instrument Sans | 12.5/1.45 | 400 |
| Data: paths, sizes, dates, tokens, kbd | **Martian Mono** | 11–12.5 | 400–500 · width 87.5% |

- All three fonts are **OFL-licensed and bundled** with the app (`app/src/assets/fonts/`). They are never loaded from the web, in line with the privacy promise. `design.html` uses Google Fonts only as a mockup convenience.
- **Fallbacks:** `Georgia` (display), `system-ui` (UI), `ui-monospace` (data).
- Numbers in tables use `font-variant-numeric: tabular-nums`.
- **Never truncate** critical text (counts, destructive labels). File names may truncate in the middle (`IMG_48…(3).JPG`), with the full name in a tooltip and the preview pane.

---

## 5. Shape, space, elevation, motion

| Token | Value |
|---|---|
| Radius | `xs 4` · `sm 6` · `md 10` (buttons, inputs) · `lg 16` (cards) · `xl 22` (dialogs) · `pill` · **egg** (`50% 50% 46% 46% / 58% 58% 42% 42%`, brand moments only) |
| Spacing | 4 px base: 4, 8, 12, 16, 20, 24, 32, 40, 48, 64 |
| Elevation | `shadow-1` rows/cards · `shadow-2` hover/popovers · `shadow-3` dialogs/toasts/action bar. Shadows are warm-tinted in light mode and deep black in dark mode |
| Control heights | 28 (small) · 34 (default) · 42 (large). Minimum hit target is 28 × 28 |
| Motion | `dur-1 120ms` (hover/press) · `dur-2 200ms` (toggles, selection) · `dur-3 360ms` (panels). Easing is `ease-out (.2,.8,.2,1)` or `spring (.34,1.56,.64,1)` for checkmarks and switches |

**Motion rules:**
- Motion confirms state changes (a check pops, a row slides out to the Trash, the meter fills). It is never decoration in work views.
- One staggered reveal is allowed on first launch and onboarding.
- `prefers-reduced-motion` disables all animation, and the meter jumps to its value.
- A subtle **paper grain** (SVG noise, 4–6 % opacity) sits on the window background for warmth. It is turned off in the high-contrast theme.

---

## 6. Iconography

**Lucide** (ISC licence, GPL-compatible) at 1.9 px stroke: 16 px in UI, 20 px in dialogs, 22 px in feature tiles. An icon never stands alone: it always has a visible label or a tooltip plus an `aria-label`.

| Concept | Icon |
|---|---|
| Overview | `layout-dashboard` |
| Browse | `folder-open` |
| Cleanup | `sparkles` |
| Rename | `pencil-line` |
| Tidy | `folder-tree` |
| Rules | `workflow` |
| History | `history` |
| Settings | `settings` |
| Trash | `trash-2` |
| Undo | `undo-2` |
| Keep / pin | `pin` |
| Duplicate | `copy` |
| Online-only | `cloud` |
| Drive | `hard-drive` |
| Offline | `wifi-off` |
| Locked | `lock` |
| Installer | `package` |
| Screenshot | `monitor` |
| Unfinished download | `download` |
| Archive | `archive` |
| Empty | `folder-x` |

---

## 7. Layout

- **App shell:**
  - Native title bar per OS (traffic lights on macOS, caption buttons on Windows, the decorations of the user's Linux desktop).
  - **Sidebar** of 228 px (navigation, locations with online/offline dots, scan status, Settings).
  - **Top bar** of 56 px (search `⌘/Ctrl K`, Scan now, History).
  - **Content** with 24 px padding.
- **Minimum window size:** 960 × 600. Below 1100 px wide, the preview pane becomes an overlay (Space toggles it).
- **Cleanup layout:** detector **chips** across the top (horizontally scrollable), the candidate list in the middle, and a **preview pane** of 290 px on the right. An **action bar** floats at the bottom when items are selected.
- **Text expansion:** layouts must tolerate **+35 %** text length (Dutch compounds such as *"Oude installatiebestanden"*). Buttons grow; they never clip. This is checked in `design.html` with the NL switch.

---

## 8. Component inventory

Implemented with **Radix UI primitives** plus Tailwind tokens. Every interactive component has: default, hover, active, **focus-visible**, disabled and (where relevant) loading states.

| Component | Variants / states | Radix primitive | Used in | Phase |
|---|---|---|---|---|
| Button | primary (yolk), secondary, ghost, keep (sage-soft), danger-soft, danger · sm/md/lg · icon-only · loading · with `kbd` hint | — | everywhere | MVP |
| Text input | default, mono, invalid + error help, disabled | — | rename, settings, dialogs | MVP |
| Search field | pill, `⌘K` hint, clear button | — | top bar, lists | MVP |
| Select | default, compact | `Select` | settings, duplicate keep rule, presets | MVP |
| Checkbox | checked, indeterminate, disabled | `Checkbox` | lists, onboarding | MVP |
| Radio / choice card | radio, choice card with title + description | `RadioGroup` | conflict dialog, settings | MVP |
| Switch | on (sage), off | `Switch` | settings, rule cards, automation pause | MVP |
| Segmented control | 2–4 options, icon + label | `ToggleGroup` | theme, view mode, language | MVP |
| Slider | with value label | `Slider` | thresholds (stale age, screenshot age) | MVP |
| Badge / chip | type category, status (dot), online-only, offline, locked, solid | — | rows, preview pane | MVP |
| Confidence eggs | high / medium / low (+ label) | — | candidates, detector cards | MVP |
| Tooltip | default (inverted) | `Tooltip` | icons, truncated names, reasons | MVP |
| Banner | info, warn, danger, ok, **recovery** (dashed yolk) · with actions | — | overview, top of lists | MVP |
| Toast | success + **Undo**, error + Show files | `Toast` | after every apply | MVP |
| Progress | striped bar (scan), meter ring (overview) | `Progress` | scan, overview | MVP |
| Skeleton | line, row | — | loading lists | MVP |
| Dialog | dry-run confirm, conflict, **permanent-delete consent** (typed phrase), recovery | `Dialog` / `AlertDialog` | apply flows | MVP |
| Menu | context menu on rows, overflow menu | `ContextMenu`, `DropdownMenu` | lists | MVP |
| Tabs | underline / segmented | `Tabs` | settings, cleanup | MVP |
| Keyboard hint (`kbd`) | single, combo | — | buttons, cheat-sheet | MVP |
| List row | file row, candidate row (reason + confidence + size), disabled (placeholder), selected | — | browse, cleanup, review | MVP |
| Grid tile | thumbnail + name + size, selected | — | browse grid | MVP |
| Preview pane | hero (thumbnail / Quick Look), badges, reason box, metadata list, actions | — | cleanup, browse | MVP |
| Action bar | inverted, count + size, Keep / Trash / More | — | cleanup, rename, browse | MVP |
| Duplicate group | copies side by side, **Keep** marker, keep-rule select | — | duplicates | MVP |
| Rename step | numbered, draggable, inline input, remove | — | rename | MVP |
| Rename preview table | before → after, changed part highlighted, **conflict row** (comb-soft + wavy underline + badge) | — | rename | MVP |
| History item | icon by action, title, time + source, Undo | — | history, overview | MVP |
| Empty state | mascot + title + body (+ action) | — | all lists | MVP |
| Location item | name, count, online/offline dot, sync-root icon | — | sidebar, onboarding | MVP |
| Detector card / chip | icon, name, size, count, confidence | — | overview, cleanup | MVP |
| Rule card | on/off, trigger + mode summary, flow tokens, Preview (dry run) | — | rules | v1 |
| Triage card | big preview, K / D / A / → actions | — | quick triage | v1 |
| Storage treemap | per folder and type, hover details | — | insights | v1 |

---

## 9. Key patterns

| Pattern | Behaviour |
|---|---|
| **Review → Apply** | Selecting items shows the **action bar** with count + size. "Move N to Trash" opens the **dry-run dialog** (per-detector summary + total), and the confirm button repeats the count. After applying: a toast with **Undo**, then the **Trash reminder** banner |
| **Conflicts** | A dialog with both files side by side and choice cards: *Keep both* (default) / *Replace* (says "the existing file goes to the Trash") / *Skip*, plus "Do this for the N other conflicts" |
| **No Trash on this drive** | An AlertDialog in calm, plain words with the drive name. The user must type `delete N`; the confirm button stays disabled until then. The safe action ("Keep files") is the default focus |
| **Recovery** | A dashed yolk banner at the top of Overview: what happened ("interrupted while moving 38 files"), what's done and not done, "Nothing was lost", plus **Resume** (primary) and **Roll back** |
| **Skipped files** | A danger banner or toast with counts by reason, plus **Show files** and **Retry** |
| **Sync root notice** | An info banner the first time a location inside OneDrive/iCloud/Dropbox is managed |
| **Offline location** | A dimmed sidebar item + `wifi-off`, and a warn banner when it is opened. Items are kept |
| **Online-only file** | A disabled row with the "Online only" badge and the reason "Not downloaded, so not checked" |
| **Placeholder previews** | Show the type icon instead of a thumbnail. Never trigger a download |
| **Undo everywhere** | Every toast for a change has Undo, and every History item has Undo. `⌘/Ctrl Z` undoes the last batch |

---

## 10. Screens (MVP)

The mockups in `design.html` cover **Overview**, **Cleanup** and **Rename**. The other screens reuse the same components.

| Screen | Key content |
|---|---|
| **Onboarding** (4 steps) | Welcome (hen) → pick locations (user folders pre-checked, add a folder via native dialog, sync-root notice) → permissions guide (macOS Full Disk Access / folder prompts) → first scan with progress → quick wins |
| **Overview** | Greeting · **kipple meter** (reclaimable vs scanned) + "Review cleanup" · detector cards · space-saved card with sparkline · Trash reminder · recent History · recovery banner (when needed) |
| **Browse** | List / grid / columns · filters (type, size, date, status) · search · preview pane · context menu (open, reveal, rename, move, trash, pin) |
| **Cleanup** | Detector chips · candidate list with reasons and confidence · preview pane with metadata and "Never suggest" · action bar · duplicate resolver view for `duplicate` |
| **Rename** | Preset select · step pipeline · live before → after table with conflicts · action bar ("Rename 5, skip 1") |
| **Tidy** | Preset cards ("Sort Desktop by type"…) · **folder-tree preview** of the result · dry-run summary |
| **History** | Timeline grouped by day · filter by source (cleanup, rename, tidy, rule) · Undo per batch or entry · skipped details |
| **Settings** | General (language, theme, launch at login) · Locations · Thresholds · Updates · Privacy & diagnostics (export, redaction toggle) · **Remove all Kipple data** |
| **v1** | Rules (cards + editor + dry run), Review inbox, Quick triage, Storage treemap, Holding area |

---

## 11. Accessibility

- **WCAG 2.2 AA** for both themes (see §3.3). The high-contrast theme comes in v1.
- **Focus:** a 2 px `--focus` ring with 2 px offset on every interactive element. It is never removed.
- **Keyboard-first** (shown in the cheat-sheet, `?`):

| Keys | Action |
|---|---|
| `⌘/Ctrl K` | Search |
| `Space` | Quick look |
| `Enter` / `F2` | Rename |
| `⌘/Ctrl A` | Select all |
| `⌫ / Delete` | Move to Trash (opens the dry-run dialog) |
| `K` | Keep |
| `P` | Pin / never suggest |
| `T` | Quick triage |
| `⌘/Ctrl Z` | Undo last batch |
| `⌘/Ctrl 1…7` | Navigate sections |
| `Esc` | Close dialog / preview |

- **Screen readers:** lists are `listbox`/`grid` with `aria-selected`. Reasons and confidence are part of the accessible name ("Zoom_6.2.1.dmg, installer, high confidence, 128 MB"). Progress uses `aria-live="polite"`.
- **Colour is never the only signal:** status and confidence always include text or shape.
- **Reduced motion** is respected (§5), and the grain overlay is decorative (`pointer-events: none`).
- **Hit targets:** at least 28 × 28 px (desktop), with ≥ 8 px spacing between destructive and non-destructive buttons.

---

## 12. i18n in design

- All strings are keys in `locales/{en,nl}` (ICU), so **no text in images**.
- **Numbers and sizes** use `Intl`: `4.3 GB` / `4,3 GB`, `1,284` / `1.284`. Dates follow the locale: `14 Mar` / `14 mrt`.
- **Plurals** use ICU: `{count, plural, one {# file} other {# files}}` / `{count, plural, one {# bestand} other {# bestanden}}`.
- **Truncation:** never in buttons, titles or destructive labels. File names truncate in the middle.
- RTL layout is v1+. Use logical CSS properties (`margin-inline`, `padding-inline`) from day one.

---

## 13. Implementation mapping

- `app/src/styles/tokens.css` holds the tokens as CSS custom properties, exactly as in `design.html` (`:root`, `[data-theme="dark"]`, and `prefers-color-scheme` when the theme is set to System).
- **Tailwind v4:** `@theme` maps tokens to utilities (`bg-surface`, `text-muted`, `bg-yolk`, `text-comb`…), so no hex values appear in components.
- **Theme switching:** Rust passes the OS appearance; the user override lives in `config/settings.json`, and the frontend sets `data-theme` on `<html>`.
- **Fonts:** `@font-face` from bundled WOFF2 files (Fraunces variable, Instrument Sans variable, Martian Mono variable with width axis).
- **Mascot:** SVG components (`<Hen state="happy" />`) using the token colours `--hen`, `--hen-line`, `--beak`, `--comb` and `--wing`, so they adapt to the theme automatically.
- **Radix primitives:** listed per component in §8.

---

## 14. Open design questions

1. A **final mascot and app icon** by an illustrator (the current hen is a geometric placeholder that defines proportions and colours).
2. A **compact density** option for power users (row height 32 vs 44)?
3. **Sound** on "space saved" (a soft cluck)? Off by default if added.
4. The **high-contrast theme** palette (v1).
