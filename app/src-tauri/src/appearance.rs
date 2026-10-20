// SPDX-License-Identifier: GPL-3.0-or-later
//! Light/dark appearance. The effective theme is read from the native window, which is more
//! reliable than the webview's `prefers-color-scheme` (notably WebKitGTK). The user's override is
//! stored in settings.json and applied app-wide, so every window and the title bars follow.
//!
//! Two platform traps shape this module:
//! - Windows: `set_theme` fires `ThemeChanged` synchronously on the main thread, so no lock may
//!   be held while calling window/app theme APIs (it would deadlock in `on_window_theme_changed`).
//! - Linux: following the OS (`set_theme(None)`) emits a bogus `ThemeChanged(Light)`, so event
//!   payloads are never trusted; the window is asked again instead.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Runtime, State, WebviewWindow};
use tauri_specta::Event;

use crate::ipc::Coded;
use crate::settings::{self, Settings, ThemePreference};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

impl From<tauri::Theme> for Theme {
    fn from(theme: tauri::Theme) -> Self {
        match theme {
            tauri::Theme::Dark => Theme::Dark,
            _ => Theme::Light,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Appearance {
    pub preference: ThemePreference,
    /// The theme actually shown: the override, or what the OS uses when following it.
    pub effective: Theme,
}

/// Sent when the OS switches between light and dark while Kipple follows it.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct AppearanceChanged {
    pub appearance: Appearance,
}

/// The theme to force; `None` follows the OS.
pub fn forced_theme(preference: ThemePreference) -> Option<tauri::Theme> {
    match preference {
        ThemePreference::System => None,
        ThemePreference::Light => Some(tauri::Theme::Light),
        ThemePreference::Dark => Some(tauri::Theme::Dark),
    }
}

/// The pure state: what is saved and what was last reported to the UI.
#[derive(Debug)]
struct Inner {
    settings: Settings,
    effective: Theme,
}

impl Inner {
    fn appearance(&self) -> Appearance {
        Appearance {
            preference: self.settings.theme,
            effective: self.effective,
        }
    }

    /// The window now shows `observed`. Returns the appearance to announce, if it changed.
    fn observed(&mut self, observed: Theme) -> Option<Appearance> {
        if observed == self.effective {
            return None;
        }
        self.effective = observed;
        Some(self.appearance())
    }
}

pub struct AppearanceState {
    settings_path: PathBuf,
    inner: Mutex<Inner>,
}

impl AppearanceState {
    pub fn load(settings_path: PathBuf) -> Self {
        let settings = settings::load(&settings_path);
        let effective = match settings.theme {
            ThemePreference::Dark => Theme::Dark,
            _ => Theme::Light, // corrected by `init` once the window exists
        };
        Self {
            settings_path,
            inner: Mutex::new(Inner {
                settings,
                effective,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn preference(&self) -> ThemePreference {
        self.lock().settings.theme
    }

    /// Saves first; memory only changes if the file was written.
    fn persist(&self, preference: ThemePreference) -> Result<(), Coded> {
        let mut inner = self.lock();
        let mut next = inner.settings.clone();
        next.theme = preference;
        settings::save(&self.settings_path, &next)
            .map_err(|_| Coded::new("settings_save_failed"))?;
        inner.settings = next;
        Ok(())
    }
}

/// Reads what the window shows now (no lock held: see the module docs).
fn read_window_theme<R: Runtime>(window: &WebviewWindow<R>) -> Option<Theme> {
    window.theme().ok().map(Theme::from)
}

/// Records the window's real theme after it was created with the saved preference.
pub fn init<R: Runtime>(window: &WebviewWindow<R>, state: &AppearanceState) {
    if let Some(theme) = read_window_theme(window) {
        state.lock().observed(theme);
    }
}

/// The window reported a theme change; re-read it instead of trusting the payload.
pub fn on_window_theme_changed<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    state: &AppearanceState,
) {
    let Some(theme) = read_window_theme(window) else {
        return;
    };
    let changed = state.lock().observed(theme);
    if let Some(appearance) = changed {
        let _ = AppearanceChanged { appearance }.emit(app);
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_appearance(state: State<'_, AppearanceState>) -> Appearance {
    state.lock().appearance()
}

#[tauri::command]
#[specta::specta]
pub fn set_theme_preference(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, AppearanceState>,
    preference: ThemePreference,
) -> Result<Appearance, Coded> {
    state.persist(preference)?;
    app.set_theme(forced_theme(preference));
    let effective = match forced_theme(preference) {
        Some(forced) => Theme::from(forced),
        None => read_window_theme(&window).unwrap_or(Theme::Light),
    };
    let mut inner = state.lock();
    inner.observed(effective);
    Ok(inner.appearance())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner(preference: ThemePreference, effective: Theme) -> Inner {
        let mut settings = Settings::default();
        settings.theme = preference;
        Inner {
            settings,
            effective,
        }
    }

    #[test]
    fn os_themes_map_to_light_and_dark() {
        assert_eq!(Theme::from(tauri::Theme::Light), Theme::Light);
        assert_eq!(Theme::from(tauri::Theme::Dark), Theme::Dark);
    }

    #[test]
    fn system_follows_the_os_and_overrides_force_a_theme() {
        assert_eq!(forced_theme(ThemePreference::System), None);
        assert_eq!(
            forced_theme(ThemePreference::Light),
            Some(tauri::Theme::Light)
        );
        assert_eq!(
            forced_theme(ThemePreference::Dark),
            Some(tauri::Theme::Dark)
        );
    }

    #[test]
    fn a_real_change_is_announced_once() {
        let mut s = inner(ThemePreference::System, Theme::Light);
        assert_eq!(
            s.observed(Theme::Dark),
            Some(Appearance {
                preference: ThemePreference::System,
                effective: Theme::Dark
            })
        );
        assert_eq!(
            s.observed(Theme::Dark),
            None,
            "no duplicate event for the same theme"
        );
    }

    #[test]
    fn under_an_override_the_window_keeps_reporting_the_override() {
        // An OS switch while forced to Dark: the window still reads Dark, so nothing changes.
        let mut s = inner(ThemePreference::Dark, Theme::Dark);
        assert_eq!(s.observed(Theme::Dark), None);
    }

    #[test]
    fn a_failed_save_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        // A folder where the file should be: the save must fail.
        let path = d.path().join("settings.json");
        #[allow(clippy::disallowed_methods)] // test fixture
        std::fs::create_dir(&path).unwrap();
        let state = AppearanceState::load(path);
        assert_eq!(
            state.persist(ThemePreference::Dark),
            Err(Coded::new("settings_save_failed"))
        );
        assert_eq!(state.preference(), ThemePreference::System);
    }

    #[test]
    fn a_successful_save_updates_the_preference() {
        let d = tempfile::tempdir().unwrap();
        let state = AppearanceState::load(d.path().join("config/settings.json"));
        state.persist(ThemePreference::Light).unwrap();
        assert_eq!(state.preference(), ThemePreference::Light);
        assert_eq!(
            AppearanceState::load(d.path().join("config/settings.json")).preference(),
            ThemePreference::Light
        );
    }

    #[test]
    fn appearance_serializes_for_the_frontend() {
        let a = Appearance {
            preference: ThemePreference::System,
            effective: Theme::Dark,
        };
        assert_eq!(
            serde_json::to_value(a).unwrap(),
            serde_json::json!({ "preference": "system", "effective": "dark" })
        );
    }
}
