# ADR-0007: The Rust backend is the trust boundary; the webview is treated as untrusted

- Status: Accepted
- Date: 2026-10-20

## Context
Kipple previews untrusted files (random downloads: SVG, PDF, Markdown, media) inside the webview, and it can trash and move files. If a preview bug or injected script could talk to the backend with arbitrary input, a single exploit could add `/` as a location and trash everything. A broad asset-protocol scope would also bypass the Rust-side checks for exclusions and protected paths. It could even trigger cloud placeholder downloads, for example through `<img src>`.

## Decision
- **Plans are built, stored and validated in Rust.** The UI sends item IDs, step definitions and conflict resolutions, then applies by `plan_id`. Plans are re-validated at apply time.
- **Locations and custom destinations are only added through native dialogs that Rust invokes** (`tauri-plugin-dialog` called from Rust). These return tokens. The frontend can never provide a raw path to act on.
- **No `plugin-fs` and no broad asset-protocol scope.** Previews are served by a custom `kipple-preview://<token>` scheme. Each token is short-lived and issued for one item. The handler refuses placeholders, protected and excluded paths, and offline locations, sends an explicit MIME type with `nosniff`, and supports Range requests.
- **Navigation guard:** the webview only ever shows the app's own origin. External links are cancelled and offered as "open in browser".
- **Strict CSP**, no remote origins and no inline scripts. SVG is shown only via `<img>`, HTML as text, Markdown without raw HTML, and pdf.js is pinned and runs without eval.

## Consequences
- A compromised webview can at most ask for plans over items the user already manages, and applying them still requires the normal confirmation flows.
- A little more backend code (plan store, tokens, scheme handler) in exchange for one clear security model.
- Easier to reason about in reviews: "any path the backend acts on came from the index or a native dialog".
