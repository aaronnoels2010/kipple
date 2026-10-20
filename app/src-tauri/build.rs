// SPDX-License-Identifier: GPL-3.0-or-later
use std::env;
use std::path::Path;

include!("src/command_names.rs");

fn main() {
    // Declaring the app's commands makes each one need an explicit `allow-<command>`
    // permission in capabilities/ (ADR-0007: the UI only gets what it needs).
    let commands = tauri_build::AppManifest::new().commands(COMMAND_NAMES);
    // On Windows the Common-Controls v6 manifest is embedded below for *every* binary, test
    // executables included (otherwise they crash with STATUS_ENTRYPOINT_NOT_FOUND), so
    // tauri-build must not embed its own copy into the app as well.
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    let attributes = tauri_build::Attributes::new()
        .app_manifest(commands)
        .windows_attributes(windows);
    tauri_build::try_build(attributes).expect("tauri-build failed");

    let target_is_msvc = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if target_is_msvc {
        let manifest =
            Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");
    }
}
