// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike S2 (K-0.3): does an unsigned or self-signed Kipple keep its macOS folder
//! permissions (TCC) across updates? Throwaway code, never shipped.

use serde::Serialize;
use std::path::PathBuf;

/// The only folders the probe may read: the TCC-protected user folders.
const PROTECTED_FOLDERS: [&str; 3] = ["Desktop", "Documents", "Downloads"];

#[derive(Serialize)]
struct Info {
    version: String,
    identifier: String,
    exe: String,
}

#[derive(Serialize)]
struct Probe {
    folder: String,
    ok: bool,
    entries: usize,
    error: Option<String>,
}

#[tauri::command]
fn info(app: tauri::AppHandle) -> Info {
    Info {
        version: app.package_info().version.to_string(),
        identifier: app.config().identifier.clone(),
        exe: std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
    }
}

/// Lists one protected folder. On macOS the first call triggers the TCC prompt
/// ("… would like to access files in your Desktop folder").
#[tauri::command]
fn probe(folder: String) -> Probe {
    if !PROTECTED_FOLDERS.contains(&folder.as_str()) {
        return Probe { folder, ok: false, entries: 0, error: Some("folder not allowed".into()) };
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    match std::fs::read_dir(home.join(&folder)) {
        Ok(entries) => Probe { folder, ok: true, entries: entries.count(), error: None },
        Err(e) => Probe { folder, ok: false, entries: 0, error: Some(e.to_string()) },
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![info, probe])
        .run(tauri::generate_context!())
        .expect("error while running the TCC spike");
}
