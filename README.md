# Kipple

> *Fight the kipple.* · *Weg met de kipple.*

**Kipple** is a free, open-source desktop app for **Linux, macOS and Windows** that helps you find, understand, organise, rename and **safely** clean up your files.

In Philip K. Dick's *Do Androids Dream of Electric Sheep?*, *kipple* is the useless clutter that piles up by itself. Kipple is the app that fights back.

> **Status: pre-alpha.** Planning and design are done, and the MVP is being built. There are no releases yet.

## What it will do

- **Clean up with reasons.** Kipple finds duplicates, old installers, unfinished downloads, already-extracted archives, empty folders, large old files and old screenshots, and tells you *why* each one is suggested.
- **Safe by default.** Everything goes to your Trash or Recycle Bin, every action can be undone, and an interrupted operation is recovered on the next start.
- **Preview everything:** images, PDFs, text and code, video and audio, with a Quick Look-style preview.
- **Batch rename** with a live before → after preview and conflict detection.
- **One-click tidy presets**, with automatic rules later, only when you switch them on.
- **Private.** No accounts, no telemetry, no server. Nothing leaves your computer.
- **English and Dutch** at launch, ready for more languages.

## Documentation

| Document | What's in it |
|---|---|
| [Functional plan](docs/FUNCTIONAL_PLAN.md) | Features, priorities (MVP / v1 / later) and decisions |
| [Architecture](docs/ARCHITECTURE.md) | How it's built: Tauri 2 + Rust core + React |
| [Decisions (ADRs)](docs/adr/) | The key architectural decisions and why they were made |
| [Domain language](docs/CONTEXT.md) | The terms used in code, docs and UI |
| [Design system](docs/DESIGN.md) · [mockup](docs/design.html) | Theme (light & dark), components, screens |
| [MVP tasks](docs/MVP_TASKS.md) | The build plan towards v0.1.0 |
| [Developing](docs/DEVELOPING.md) | Setting up your machine to build Kipple |

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first: commits need a DCO sign-off (`git commit -s`), and code is written test-first.
Please follow the [Code of Conduct](CODE_OF_CONDUCT.md). To report a security issue, see [SECURITY.md](SECURITY.md); don't open a public issue for it.

## Licence

Kipple is licensed under the **GNU General Public License v3.0 or later** (`GPL-3.0-or-later`). See [LICENSE](LICENSE).
