# Developing Kipple

How to set up your machine to build and test Kipple. For workflow rules (TDD, DCO sign-off, Definition of Done), see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Pinned versions

| Tool | Version | Pinned in |
|---|---|---|
| Rust | **1.99.1** + `rustfmt`, `clippy` | [`rust-toolchain.toml`](../rust-toolchain.toml). Run `rustup toolchain install` once in the repo root to install it |
| Node.js | **24** (LTS) | [`.nvmrc`](../.nvmrc), `engines` in [`package.json`](../package.json) |
| pnpm | **11.1.2** | `packageManager` in [`package.json`](../package.json), via Corepack |

You only need to install **rustup** and **Node 24**. The exact versions come from these files.

## Prerequisites per OS

Kipple is a [Tauri 2](https://v2.tauri.app/start/prerequisites/) app, so you need the Tauri system dependencies plus Rust and Node. If something here disagrees with the Tauri prerequisites page, that page wins; please open a PR to fix this one.

### macOS

1. **Xcode Command Line Tools** (or the full Xcode):
   ```sh
   xcode-select --install
   ```
2. **Rust** via rustup (don't use Homebrew's `rust`):
   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   . "$HOME/.cargo/env"
   rustup target add x86_64-apple-darwin aarch64-apple-darwin   # universal .dmg builds
   ```
3. **Node 24**, e.g. with [nvm](https://github.com/nvm-sh/nvm): `nvm install` (reads `.nvmrc`).
4. **pnpm** via Corepack: `corepack enable`. If `corepack` is missing, run `npm install -g corepack` first.

### Windows 10 / 11

1. **Microsoft C++ Build Tools**: install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and select **"Desktop development with C++"**.
2. **WebView2**: already included in Windows 10 (1803+) and 11. Otherwise install the [Evergreen runtime](https://developer.microsoft.com/microsoft-edge/webview2/).
3. **Rust** via [rustup-init.exe](https://rustup.rs/), with the **MSVC** toolchain (the default).
4. **Node 24** ([installer](https://nodejs.org/) or [nvm-windows](https://github.com/coreybutler/nvm-windows)), then `corepack enable` in an admin terminal.
5. Recommended: enable long paths (`git config --global core.longpaths true`) and Windows Developer Mode.

### Linux

Install the system libraries, then Rust and Node as on macOS (rustup + nvm + `corepack enable`).

**Debian / Ubuntu**
```sh
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
# For media previews (video/audio) in the webview:
sudo apt install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav
```

**Fedora**
```sh
sudo dnf check-update
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
  libappindicator-gtk3-devel librsvg2-devel libxdo-devel
sudo dnf group install "c-development"
```

**Arch**
```sh
sudo pacman -Syu
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg xdotool
```

## Check your setup

From the repo root:

```sh
rustup toolchain install   # installs the toolchain + components pinned in rust-toolchain.toml
rustc --version        # 1.99.1
cargo clippy --version
cargo fmt --version
node --version         # v24.x
pnpm --version         # 11.1.2
```

If `clippy` or `rustfmt` is reported as "not installed", run `rustup toolchain install` in the repo root (it installs the components listed in `rust-toolchain.toml`), or `rustup component add clippy rustfmt`.

### macOS gotcha: a non-Apple `xattr` in your PATH

If `tauri build` fails with `failed to remove extra attributes from app bundle: failed to run xattr`, a Python `xattr` (pyenv, conda, pip) is shadowing Apple's `/usr/bin/xattr`. Check with `which -a xattr`, and make sure `/usr/bin` comes first when building, e.g. `PATH="/usr/bin:$PATH" pnpm tauri build`.

## Commands available today

These are the same checks CI runs (`.github/workflows/ci.yml`). Run them before opening a PR:

| Command | What it does |
|---|---|
| `cargo test --workspace` | All Rust tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lints, including the **forbidden APIs**: file mutation outside the Executor ([ADR-0002](adr/0002-plan-review-apply.md)) and any I/O in `kipple-core` |
| `cargo fmt --all` | Formats the code |
| `cargo xtask layering` | Checks that crates only depend inward (ARCHITECTURE §2). A new crate needs a rule in `xtask/src/layering.rs` |
| `cargo xtask clippy-config` | Checks that the **generated** `clippy.toml` and `crates/kipple-core/clippy.toml` are up to date. To change the forbidden APIs, edit `xtask/src/clippy_config.rs` and run `cargo xtask clippy-config --write`; never edit the TOML files by hand |
| `cargo deny check` | Licences (GPL-3.0-compatible only), security advisories, crate sources. Install with `brew install cargo-deny` or `cargo install cargo-deny --locked` |
| `pnpm check:spdx` | Checks that every source file has the `GPL-3.0-or-later` licence header |
| `pnpm check:npm-licenses` | Checks that every npm package in the lockfile has a licence on the `deny.toml` allowlist (plus OFL-1.1 for the bundled fonts and the permissive BlueOak-1.0.0) |
| `pnpm test:scripts` | Runs the tests for the repo scripts |

**The app and the frontend** (run `pnpm install` once in the repo root first):

| Command | What it does |
|---|---|
| `pnpm -C app tauri dev` | Runs Kipple with hot reload (Vite on port 1420 + the Tauri shell) |
| `pnpm -C app test` | Frontend tests (Vitest + Testing Library) |
| `pnpm -C app typecheck` | TypeScript check |
| `pnpm -C app tauri build` | Release build and installers for the current OS |
| `KIPPLE_UPDATE_BINDINGS=1 cargo test -p kipple-app bindings` | Regenerates the typed IPC bindings `app/src/bindings/index.ts` after changing a command or IPC type in `app/src-tauri/src/ipc.rs`. CI fails if they're stale |

**Adding user-visible text:** add the key to `locales/en/*.json` and `locales/nl/*.json` (ICU MessageFormat, see [`locales/README.md`](../locales/README.md)); UI text goes in `app.json`, tray and notification text in `tray.json`. `pnpm -C app test` fails on missing or mismatched keys and placeholders, and on unused UI keys; the Rust build fails if a language misses a tray key. Rust code refers to tray keys through the generated `kipple_i18n_rs::key::*` constants, so a typo doesn't compile. Check the layout with the pseudo-locale: `VITE_KIPPLE_LOCALE=pseudo pnpm -C app tauri dev`.

**Adding an IPC command:** write it in `ipc.rs` with `#[tauri::command] #[specta::specta]`, add it to `collect_commands!` and to `src/command_names.rs`, grant `allow-<command-name>` in `capabilities/default.json`, then regenerate the bindings. Tests fail if any of these disagree. Commands return data or `Coded { code, params }`, never display text (ADR-0004).

The shell's security settings (CSP, only explicitly listed `allow-<command>` capabilities, no file-system plugin, navigation guard) are pinned by tests in `app/src-tauri/tests/security_config.rs` and `src/navigation.rs`. Changing them needs an ADR change first ([ADR-0007](adr/0007-backend-trust-boundary.md)).

**Runtime `<style>` tags** (Radix's scroll lock for dialogs and menus) get Tauri's per-load CSP nonce through the empty `<style>` in `app/index.html` and `src/styleNonce.ts`, so `style-src` stays `'self'`. Don't remove that tag.

⚠️ **`tauri dev` doesn't enforce the CSP on desktop** (it loads the Vite server directly). Before relying on a new asset type, font or style technique, check it in a release-mode build: `pnpm -C app tauri build --debug`. Runtime CSS-in-JS libraries that inject `<style>` tags won't work under `style-src 'self'`.

**Logging:** use `tracing`, and put paths in a field named `path`, `file`, `dir` or `*_path`/`*_file`/`*_dir`, never in the message text: `tracing::info!(path = %path.display(), "scanned")`. Those fields are hashed whole; every line is also scrubbed for anything else that looks like a path (third-party errors, panics, `log` crate messages), but the scrubber can't tell where an unquoted path with spaces ends. A path shows up as `<path#1a2b3c4d5e6f>`, an HMAC with a per-install key (`redaction.key` in the app data dir, never exported), so the same file always gets the same tag. Log files roll daily in the app log dir (`~/Library/Logs/app.kipple.desktop` on macOS, `%LOCALAPPDATA%\app.kipple.desktop\logs` on Windows, `~/.local/share/app.kipple.desktop/logs` on Linux) and a week is kept. Set the level with `KIPPLE_LOG=debug`, or per target: `KIPPLE_LOG=kipple_app=trace` (everything else stays at `info`).

**Need an exception to the forbidden APIs?** Only the Executor and the platform adapters it calls may change files. Put `#[allow(clippy::disallowed_methods)]` on that specific module with a comment saying why; reviewers check every one.

## Releasing

Releases are built by `.github/workflows/release.yml` when a `vX.Y.Z` tag is pushed. It makes a **draft** GitHub release with every installer (Windows NSIS per user, MSI and a portable zip; a universal macOS `.dmg` signed with the self-signed "Kipple Release Signing" certificate, [ADR-0008](adr/0008-macos-signing-without-notarisation.md); Linux AppImage, `.deb` and `.rpm`), the signed updater bundles with `latest.json`, `SHA256SUMS.txt` and build-provenance attestations.

**One-time setup (maintainer, on macOS):** `bash scripts/release/create-release-secrets.sh` creates the updater signing key and the macOS certificate in `~/kipple-release-secrets`, uploads them as GitHub Actions secrets and writes the public updater key into `app/src-tauri/tauri.release.conf.json` (commit that). It prints no private key or password. **Back the folder up offline, then delete it**: losing the updater key breaks updates for every installed copy, and a new certificate makes macOS ask every user for folder permissions again.

**Cutting a release:**
1. Set the version in the root `Cargo.toml` (`[workspace.package] version`). Pre-releases need a numeric suffix (`0.2.0-1`, not `0.2.0-rc.1`), because MSI versions can only hold numbers.
2. Commit, then `git tag v0.2.0 && git push origin v0.2.0`. The workflow fails early if the tag and the version differ, the pre-release suffix isn't numeric, or the updater public key is missing. Re-running it reuses the same draft.
3. Check the draft (install it on each OS, see [INSTALL.md](INSTALL.md)), then publish it on GitHub.

Local builds (`pnpm -C app tauri build`) don't need any of the secrets: updater bundles are only made with `--config src-tauri/tauri.release.conf.json`.

## Repository layout (planned)

```
crates/      Rust workspace: core (pure domain), index, engine, platform, preview, i18n-rs, cli
app/         Tauri shell (src-tauri) + React UI (src)
locales/     en/ and nl/: all user-visible text
docs/        plan, architecture, ADRs, design, tasks
scripts/     repo tooling (+ tests in scripts/tests)
spikes/      throwaway prototypes (not shipped)
```

See [ARCHITECTURE.md §4](ARCHITECTURE.md#4-repository-layout) for details.

## Editor setup

- **Rust:** [rust-analyzer](https://rust-analyzer.github.io/) (it uses the pinned toolchain automatically).
- **TypeScript/React:** ESLint and Prettier integrations, configured once the frontend lands.
- `.editorconfig` sets UTF-8, LF line endings, and 2-space indentation (4 for Rust).

## Commit sign-off

Every commit needs a DCO sign-off. Configure your identity once, then always commit with `-s`:

```sh
git config user.name  "Your Name"
git config user.email "you@example.com"
git commit -s -m "feat: ..."
```
