// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike S8 (K-1.11): how much memory does Kipple use while it only lives in the tray?
//! Loads the real Kipple UI (app/dist), adds a tray icon, then after S8_DELAY_MS:
//!   S8_MODE=open     keeps the window (baseline)
//!   S8_MODE=hide     hides the window (webview kept alive)
//!   S8_MODE=destroy  destroys the window and its webview (ADR-0004's plan)
//! and keeps running in the tray. measure.sh / measure.ps1 sample the whole process tree.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use tauri::tray::TrayIconBuilder;
use tauri::{RunEvent, WebviewUrl, WebviewWindowBuilder};

fn env_ms(name: &str, default: u64) -> Duration {
    let ms = std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default);
    Duration::from_millis(ms)
}

fn main() {
    let mode = std::env::var("S8_MODE").unwrap_or_else(|_| "destroy".into());
    let delay = env_ms("S8_DELAY_MS", 8000);
    let app = tauri::Builder::default()
        .setup(move |app| {
            let icon = app.default_window_icon().cloned().expect("an app icon");
            TrayIconBuilder::with_id("s8").icon(icon).tooltip("Kipple S8").build(app)?;
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Kipple S8")
                .inner_size(1180.0, 740.0)
                .build()?;
            let mode = mode.clone();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                let result = match mode.as_str() {
                    "hide" => window.hide(),
                    "destroy" => window.destroy(),
                    _ => Ok(()),
                };
                eprintln!("S8: {mode} -> {result:?}");
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the spike app");
    // Like Kipple in the tray: closing the last window doesn't quit.
    app.run(|_, event| {
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
