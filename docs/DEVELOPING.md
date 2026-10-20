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

The app skeleton arrives with task K-1.x ([MVP_TASKS.md](MVP_TASKS.md)). Until then:

| Command | What it does |
|---|---|
| `pnpm check:spdx` | Checks that every source file has the `GPL-3.0-or-later` licence header |
| `pnpm test:scripts` | Runs the tests for the repo scripts |

This table grows as the skeleton lands (`pnpm tauri dev`, `cargo test --workspace`, `pnpm test`, …).

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
