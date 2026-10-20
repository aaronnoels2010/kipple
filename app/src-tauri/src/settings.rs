// SPDX-License-Identifier: GPL-3.0-or-later
//! `<app config dir>/config/settings.json`: the user's preferences (ADR-0003, portable,
//! hand-editable config). Reading is lenient: a bad value falls back to its default without
//! losing the rest, keys from a newer Kipple are kept, and a file that isn't JSON is set aside as
//! `settings.json.bad` instead of being overwritten.

use std::io::Write;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use specta::Type;

const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Highest schema version that wrote this file; never lowered (a newer Kipple may rely on it).
    #[serde(default)]
    version: u32,
    #[serde(default, deserialize_with = "lenient")]
    pub theme: ThemePreference,
    /// Keys this version doesn't know (e.g. written by a newer Kipple); preserved on save.
    #[serde(flatten)]
    other: Map<String, Value>,
}

/// An invalid value becomes the default instead of failing the whole file.
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).unwrap_or_default())
}

/// Missing or unreadable files give the defaults. A file that isn't a JSON object is renamed to
/// `settings.json.bad` so the next save can't destroy what the user wrote.
pub fn load(path: &Path) -> Settings {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Settings::default();
    };
    match serde_json::from_str::<Settings>(&text) {
        Ok(settings) => settings,
        Err(_) => {
            set_aside(path);
            Settings::default()
        }
    }
}

// Kipple's own config file, not a user file: ADR-0002's Executor rule covers files in managed
// locations only.
#[allow(clippy::disallowed_methods)]
fn set_aside(path: &Path) {
    let _ = std::fs::rename(path, path.with_extension("json.bad"));
}

/// Writes atomically (synced temporary file + rename), creating the config folder if needed.
#[allow(clippy::disallowed_methods)] // see `set_aside`
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let mut settings = settings.clone();
    settings.version = settings.version.max(VERSION);
    let text = serde_json::to_string_pretty(&settings)? + "\n";

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?; // the rename must never expose an empty file after a power loss
    drop(file);
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[allow(clippy::disallowed_methods)] // test fixture
    fn write(path: &Path, content: &str) {
        std::fs::write(path, content).unwrap();
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn missing_file_gives_defaults() {
        let d = dir();
        assert_eq!(load(&d.path().join("settings.json")), Settings::default());
        assert_eq!(Settings::default().theme, ThemePreference::System);
    }

    #[test]
    fn save_then_load_round_trips_and_creates_the_folder() {
        let d = dir();
        let path = d.path().join("config/settings.json");
        let s = Settings {
            theme: ThemePreference::Dark,
            ..Default::default()
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).theme, ThemePreference::Dark);
        let json = read_json(&path);
        assert_eq!(json["theme"], "dark");
        assert_eq!(json["version"], 1);
    }

    #[test]
    fn a_file_that_is_not_json_is_set_aside_not_overwritten() {
        let d = dir();
        let path = d.path().join("settings.json");
        write(&path, "{ not json");
        assert_eq!(load(&path), Settings::default());
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.bad")).unwrap(),
            "{ not json"
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_non_object_json_file_is_set_aside_too() {
        let d = dir();
        let path = d.path().join("settings.json");
        write(&path, "[1, 2, 3]");
        assert_eq!(load(&path), Settings::default());
        assert!(path.with_extension("json.bad").exists());
    }

    #[test]
    fn an_unknown_value_falls_back_without_losing_other_keys() {
        let d = dir();
        let path = d.path().join("settings.json");
        write(
            &path,
            r#"{ "version": 1, "theme": "purple", "language": "nl" }"#,
        );
        let s = load(&path);
        assert_eq!(s.theme, ThemePreference::System);
        save(&path, &s).unwrap();
        assert_eq!(
            read_json(&path)["language"],
            "nl",
            "keys from other versions must survive a save"
        );
    }

    #[test]
    fn a_newer_version_is_never_downgraded() {
        let d = dir();
        let path = d.path().join("settings.json");
        write(
            &path,
            r#"{ "version": 7, "theme": "light", "futureThing": true }"#,
        );
        let s = load(&path);
        save(&path, &s).unwrap();
        let json = read_json(&path);
        assert_eq!(json["version"], 7);
        assert_eq!(json["futureThing"], true);
    }

    #[test]
    fn save_leaves_no_temporary_files() {
        let d = dir();
        let path = d.path().join("settings.json");
        save(&path, &Settings::default()).unwrap();
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["settings.json"]);
    }
}
