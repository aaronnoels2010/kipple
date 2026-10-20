# Translations

All user-visible text lives here, in [ICU MessageFormat](https://formatjs.github.io/docs/core-concepts/icu-syntax): plurals, selects and placeholders, never strings glued together in code ([ADR-0004](../docs/adr/0004-i18n-codes-not-text.md)).

| File | Used by |
|---|---|
| `<lang>/app.json` | The UI (react-intl) |
| `<lang>/tray.json` | The Rust side (tray menu, OS notifications) via `crates/kipple-i18n-rs`, which compiles it in at build time. Keep these messages simple: placeholders and `plural` only |

`en` is the source language. Every other language must have exactly the same keys with the same placeholders; tests fail otherwise (`pnpm -C app test`, `cargo test -p kipple-i18n-rs`).

Dutch uses informal *je/jij*. To spot untranslated or clipped text, run the UI with the pseudo-locale: `VITE_KIPPLE_LOCALE=pseudo pnpm -C app tauri dev`.
