// SPDX-License-Identifier: GPL-3.0-or-later
include!("src/command_names.rs");

fn main() {
    // Declaring the app's commands makes each one need an explicit `allow-<command>`
    // permission in capabilities/ (ADR-0007: the UI only gets what it needs).
    let manifest = tauri_build::AppManifest::new().commands(COMMAND_NAMES);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("tauri-build failed");
}
