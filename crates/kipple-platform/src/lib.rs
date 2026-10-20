// SPDX-License-Identifier: GPL-3.0-or-later
//! OS port traits with macOS, Windows and Linux adapters, plus in-memory fakes.

// FFI to OS APIs lives here, so `unsafe` is allowed in this crate; clippy requires a
// `// SAFETY:` comment on every unsafe block (undocumented_unsafe_blocks).
