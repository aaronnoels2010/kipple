// SPDX-License-Identifier: GPL-3.0-or-later
//! Paths Kipple never changes (CONTEXT.md, "Protected path"), and bundles: folders the OS shows
//! as one file, which Kipple treats as one opaque Item and never descends into.

use std::path::{Component, Path, PathBuf};

use kipple_core::category::extension_of;
use kipple_core::paths::is_within;

use crate::Os;

/// Folder extensions that make a folder a bundle on macOS.
const MACOS_BUNDLES: &[&str] = &[
    "app",
    "bundle",
    "framework",
    "plugin",
    "kext",
    "appex",
    "xpc",
    "photoslibrary",
    "photolibrary",
    "fcpbundle",
    "imovielibrary",
    "tvlibrary",
    "musiclibrary",
    "logicx",
    "band",
    "rtfd",
    "pages",
    "numbers",
    "key",
    "xcodeproj",
    "xcworkspace",
    "playground",
    "pkg",
    "mpkg",
    "scptd",
    "lpdf",
];

/// Folders with version-control internals: opaque on every OS, so a repository is never
/// half cleaned up.
const REPOSITORY_DIRS: &[&str] = &[".git", ".hg", ".svn"];

#[derive(Debug, Clone)]
pub struct ProtectedPaths {
    os: Os,
    roots: Vec<PathBuf>,
    case_sensitive: bool,
}

impl ProtectedPaths {
    /// The rules for `os` (ARCHITECTURE §8), for a user whose home folder is `home`, with system
    /// folders on `system_drive` (Windows, e.g. `C:\`), plus Kipple's own folders.
    pub fn for_os(os: Os, home: &Path, system_drive: &Path, kipple_dirs: &[PathBuf]) -> Self {
        let mut roots: Vec<PathBuf> = match os {
            Os::MacOs => [
                "/System",
                "/Library",
                "/usr",
                "/bin",
                "/sbin",
                "/private",
                "/opt/homebrew",
                "/Applications",
            ]
            .into_iter()
            .map(PathBuf::from)
            .chain([home.join("Library")])
            .collect(),
            Os::Windows => [
                "Windows",
                "Program Files",
                "Program Files (x86)",
                "ProgramData",
            ]
            .into_iter()
            .map(|dir| system_drive.join(dir))
            .chain([home.join("AppData")])
            .collect(),
            Os::Linux => [
                "/usr", "/etc", "/proc", "/sys", "/boot", "/dev", "/run", "/bin", "/sbin", "/lib",
                "/lib64", "/var", "/snap",
            ]
            .into_iter()
            .map(PathBuf::from)
            .chain([home.join(".config"), home.join(".local/share")])
            .collect(),
        };
        roots.extend(kipple_dirs.iter().cloned());
        // macOS and Windows volumes are case-insensitive by default; on a case-sensitive one this
        // only protects a little more than needed, never less.
        Self {
            os,
            roots,
            case_sensitive: os == Os::Linux,
        }
    }

    /// Whether Kipple must never change `path`: it's in (or is) a protected folder, or it lies
    /// inside a bundle or a repository's internals. A path with `..` is always protected: it
    /// could point anywhere.
    pub fn is_protected(&self, path: &Path) -> bool {
        if path.components().any(|c| c == Component::ParentDir) {
            return true;
        }
        if self
            .roots
            .iter()
            .any(|root| is_within(root, path, self.case_sensitive))
        {
            return true;
        }
        // Every component but the last is a folder: one that's opaque means `path` is inside it.
        let mut folders = path.components().collect::<Vec<_>>();
        folders.pop();
        folders.iter().any(|c| matches!(c, Component::Normal(name) if name.to_str().is_some_and(|n| self.is_opaque_dir(n))))
    }

    /// Whether a folder named `name` is a bundle (macOS) or repository internals (any OS):
    /// one opaque Item.
    pub fn is_opaque_dir(&self, name: &str) -> bool {
        let same = |a: &str, b: &str| {
            if self.case_sensitive {
                a == b
            } else {
                a.eq_ignore_ascii_case(b)
            }
        };
        REPOSITORY_DIRS.iter().any(|dir| same(dir, name))
            || (self.os == Os::MacOs
                && extension_of(name).is_some_and(|ext| MACOS_BUNDLES.contains(&ext.as_str())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macos() -> ProtectedPaths {
        ProtectedPaths::for_os(
            Os::MacOs,
            Path::new("/Users/zoe"),
            Path::new("/"),
            &["/Users/zoe/Library/Application Support/app.kipple.desktop".into()],
        )
    }
    fn windows() -> ProtectedPaths {
        ProtectedPaths::for_os(
            Os::Windows,
            Path::new(r"C:\Users\Zoe"),
            Path::new(r"C:\"),
            &[],
        )
    }
    fn linux() -> ProtectedPaths {
        ProtectedPaths::for_os(
            Os::Linux,
            Path::new("/home/zoe"),
            Path::new("/"),
            &["/home/zoe/.local/share/app.kipple.desktop".into()],
        )
    }

    #[cfg(not(windows))]
    #[test]
    fn macos_rules() {
        let p = macos();
        #[rustfmt::skip]
        let cases = [
            ("/System/Library/x", true), ("/Library/Fonts/a.ttf", true), ("/usr/local/bin/x", true),
            ("/private/var/tmp/x", true), ("/opt/homebrew/bin/x", true), ("/Applications/Zoom.app", true),
            ("/Users/zoe/Library/Caches/x", true), ("/users/zoe/library/x", true),
            ("/Users/zoe/Downloads/Zoom.dmg", false), ("/Users/zoe/Desktop", false),
            ("/Users/zoe/Documents/Talk.key", false),               // the bundle itself: one Item
            ("/Users/zoe/Documents/Talk.key/Data/x.png", true),     // inside it: never touched
            ("/Users/zoe/Code/kipple/.git/HEAD", true),
            ("/Users/zoe/Code/kipple/src/main.rs", false),
            ("/Volumes/USB/Photos/a.jpg", false),
        ];
        for (path, protected) in cases {
            assert_eq!(p.is_protected(Path::new(path)), protected, "{path}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_rules() {
        let p = windows();
        #[rustfmt::skip]
        let cases = [
            (r"C:\Windows\System32\x.dll", true), (r"C:\Program Files\App\a.exe", true),
            (r"C:\Program Files (x86)\App\a.exe", true), (r"C:\ProgramData\x", true),
            (r"C:\Users\Zoe\AppData\Local\x", true), (r"c:\users\zoe\appdata\x", true),
            (r"C:\Users\Zoe\Downloads\setup.exe", false), (r"D:\Photos\a.jpg", false),
            (r"C:\Users\Zoe\Code\kipple\.git\HEAD", true),
            (r"C:\Users\Zoe\Documents\Talk.key\x", false),          // not a bundle on Windows
        ];
        for (path, protected) in cases {
            assert_eq!(p.is_protected(Path::new(path)), protected, "{path}");
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn linux_rules() {
        let p = linux();
        #[rustfmt::skip]
        let cases = [
            ("/usr/bin/x", true), ("/etc/hosts", true), ("/proc/1/maps", true), ("/sys/x", true),
            ("/boot/x", true), ("/home/zoe/.config/x", true), ("/home/zoe/.local/share/x", true),
            ("/home/zoe/.local/share/app.kipple.desktop/index.db", true),
            ("/home/zoe/Downloads/a.deb", false), ("/home/zoe/Downloads/A.deb", false),
            ("/home/zoe/.local/bin/tool", false),
            ("/home/zoe/repo/.git/config", true), ("/media/usb/a.jpg", false),
            ("/home/zoe/x/../../../etc/passwd", true),               // `..`: never treated as safe
        ];
        for (path, protected) in cases {
            assert_eq!(p.is_protected(Path::new(path)), protected, "{path}");
        }
    }

    #[test]
    fn opaque_dirs() {
        assert!(macos().is_opaque_dir("Zoom.app"));
        assert!(macos().is_opaque_dir("Photos Library.photoslibrary"));
        assert!(macos().is_opaque_dir("Talk.KEY"));
        assert!(macos().is_opaque_dir(".git"));
        assert!(!macos().is_opaque_dir("Photos"));
        assert!(!macos().is_opaque_dir(".gitignore"));
        assert!(linux().is_opaque_dir(".git"));
        assert!(!linux().is_opaque_dir("Zoom.app"), "only macOS has bundles");
        assert!(!windows().is_opaque_dir("Talk.key"));
    }
}
