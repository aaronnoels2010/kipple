# Spike S8: idle memory with the webview destroyed vs hidden (K-1.11)

**Question.** ADR-0004 plans to *destroy* the webview while Kipple only lives in the tray, and re-create it when the window opens. Does that actually keep the tray-only process small (target: **< 200 MB idle**, FUNCTIONAL_PLAN §2), compared with merely hiding the window? And does the tray menu work from the Rust catalog, with no webview at all?

**Answer: yes, destroy it.** Hiding the window saves nothing on any OS. Destroying it cuts idle memory by 35–90 % and brings every OS under the target. Re-opening is fast (≈ 0.1 s on a Mac). The tray menu renders from `kipple-i18n-rs` in the OS language. ADR-0004 is confirmed.

## Setup

- `src-tauri/`: a standalone Tauri 2.12.1 app that loads the **real Kipple UI** (`app/dist`) with a tray icon. After the page has loaded it keeps (`open`), hides (`hide`) or destroys (`destroy`) the window, then idles in the tray.
- The tray menu ("Open Kipple" / "Quit Kipple", "Kipple openen" / "Kipple afsluiten") comes from `kipple-i18n-rs`. "Open" re-creates a destroyed window.
- `measure.sh` (macOS, Linux) and `measure.ps1` (Windows) sum the **whole process family**, because the webview runs in helper processes: WebView2 children (`msedgewebview2.exe`) on Windows, WebKit XPC services on macOS, `WebKitWebProcess` / `WebKitNetworkProcess` on Linux. They wait for the app's own "ready" log line, idle 30 s, then measure. Median of 3 runs per mode. Runs whose hide/destroy didn't happen are left out.
- CI: `.github/workflows/spike-idle-memory.yml` (manual), on GitHub's `windows-latest`, `macos-latest` and `ubuntu-latest` runners.

## Results

Median of 3 valid runs, 30 s idle. MiB.

| OS | Metric | Window open | Hidden | **Destroyed** |
|---|---|---|---|---|
| Windows (CI) | working set / private bytes | 326 / 106 (7 processes) | 332 / 111 | **27 / 5** (1 process) |
| macOS (CI) | RSS / physical footprint | 192 / 65 (3) | 189 / 65 | **124 / 31** (2) |
| macOS (M-series laptop) | RSS / footprint of the app process* | 216 / 98 (4) | 216 / 111 | **166 / 59** (3) |
| Linux (CI, Xvfb) | RSS / PSS | 523 / 359 (3) | 515 / 355 | **213 / 145** (2) |

\* Without sudo, macOS doesn't show the footprint of Apple's WebKit processes, so the laptop's footprint column only covers the app process. Its RSS column covers everything.

- **Windows:** destroying the last webview ends the whole WebView2 browser process family: one 27 MiB process remains.
- **macOS:** the WebContent process goes; the shared WebKit networking service stays.
- **Linux:** the web process goes; the network process stays. The numbers are inflated on the runner by software rendering (Mesa llvmpipe under Xvfb); PSS, which splits shared libraries fairly, is the better guide there and is under the target.
- **Hidden ≈ open** everywhere: a hidden webview keeps all its processes and memory.

**Re-opening after a destroy** (window created → page loaded): 92 ms on the laptop (release build). On the Linux runner, page loads took ≈ 5.7 s in any mode, because of software rendering; that's the runner, not the destroy.

## Findings beyond the question

1. **Linux, no desktop portal → startup stalls.** On the runner (no `xdg-desktop-portal`), creating the window blocks the main thread in tao's `portal::theme()`, a blocking D-Bus call, until it times out: about 25–30 s. Regular desktops (GNOME, KDE, …) have a portal, but minimal setups (tiling window managers, some containers) may not. Follow-up for the Linux adapter: check whether passing an explicit theme to the window skips the query, or file it upstream.
2. **Window calls from a worker thread can wait forever on Linux** while the main thread is blocked (see 1). Do window operations on the main thread (`run_on_main_thread`), as the spike now does.
3. **Kill the whole process tree in tests.** Killing only the app left WebKit helper processes behind on Linux, which stalled the next run.

## Decision

- **Confirmed (ADR-0004): destroy the webview while Kipple lives in the tray**; re-create it when the window opens. Never just hide it.
- The tray menu and notifications use the Rust catalog (`kipple-i18n-rs`); this spike shows that works with no webview at all.
- The tray itself is a v1 feature (FUNCTIONAL_PLAN); this spike only proves the approach.

## Reproduce

```sh
pnpm -C app build                                        # the UI the spike loads
cargo build --release --manifest-path spikes/s8-idle-memory/src-tauri/Cargo.toml
bash spikes/s8-idle-memory/measure.sh                    # macOS; Linux: xvfb-run -a bash …
pwsh spikes/s8-idle-memory/measure.ps1                   # Windows
S8_LOCALE=nl-BE S8_MODE=open spikes/s8-idle-memory/src-tauri/target/release/kipple-spike-s8   # Dutch tray menu
```

Or run the "Spike S8 · idle memory" workflow (inputs: runs, idle seconds, runners).
