# Contributing to Kipple

Thanks for helping fight the kipple! This guide explains how to work on Kipple and what a change needs before it can be merged.

## Before you start

- **Check the plan.** The scope is in [docs/FUNCTIONAL_PLAN.md](docs/FUNCTIONAL_PLAN.md), including the **non-goals** (Kipple is *not* a system cleaner). The build plan is in [docs/MVP_TASKS.md](docs/MVP_TASKS.md), where every task has an ID such as `K-2.6`.
- **For anything bigger than a small fix, open an issue first** so we can agree on the approach.
- **Learn the words.** [docs/CONTEXT.md](docs/CONTEXT.md) defines the domain terms (Plan, Batch, Journal, Candidate…). Use them in code, commits and UI copy.
- **Set up your machine** with [docs/DEVELOPING.md](docs/DEVELOPING.md).

## Workflow

1. **Branch** from `main`: `feat/<short-name>`, `fix/<short-name>`, `docs/<short-name>` or `spike/<short-name>`.
2. **Write the test first.** Kipple is built with TDD (red → green → refactor). A PR without tests for new behaviour won't be merged.
3. **Commit** using [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`), and **sign off every commit** (see DCO below).
4. **Open a pull request** with the template filled in, and mention the task ID (`K-x.y`) if there is one.

## Developer Certificate of Origin (DCO)

Kipple uses the [Developer Certificate of Origin](https://developercertificate.org/) instead of a CLA. By signing off a commit, you certify that you wrote the change, or otherwise have the right to submit it under the project's licence (GPL-3.0-or-later).

Sign off by adding `-s` to `git commit`:

```sh
git commit -s -m "feat(scan): skip bundles during walk"
```

This adds a line like `Signed-off-by: Your Name <you@example.com>` to the commit message. It must match the commit author. To fix the last commit, run `git commit --amend -s --no-edit`.

## Definition of Done

Every change must meet all of these:

- [ ] **Tests written first** and passing in CI on **macOS, Windows and Linux**
- [ ] `cargo fmt`, `clippy`, ESLint and type checks are clean
- [ ] **No user-visible text in code.** Strings go in `locales/en` **and** `locales/nl`, and the backend returns codes + parameters ([ADR-0004](docs/adr/0004-i18n-codes-not-text.md))
- [ ] **No file mutation outside the Executor** ([ADR-0002](docs/adr/0002-plan-review-apply.md))
- [ ] **No raw file paths from the frontend.** The backend acts only on item IDs and dialog tokens ([ADR-0007](docs/adr/0007-backend-trust-boundary.md))
- [ ] UI uses **design tokens** only (no hex values), with keyboard access and a visible focus ring ([DESIGN.md](docs/DESIGN.md))
- [ ] New source files start with the licence header (see below)
- [ ] Docs are updated where behaviour or terms changed (CONTEXT.md, ADRs, ARCHITECTURE.md)

## Licence headers

Every source file (`.rs`, `.ts`, `.tsx`, `.js`, `.css`, `.sh`, …) starts with an SPDX identifier in its first five lines:

```rust
// SPDX-License-Identifier: GPL-3.0-or-later
```

```css
/* SPDX-License-Identifier: GPL-3.0-or-later */
```

```sh
# SPDX-License-Identifier: GPL-3.0-or-later
```

Check it with `pnpm check:spdx` (or `bash scripts/check-spdx.sh`).

**Dependencies** must have a GPL-3.0-compatible licence (MIT, BSD, Apache-2.0, MPL-2.0, LGPL, GPL-3.0). GPL-2.0-only, proprietary and "non-commercial" licences can't be used. CI enforces this.

## Translations

- Every new string needs an **English and a Dutch** entry. Use full sentences with ICU placeholders and plurals, and never build sentences by concatenating strings.
- Dutch uses informal *je/jij*. Native-speaker review is welcome on any PR that touches `locales/nl`.
- Want to add a language? Open an issue. Community translation tooling will come later.

## UI changes

Include screenshots in **light and dark** mode and in **EN and NL** in the PR. Follow the voice & tone in [DESIGN.md](docs/DESIGN.md): playful where nothing is at stake, calm and plain wherever files could be lost.

## Security

Never report a vulnerability in a public issue. See [SECURITY.md](SECURITY.md).

## Code of Conduct

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
