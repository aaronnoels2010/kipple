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

/// Folders in the home folder holding keys, credentials and app settings: never touched, on any
/// OS (an "unused for a year" rule must never reach a private key).
const HOME_PRIVATE: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube",
    ".docker",
    ".password-store",
    ".pki",
    ".config",
    ".local/share",
];

/// Folders the OS keeps on every volume (seen when a Location is a whole drive), and Trash
/// folders: protected wherever they appear.
const SYSTEM_FOLDER_NAMES: &[&str] = &[
    ".Spotlight-V100",
    ".Trashes",
    ".fseventsd",
    ".DocumentRevisions-V100",
    ".TemporaryItems",
    ".Trash",
    "$Recycle.Bin",
    "System Volume Information",
    "lost+found",
];

#[derive(Debug, Clone)]
pub struct ProtectedPaths {
    os: Os,
    roots: Vec<PathBuf>,
    /// Inside a protected root but meant for the user's files (cloud folders in ~/Library).
    exceptions: Vec<PathBuf>,
    home: PathBuf,
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
                "/etc",
                "/var",
                "/tmp",
                "/dev",
                "/cores",
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
            .collect(),
        };
        roots.extend(HOME_PRIVATE.iter().map(|dir| home.join(dir)));
        roots.extend(kipple_dirs.iter().cloned());
        // iCloud Drive and File Provider folders (OneDrive, Dropbox…) live in ~/Library.
        let exceptions = match os {
            Os::MacOs => vec![
                home.join("Library/CloudStorage"),
                home.join("Library/Mobile Documents/com~apple~CloudDocs"),
            ],
            Os::Windows | Os::Linux => Vec::new(),
        };
        // macOS and Windows volumes are case-insensitive by default; on a case-sensitive one this
        // only protects a little more than needed, never less.
        Self {
            os,
            roots,
            exceptions,
            home: home.to_path_buf(),
            case_sensitive: os == Os::Linux,
        }
    }

    /// Whether Kipple must never change `path`: it's in (or is) a protected folder, a system or
    /// Trash folder, or it lies inside a bundle or a repository's internals. A relative path or
    /// one with `..` is always protected: it could point anywhere.
    pub fn is_protected(&self, path: &Path) -> bool {
        if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
            return true;
        }
        let within = |dir: &PathBuf| is_within(dir, path, self.case_sensitive);
        if !self.exceptions.iter().any(within) && self.roots.iter().any(within) {
            return true;
        }
        let names: Vec<&str> = path
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect();
        if names.iter().any(|n| self.is_system_folder(n)) {
            return true;
        }
        // Windows keeps the user's registry hive (NTUSER.DAT and its logs) in the home folder.
        if self.os == Os::Windows
            && path.parent().is_some_and(|dir| {
                is_within(dir, &self.home, false) && is_within(&self.home, dir, false)
            })
            && names
                .last()
                .is_some_and(|n| n.to_ascii_lowercase().starts_with("ntuser.dat"))
        {
            return true;
        }
        // Every component but the last is a folder: one that's opaque means `path` is inside it.
        names
            .split_last()
            .is_some_and(|(_, folders)| folders.iter().any(|n| self.is_opaque_dir(n)))
    }

    fn is_system_folder(&self, name: &str) -> bool {
        SYSTEM_FOLDER_NAMES.iter().any(|s| self.same(s, name)) || name.starts_with(".Trash-")
    }

    fn same(&self, a: &str, b: &str) -> bool {
        if self.case_sensitive {
            a == b
        } else {
            a.eq_ignore_ascii_case(b)
        }
    }

    /// Whether a folder named `name` is a bundle (macOS) or repository internals (any OS):
    /// one opaque Item.
    pub fn is_opaque_dir(&self, name: &str) -> bool {
        REPOSITORY_DIRS.iter().any(|dir| self.same(dir, name))
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
            // Review of K-2.3: system folders, secrets, per-volume system folders, cloud folders.
            ("/etc/hosts", true), ("/tmp/x", true), ("/var/log/x", true), ("/dev/disk0", true), ("/cores/x", true),
            ("/Users/zoe/.ssh/id_ed25519", true), ("/Users/zoe/.gnupg/x", true), ("/Users/zoe/.aws/credentials", true),
            ("/Users/zoe/.Trash/old.pdf", true),
            ("/Volumes/USB/.Spotlight-V100/x", true), ("/Volumes/USB/.Trashes/501/x", true),
            ("/Volumes/USB/.fseventsd/x", true), ("/Volumes/USB/.DocumentRevisions-V100/x", true),
            ("/Users/zoe/Library/CloudStorage/OneDrive-Personal/Docs/a.pdf", false),
            ("/Users/zoe/Library/Mobile Documents/com~apple~CloudDocs/a.pdf", false),
            ("/Users/zoe/Library/Mobile Documents/com~apple~Pages/x", true),
            ("relative/path", true),
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
            (r"C:\$Recycle.Bin\S-1-5\x", true), (r"D:\System Volume Information\x", true),
            (r"C:\Users\Zoe\NTUSER.DAT", true), (r"C:\Users\Zoe\ntuser.dat.LOG1", true),
            (r"C:\Users\Zoe\.ssh\id_rsa", true),
            (r"\\?\C:\Windows\System32\x.dll", true),           // verbatim form of C:\Windows
            (r"Users\Zoe\x", true), (r"\Users\Zoe\x", true),     // relative / drive-relative
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
            ("/home/zoe/.gnupg/pubring.kbx", true), ("/home/zoe/.ssh/id_ed25519", true),
            ("/media/usb/lost+found/x", true), ("/media/usb/.Trash-1000/files/x", true),
            ("home/zoe/x", true),                                     // relative: could be anywhere
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
