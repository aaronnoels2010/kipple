# ADR-0005: OS-specific behaviour lives behind platform ports (hexagonal architecture)

- Status: Accepted
- Date: 2026-10-20

## Context
Much of Kipple's value depends on OS-specific facts: Trash and restore, last-used signals, download origin, cloud placeholders, protected paths, bundles, file locks, permissions, thumbnails and Finder tags. The behaviour differs a lot between macOS, Windows and Linux, and parts of it are hard to test on a developer machine.

## Decision
- `kipple-platform` defines **port traits** (`Trash`, `LastUsed`, `DownloadOrigin`, `Placeholder`, `ProtectedPaths`, `Volume`, `LockInfo`, `Permissions`, `Thumbnails`, `Reveal`, `Tags`, `ShellIntegration`), plus one adapter per OS, selected with `cfg(target_os)`.
- `kipple-core` and `kipple-engine` depend **only on the traits**.
- Every port has an **in-memory fake** for unit and integration tests, and **contract tests** that run on the real OS in the CI matrix (macOS, Windows, Ubuntu).
- Ports that a platform doesn't support return a typed `Unsupported`, and the UI hides the related feature instead of failing.

## Consequences
- The core logic is fully testable on any machine (TDD friendly).
- OS quirks stay contained. Adding a platform-specific improvement doesn't touch the core.
- Each trait is one more layer to maintain, but it's thin compared with the OS code behind it.
