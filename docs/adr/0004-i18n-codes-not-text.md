# ADR-0004: The backend emits codes and parameters; one catalog renders all text

- Status: Accepted (revised after architecture review)
- Date: 2026-10-20

## Context
Kipple launches in English and Dutch and must be ready for more languages. User-visible text comes from many places: candidate reasons, validation issues, error reports, batch summaries, notifications, the tray menu, the right-click menu and the installer.

The first draft had a hidden webview format the tray and notification text. The review showed problems with that:
- A hidden webview can cost 100–250 MB on its own, which threatens the < 200 MB idle target.
- Hidden webviews have their timers throttled.
- Tray labels are needed before the webview has loaded and after a renderer crash.

## Decision
- The Rust backend returns stable **codes + typed parameters** for everything shown inside the app window, e.g. `{ code: "installer_old", params: { days: 214, appInstalled: true } }`. The frontend renders them with **ICU MessageFormat** (`react-intl`) from `locales/{en,nl}/*.json`.
- **Tray menu and OS notification strings** (a small set of about 30 keys, tagged `"rust": true` in the catalog) are compiled at build time into a **Rust mini-catalog** (`kipple-i18n-rs`), with plurals and numbers via ICU4X. The source is the same `locales/` files.
- **While Kipple runs in the tray, the webview is destroyed**, and it is re-created when the window opens. The app keeps running by preventing exit when the last window closes.
- **Strings outside the app runtime** (installer, right-click menu, desktop entries) are generated at build time from the same catalogs.
- The OS locale is read in Rust (`sys-locale`) and passed to the frontend. Users can override it in settings.

## Consequences
- One translation source and one message format, whether the text is rendered by TypeScript or Rust. CI checks that both are in sync.
- The idle tray process holds no webview (to be confirmed in spike S8).
- Codes are a stable, typed API between backend and frontend.
- Logs use codes as well, so exported diagnostics are language-independent.
