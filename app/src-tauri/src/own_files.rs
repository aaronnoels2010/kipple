// SPDX-License-Identifier: GPL-3.0-or-later
//! Kipple's own files (settings, locations, the log key): written atomically. They're not user
//! files, so ADR-0002's rule (only the Executor changes files) is about managed locations, not
//! these; every file-changing call for them lives here.

use std::io::{self, Write};
use std::path::Path;

/// Who may read a file written by [`write_atomic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Normal,
    /// Only the user (mode 0600 on Unix): secrets like the log redaction key.
    Private,
}

/// Writes `bytes` to `path` via a synced temporary file and a rename, creating the folder if
/// needed: a crash or power loss leaves either the old file or the new one, never a torn one.
#[allow(clippy::disallowed_methods)]
pub fn write_atomic(path: &Path, bytes: &[u8], access: Access) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if access == Access::Private {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    }
    #[cfg(not(unix))]
    let _ = access;
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?; // the rename must never expose an empty file after a power loss
    drop(file);
    std::fs::rename(&tmp, path)
}

/// Renames a file Kipple can't read to `<name>.bad`, so the next save can't destroy what the
/// user (or a newer Kipple) wrote. Best effort: if even that fails, the file stays where it is.
#[allow(clippy::disallowed_methods)]
pub fn set_aside(path: &Path) {
    let mut bad = path.file_name().unwrap_or_default().to_os_string();
    bad.push(".bad");
    let _ = std::fs::rename(path, path.with_file_name(bad));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_replace_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("a.json");
        write_atomic(&path, b"long first version", Access::Normal).unwrap();
        write_atomic(&path, b"short", Access::Normal).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"short");
        assert!(!dir.path().join("sub/a.json.tmp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key");
        write_atomic(&path, b"k", Access::Private).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn set_aside_keeps_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        write_atomic(&path, b"{oops", Access::Normal).unwrap();
        set_aside(&path);
        assert!(!path.exists());
        assert_eq!(
            std::fs::read(dir.path().join("settings.json.bad")).unwrap(),
            b"{oops"
        );
    }
}
