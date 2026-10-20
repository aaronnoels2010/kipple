// SPDX-License-Identifier: GPL-3.0-or-later
//! ADR-0007 as tests: the webview is untrusted, so the shell's security settings must not be
//! loosened by accident. These compare against exact expectations: any change, even an
//! addition, fails here and needs an ADR update first.

use std::collections::BTreeMap;

use serde_json::{Value, json};

fn conf() -> Value {
    serde_json::from_str(include_str!("../tauri.conf.json")).unwrap()
}

/// CSP as directive → sources, so order and whitespace don't matter but content does.
fn csp_map(csp: &str) -> BTreeMap<String, Vec<String>> {
    csp.split(';')
        .filter_map(|d| {
            let mut parts = d.split_whitespace();
            let name = parts.next()?.to_string();
            Some((name, parts.map(str::to_string).collect()))
        })
        .collect()
}

fn expected_csp(extra: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    let mut m = csp_map(
        "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; \
         connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; \
         form-action 'none'; frame-ancestors 'none'",
    );
    for (directive, sources) in extra {
        m.insert(
            directive.to_string(),
            sources.iter().map(|s| s.to_string()).collect(),
        );
    }
    m
}

#[test]
fn security_block_has_exactly_the_reviewed_keys() {
    let security = conf()["app"]["security"].as_object().unwrap().clone();
    let keys: Vec<&str> = security.keys().map(String::as_str).collect();
    // No inline `capabilities`, no isolation `pattern`, no `dangerous*` switches.
    assert_eq!(keys, ["assetProtocol", "csp", "devCsp", "freezePrototype"]);
    assert_eq!(security["freezePrototype"], true);
    // Previews go through the per-item kipple-preview:// scheme instead (ADR-0007).
    assert_eq!(security["assetProtocol"], json!({ "enable": false }));
}

#[test]
fn no_global_tauri_object() {
    assert_eq!(conf()["app"]["withGlobalTauri"], false);
}

#[test]
fn release_csp_is_exactly_the_reviewed_policy() {
    let csp = conf()["app"]["security"]["csp"]
        .as_str()
        .unwrap()
        .to_string();
    // ipc: (macOS, Linux) and http://ipc.localhost (Windows) are Tauri's IPC endpoints.
    assert_eq!(csp_map(&csp), expected_csp(&[]));
}

#[test]
fn dev_csp_only_adds_what_vite_hot_reload_needs() {
    let csp = conf()["app"]["security"]["devCsp"]
        .as_str()
        .unwrap()
        .to_string();
    let expected = expected_csp(&[
        ("script-src", &["'self'", "'unsafe-inline'"]), // React Refresh preamble
        ("style-src", &["'self'", "'unsafe-inline'"]),  // Vite injects CSS as <style> in dev
        (
            "connect-src",
            &[
                "'self'",
                "ipc:",
                "http://ipc.localhost",
                "ws://localhost:1420",
            ],
        ),
    ]);
    assert_eq!(csp_map(&csp), expected);
}

#[test]
fn no_windows_declared_in_config() {
    // The main window is built in lib.rs so `on_navigation` / `on_new_window` are always attached.
    assert_eq!(conf()["app"]["windows"], json!([]));
}

#[test]
fn exactly_one_capability_and_it_grants_nothing() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        files,
        ["default.json"],
        "Tauri loads every file in capabilities/"
    );

    let cap: Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
    assert_eq!(cap["windows"], json!(["main"]));
    assert!(cap.get("remote").is_none(), "no IPC for remote origins");
    // The UI uses no IPC yet. Add specific `core:*:allow-*` permissions only when needed, never
    // `core:default` (it exposes home/app-data paths, menus and image loading).
    assert_eq!(cap["permissions"], json!([]));
}

#[test]
fn no_file_system_shell_or_http_plugin() {
    let manifest = include_str!("../Cargo.toml");
    for plugin in ["tauri-plugin-fs", "tauri-plugin-shell", "tauri-plugin-http"] {
        assert!(
            !manifest.contains(plugin),
            "{plugin} must not be a dependency"
        );
    }
}
