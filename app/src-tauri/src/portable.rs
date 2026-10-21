// SPDX-License-Identifier: GPL-3.0-or-later
//! **Portable mode**: when a `portable` file sits next to the executable (the Windows portable
//! zip ships one), all of Kipple's own files stay in a `KippleData` folder beside it instead of
//! the per-user app folders, so nothing is written to the user profile.
//!
//! It uses Tauri's `appDirectoriesOverride`, so every `app.path().app_*_dir()` (and the webview
//! profile, which lives in the local data dir) follows, including plugins: config, data and the
//! webview in `KippleData`, cache in `KippleData/caches`, logs in `KippleData/logs`.
//! Portable copies can't update themselves (there's no installer to run): the updater must
//! check [`is_portable`] and only link to the releases page.

use std::path::{Path, PathBuf};

use tauri::utils::config::AppDirectoriesOverride;
use tauri::{AppHandle, Context, Runtime};

/// The marker file next to the executable that turns on portable mode.
const MARKER: &str = "portable";
/// The folder next to the executable that holds everything in portable mode. A dedicated
/// subfolder, never the executable's own folder (see Tauri's `appDirectoriesOverride` notes).
const ROOT: &str = "KippleData";

/// How portable mode turned out for this run, for the startup log line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Installed,
    Portable,
    /// The marker is there, but `KippleData` can't be created (e.g. a write-protected stick):
    /// Kipple runs from the per-user folders instead of failing to start.
    Unwritable(String),
}

/// The portable folder for an executable at `exe`, if its `portable` marker file is there.
pub fn root(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    dir.join(MARKER).is_file().then(|| dir.join(ROOT))
}

/// Switches `context` to portable mode if the marker is next to the running executable. Call
/// before `Builder::run`, so every path Tauri resolves already follows it.
pub fn apply<R: Runtime>(context: &mut Context<R>) -> Mode {
    let Some(root) = std::env::current_exe().ok().and_then(|exe| root(&exe)) else {
        return Mode::Installed;
    };
    if let Err(e) = create(&root) {
        return Mode::Unwritable(e.to_string());
    }
    context.config_mut().app.app_directories_override = Some(AppDirectoriesOverride::Root(root));
    Mode::Portable
}

// Kipple's own folder, not a user file: ADR-0002's Executor rule covers managed locations.
#[allow(clippy::disallowed_methods)]
fn create(root: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(root)
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the updater (v1) checks it; nothing else needs it yet"
    )
)]
pub fn is_portable<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.config().app.app_directories_override.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn a_portable_file_next_to_the_exe_turns_on_portable_mode() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Kipple.exe");
        assert_eq!(root(&exe), None);
        std::fs::write(dir.path().join("portable"), "").unwrap();
        assert_eq!(root(&exe), Some(dir.path().join("KippleData")));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn only_a_file_named_portable_counts() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("Kipple.exe");
        std::fs::create_dir(dir.path().join("portable")).unwrap();
        assert_eq!(root(&exe), None, "a folder isn't the marker");
        std::fs::write(dir.path().join("portable.txt"), "").unwrap();
        assert_eq!(root(&exe), None);
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn a_symlinked_marker_counts_but_a_dangling_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("kipple");
        std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join("portable"))
            .unwrap();
        assert_eq!(root(&exe), None);
        std::fs::write(dir.path().join("missing"), "").unwrap();
        assert_eq!(root(&exe), Some(dir.path().join("KippleData")));
    }

    #[test]
    fn the_override_keeps_everything_inside_kipple_data() {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        let root = tempfile::tempdir().unwrap().path().join("KippleData");
        context.config_mut().app.app_directories_override =
            Some(AppDirectoriesOverride::Root(root.clone()));
        let app = tauri::test::mock_builder().build(context).unwrap();
        let paths = tauri::Manager::path(&app);
        for dir in [
            paths.app_config_dir().unwrap(),
            paths.app_data_dir().unwrap(),
            paths.app_local_data_dir().unwrap(),
            paths.app_cache_dir().unwrap(),
            paths.app_log_dir().unwrap(),
        ] {
            assert!(
                dir.starts_with(&root),
                "{} is outside {}",
                dir.display(),
                root.display()
            );
        }
        assert!(is_portable(app.handle()));
    }
}
