// SPDX-License-Identifier: GPL-3.0-or-later
//! Spike S8 (K-1.11): how much memory does Kipple use while it only lives in the tray?
//! Loads the real Kipple UI (app/dist), adds a tray icon whose menu comes from the Rust catalog
//! (kipple-i18n-rs, in the OS language), then after S8_DELAY_MS:
//!   S8_MODE=open     keeps the window (baseline)
//!   S8_MODE=hide     hides the window (webview kept alive)
//!   S8_MODE=destroy  destroys the window and its webview (ADR-0004's plan)
//! and keeps running in the tray. measure.sh / measure.ps1 sample the whole process tree.
//! S8_LOCALE=nl-BE shows the Dutch tray menu. S8_REOPEN_AFTER_MS reopens the window that long after the hide/destroy and logs how long
//! the UI takes to load again (the cost of destroying it).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use std::time::{Duration, Instant};

use kipple_i18n_rs::{format, key, Locale};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

fn env_ms(name: &str) -> Option<Duration> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
}

/// When the window was last (re)opened, to time the page load.
struct OpenedAt(Mutex<Instant>);

fn open_window(app: &AppHandle) -> tauri::Result<()> {
    *app.state::<OpenedAt>().0.lock().unwrap() = Instant::now();
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        return window.set_focus();
    }
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Kipple S8")
        .inner_size(1180.0, 740.0)
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let opened = *window.state::<OpenedAt>().0.lock().unwrap();
                eprintln!(
                    "S8: page loaded {} ms after opening",
                    opened.elapsed().as_millis()
                );
            }
        })
        .build()
        .map(|_| ())
}

fn main() {
    let mode = std::env::var("S8_MODE").unwrap_or_else(|_| "destroy".into());
    let delay = env_ms("S8_DELAY_MS").unwrap_or(Duration::from_secs(8));
    let reopen_after = env_ms("S8_REOPEN_AFTER_MS");
    // S8_LOCALE (e.g. nl-BE) overrides the OS language, to see the Dutch menu.
    let tag = std::env::var("S8_LOCALE")
        .ok()
        .or_else(sys_locale::get_locale);
    let locale = Locale::from_tag(&tag.unwrap_or_default());
    let text = move |k| format(locale, k, &[]).expect("tray keys exist in the catalog");

    let app = tauri::Builder::default()
        .manage(OpenedAt(Mutex::new(Instant::now())))
        .setup(move |app| {
            let open = MenuItem::with_id(app, "open", text(key::TRAY_OPEN), true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", text(key::TRAY_QUIT), true, None::<&str>)?;
            let icon = app.default_window_icon().cloned().expect("an app icon");
            TrayIconBuilder::with_id("s8")
                .icon(icon)
                .tooltip("Kipple S8")
                .menu(&Menu::with_items(app, &[&open, &quit])?)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => drop(open_window(app)),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            eprintln!(
                "S8: tray menu [{}] [{}] ({locale:?})",
                text(key::TRAY_OPEN),
                text(key::TRAY_QUIT)
            );
            open_window(app.handle())?;

            let (handle, mode) = (app.handle().clone(), mode.clone());
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                eprintln!("S8: {mode} requested");
                // On the main thread: from a worker thread the call waited forever on Linux.
                let main = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    let window = main.get_webview_window("main").expect("the main window");
                    let result = match mode.as_str() {
                        "hide" => window.hide(),
                        "destroy" => window.destroy(),
                        _ => Ok(()),
                    };
                    eprintln!("S8: {mode} -> {result:?}");
                });
                if let Some(after) = reopen_after {
                    std::thread::sleep(after);
                    let reopen = handle.clone();
                    let _ = handle.run_on_main_thread(move || drop(open_window(&reopen)));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the spike app");
    // Like Kipple in the tray: closing the last window doesn't quit.
    app.run(|_, event| {
        if let RunEvent::ExitRequested {
            api, code: None, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}
