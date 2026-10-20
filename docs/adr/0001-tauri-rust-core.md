# ADR-0001: Tauri 2 shell with a Rust core and a React frontend

- Status: Accepted
- Date: 2026-10-20

## Context
Kipple must run on Linux, macOS and Windows from one codebase. It is distributed unsigned via GitHub Releases, without a server. It must also scan and hash large trees fast (target: 1M files), and offer rich previews (images, PDF, media, code).

## Options
1. **Tauri 2 (Rust + system webview)**: small binaries, native performance for scanning and hashing, installer bundling and a signed updater built in. The catch is that each OS uses a different webview (WKWebView, WebView2, WebKitGTK).
2. **Electron (Node + bundled Chromium)**: the same rendering everywhere and mature tooling, but ~100 MB+ binaries, higher memory use, and scanning in Node, which is slower.
3. **Native per OS (Swift / WinUI / GTK)**: the best OS integration, but three codebases. That doesn't fit a free open-source project.
4. **Flutter / Qt**: one codebase, but weaker web-based preview tooling (pdf.js, CodeMirror), and Qt's licensing adds complexity.

## Decision
Option 1: **Tauri 2** with a Rust Cargo workspace for all logic and React + TypeScript for the UI.

## Consequences
- Fast native scanning, hashing and file operations. It's easy to hit the idle-memory target of under 200 MB.
- Installers (dmg/msi/nsis/deb/rpm/AppImage), the updater, tray, notifications and autostart come from official Tauri plugins and `tauri-action`.
- Webview differences need testing. In particular, media codecs on WebKitGTK (spike S5) and WebDriver e2e tests aren't available on macOS.
- Contributors need both Rust and TypeScript.
