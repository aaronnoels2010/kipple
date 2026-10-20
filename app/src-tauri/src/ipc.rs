// SPDX-License-Identifier: GPL-3.0-or-later
//! Typed IPC: every command the UI can call, exported to TypeScript by tauri-specta.
//! The backend never sends display text, only codes + parameters (ADR-0004).

use std::collections::BTreeMap;

use serde::Serialize;
use specta::Type;

/// Something the UI should show, as a stable code plus parameters. The frontend turns it into
/// a translated sentence (`locales/{en,nl}`); the backend never formats text itself.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct Coded {
    pub code: String,
    pub params: BTreeMap<String, Param>,
}

/// A parameter of a [`Coded`] message. Numbers are `f64` because specta refuses 64-bit
/// integers (no BigInt over IPC); serde sends NaN/±Infinity as `null`, which is why the
/// TypeScript type includes `null`. Only pass finite numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(untagged)]
#[expect(
    dead_code,
    reason = "exported to TypeScript now; the first commands returning Coded arrive with the scanner"
)]
pub enum Param {
    Text(String),
    Number(f64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
pub struct AppInfo {
    pub version: String,
    /// `macos`, `windows` or `linux`.
    pub os: String,
}

fn app_info_for(version: &str) -> AppInfo {
    AppInfo {
        version: version.to_string(),
        os: std::env::consts::OS.to_string(),
    }
}

#[tauri::command]
#[specta::specta]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    app_info_for(&app.package_info().version.to_string())
}

/// The command list, shared by the invoke handler and the TypeScript export. Keep it in sync
/// with `command_names.rs` (a test checks it).
pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![app_info])
        .typ::<Coded>()
}

#[cfg(test)]
mod tests {
    use super::*;

    include!("command_names.rs");

    const BINDINGS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings/index.ts");
    const REGENERATE: &str = "KIPPLE_UPDATE_BINDINGS=1 cargo test -p kipple-app bindings";

    fn export(path: &std::path::Path) {
        let language = specta_typescript::Typescript::default().header(format!(
            "// SPDX-License-Identifier: GPL-3.0-or-later\n// Regenerate: {REGENERATE}\n"
        ));
        specta_builder()
            .export(language, path)
            .expect("export TypeScript bindings");
    }

    /// Exports to a temporary file and returns its content.
    fn fresh_bindings() -> String {
        let path = std::env::temp_dir().join(format!("kipple-bindings-{}.ts", std::process::id()));
        export(&path);
        let content = std::fs::read_to_string(&path).unwrap();
        // Test-only cleanup of our own temp file; ADR-0002's mutation ban is about user files.
        #[allow(clippy::disallowed_methods)]
        let _ = std::fs::remove_file(&path);
        content
    }

    #[test]
    fn app_info_reports_version_and_os() {
        let info = app_info_for("1.2.3");
        assert_eq!(info.version, "1.2.3");
        assert!(
            ["macos", "windows", "linux"].contains(&info.os.as_str()),
            "{}",
            info.os
        );
    }

    #[test]
    fn coded_serializes_as_code_and_flat_params() {
        let coded = Coded {
            code: "installer_old".into(),
            params: BTreeMap::from([
                ("days".into(), Param::Number(214.0)),
                ("appInstalled".into(), Param::Bool(true)),
            ]),
        };
        assert_eq!(
            serde_json::to_value(&coded).unwrap(),
            serde_json::json!({ "code": "installer_old", "params": { "appInstalled": true, "days": 214.0 } })
        );
    }

    /// Fails when a command or type changed without regenerating the TypeScript bindings.
    #[test]
    fn bindings_are_up_to_date() {
        if std::env::var_os("KIPPLE_UPDATE_BINDINGS").is_some() {
            export(std::path::Path::new(BINDINGS));
            return;
        }
        let committed = std::fs::read_to_string(BINDINGS).unwrap_or_default();
        assert!(
            committed == fresh_bindings(),
            "app/src/bindings/index.ts is stale; run: {REGENERATE}"
        );
    }

    /// `collect_commands!` and command_names.rs (which build.rs turns into permissions) must list
    /// the same commands, or a command compiles but is rejected by the ACL at runtime.
    #[test]
    fn exported_commands_match_the_permission_manifest() {
        let bindings = fresh_bindings();
        let mut exported: Vec<&str> = bindings
            .split("__TAURI_INVOKE")
            .skip(2) // the import line, then one segment per command call
            .filter_map(|call| call.split("(\"").nth(1)?.split('"').next())
            .collect();
        exported.sort_unstable();
        let mut declared = COMMAND_NAMES.to_vec();
        declared.sort_unstable();
        assert_eq!(exported, declared);
    }
}
