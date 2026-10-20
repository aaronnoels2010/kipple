// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless harness: scan, plan, apply, bench and fault injection.

#![forbid(unsafe_code)]

fn main() {
    println!("kipple-cli {}", env!("CARGO_PKG_VERSION"));
}
