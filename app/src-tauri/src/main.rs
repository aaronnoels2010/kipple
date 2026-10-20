// SPDX-License-Identifier: GPL-3.0-or-later
// No console window next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]

fn main() {
    kipple_app::run();
}
