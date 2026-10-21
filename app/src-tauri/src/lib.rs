// SPDX-License-Identifier: GPL-3.0-or-later
//! Kipple's Tauri shell: windows, plugins and wiring. Holds no business logic (ARCHITECTURE §2).
#![forbid(unsafe_code)]

mod appearance;
mod ipc;
mod locations;
mod locations_store;
mod logging;
mod navigation;
mod own_files;
mod portable;
mod redact;
mod scans;
mod settings;

use appearance::AppearanceState;
use navigation::{Decision, Platform};
use settings::ThemePreference;
use tauri::webview::NewWindowResponse;
use tauri::{
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;

const MAIN_WINDOW: &str = "main";

pub fn run() {
    let mut context = tauri::generate_context!();
    let mode = portable::apply(&mut context);
    let ipc = ipc::specta_builder();
    let invoke_handler = ipc.invoke_handler();
    tauri::Builder::default()
        .invoke_handler(invoke_handler)
        // Must be registered first: a second launch focuses the running instance instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            focus_main_window(app)
        }))
        // Its injected click handler would bypass `on_navigation` (and has no IPC permission
        // anyway), so every link goes through the guard below instead.
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        // Rust opens the native folder dialog itself (ADR-0007); the webview gets no dialog
        // permission, so it can't open one, let alone pass a path.
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // First, so everything after it is logged. Without these folders, run unlogged.
            let paths = app.path();
            if let (Ok(logs), Ok(data)) = (paths.app_log_dir(), paths.app_data_dir()) {
                logging::init(&logs, &data.join("redaction.key"));
            }
            tracing::info!(
                version = %app.package_info().version,
                os = std::env::consts::OS,
                "Kipple started"
            );
            match &mode {
                portable::Mode::Installed => {}
                portable::Mode::Portable => tracing::info!("portable mode: data in KippleData"),
                portable::Mode::Unwritable(error) => tracing::warn!(
                    error = %error,
                    "portable marker found but KippleData can't be created; using the user folders"
                ),
            }
            ipc.mount_events(app);
            let settings_path = app
                .path()
                .app_config_dir()?
                .join("config")
                .join("settings.json");
            app.manage(AppearanceState::load(settings_path));
            // macOS: no read anywhere in Kipple can download an online-only file. Windows: cloud
            // files show up as such (readers check `is_placeholder` before reading).
            if let Err(e) = kipple_platform::never_download_placeholders() {
                tracing::warn!(error = %e, "couldn't turn off cloud downloads");
            }
            let locations = locations::LocationsState::from_app(app.handle())?;
            app.manage(scans::Scans::from_app(app.handle(), &locations)?);
            app.manage(locations);
            let state = app.state::<AppearanceState>();

            let window = create_main_window(app.handle(), state.preference())?;
            // Online checks can be slow (a disconnected network share): after the window is up.
            let locations = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                locations
                    .state::<locations::LocationsState>()
                    .refresh_online();
                // Rescans are quick (S4): bring the index up to date with what happened while
                // Kipple was closed.
                scans::start(&locations, None);
            });
            appearance::init(&window, &state);
            let (handle, observed) = (app.handle().clone(), window.clone());
            window.on_window_event(move |event| {
                if let WindowEvent::ThemeChanged(_) = event {
                    let state = handle.state::<AppearanceState>();
                    appearance::on_window_theme_changed(&handle, &observed, &state);
                }
            });
            Ok(())
        })
        .build(context)
        .expect("Kipple failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(scans) = app.try_state::<scans::Scans>()
            {
                // Scans stop at the next folder; give them a moment to commit what they wrote.
                scans.cancel(None);
                scans.wait(std::time::Duration::from_secs(2));
            }
        });
}

/// Built in code (not in tauri.conf.json) so the navigation guard is always attached.
fn create_main_window(app: &AppHandle, theme: ThemePreference) -> tauri::Result<WebviewWindow> {
    let dev_url = tauri::is_dev()
        .then(|| app.config().build.dev_url.clone())
        .flatten();
    let (nav_app, popup_app) = (app.clone(), app.clone());
    WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title("Kipple")
        .inner_size(1180.0, 740.0)
        .min_inner_size(960.0, 600.0)
        // Start in the saved theme: native chrome via `.theme`, the page via data-theme before
        // any app script runs (no flash). "System" is resolved by the page until the backend
        // reports the window's real theme.
        .theme(appearance::forced_theme(theme))
        .initialization_script(theme_bootstrap_script(theme))
        .on_navigation(move |url| {
            match navigation::decide(url, dev_url.as_ref(), Platform::current()) {
                Decision::Allow => true,
                Decision::OpenExternally => {
                    open_externally(&nav_app, url);
                    false
                }
                Decision::Block => false,
            }
        })
        // window.open / target="_blank": never a second webview (it would have no guard).
        .on_new_window(move |url, _features| {
            if navigation::decide(&url, None, Platform::current()) == Decision::OpenExternally {
                open_externally(&popup_app, &url);
            }
            NewWindowResponse::Deny
        })
        .build()
}

fn theme_bootstrap_script(theme: ThemePreference) -> String {
    match theme {
        ThemePreference::Light => "document.documentElement.dataset.theme = 'light';".into(),
        ThemePreference::Dark => "document.documentElement.dataset.theme = 'dark';".into(),
        ThemePreference::System => String::new(),
    }
}

fn open_externally(app: &AppHandle, url: &Url) {
    // Best effort: if no default browser/mail app is registered, the click simply does nothing.
    let _ = app.opener().open_url(url.as_str(), None::<&str>);
}

fn focus_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
