// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux screenshots by name and folder: what GNOME (Shell and gnome-screenshot) and KDE
//! Spectacle call them in each language. Plain text rules, so the tests run on every OS.

use std::path::{Path, PathBuf};

/// What one language calls screenshots, from the GNOME Shell, gnome-screenshot and Spectacle
/// translations. Name prefixes are lower case, with `'` standing for `’` too.
struct Names {
    /// GNOME: `<prefix> 2026-10-21 10-20-30.png`. Shell adds `-1` and gnome-screenshot ` - 1`
    /// when the name is taken.
    gnome: &'static [&'static str],
    /// Spectacle's default template: `<prefix>_20261021_102030.png`.
    spectacle: &'static str,
    /// The folders in Pictures that GNOME 42+ and Spectacle save to, as spelled on disk.
    folders: &'static [&'static str],
}

/// One row per language: adding a language is adding a row.
const LANGUAGES: &[Names] = &[
    // English: GNOME Shell 47+ says "Screenshot From", older versions "Screenshot from".
    Names {
        gnome: &["screenshot from"],
        spectacle: "screenshot",
        folders: &["Screenshots"],
    },
    // Dutch
    Names {
        gnome: &["schermafdruk van"],
        spectacle: "schermafdruk",
        folders: &["Schermafdrukken"],
    },
    // German
    Names {
        gnome: &["bildschirmfoto vom"],
        spectacle: "bildschirmfoto",
        folders: &["Bildschirmfotos"],
    },
    // French: GNOME writes `’`, Spectacle `'`.
    Names {
        gnome: &["capture d'écran du"],
        spectacle: "capture d'écran",
        folders: &["Captures d’écran", "Captures d'écran"],
    },
    // Spanish: GNOME Shell and gnome-screenshot translate it differently.
    Names {
        gnome: &["captura desde", "captura de pantalla de"],
        spectacle: "captura de pantalla",
        folders: &["Capturas de pantalla"],
    },
    // Italian
    Names {
        gnome: &["schermata del"],
        spectacle: "schermata",
        folders: &["Schermate"],
    },
    // Portuguese (Portugal)
    Names {
        gnome: &["captura de ecrã de"],
        spectacle: "captura de ecrã",
        folders: &["Capturas de ecrã"],
    },
    // Portuguese (Brazil)
    Names {
        gnome: &["captura de tela de"],
        spectacle: "captura_de_tela",
        folders: &["Capturas de tela"],
    },
];

/// Whether `text` starts with `shape`, in which `9` stands for a digit and `?` for `-` or `:`.
fn starts_with_shape(text: &str, shape: &str) -> bool {
    text.len() >= shape.len()
        && text.bytes().zip(shape.bytes()).all(|(c, s)| match s {
            b'9' => c.is_ascii_digit(),
            b'?' => c == b'-' || c == b':',
            _ => c == s,
        })
}

/// Whether a file name is what GNOME or Spectacle names a screenshot, in one of [`LANGUAGES`].
pub(crate) fn is_screenshot_name(name: &str) -> bool {
    let name = name.to_lowercase().replace('’', "'");
    LANGUAGES.iter().any(|language| {
        // gnome-screenshot before 3.x wrote `10:20:30`.
        let gnome = language.gnome.iter().any(|prefix| {
            name.strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix(' '))
                .is_some_and(|rest| starts_with_shape(rest, "9999-99-99 99?99?99"))
        });
        gnome
            || name
                .strip_prefix(language.spectacle)
                .and_then(|rest| rest.strip_prefix('_'))
                .is_some_and(|rest| starts_with_shape(rest, "99999999_999999"))
    })
}

/// `XDG_PICTURES_DIR` from a `user-dirs.dirs` file (`XDG_PICTURES_DIR="$HOME/Afbeeldingen"`).
/// As in the shell that file is written for, the last assignment wins.
pub(crate) fn pictures_dir(user_dirs: &str, home: &Path) -> Option<PathBuf> {
    let value = user_dirs
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))?;
    let quoted = value.trim().strip_prefix('"')?.strip_suffix('"')?;
    let mut value = String::with_capacity(quoted.len());
    let mut chars = quoted.chars();
    while let Some(c) = chars.next() {
        value.push(if c == '\\' { chars.next()? } else { c });
    }
    // Only `$HOME/…` and absolute paths are allowed.
    if value == "$HOME" {
        Some(home.to_path_buf())
    } else if let Some(relative) = value.strip_prefix("$HOME/") {
        Some(home.join(relative))
    } else {
        Some(PathBuf::from(value)).filter(|path| path.is_absolute())
    }
}

/// The folders GNOME and Spectacle save screenshots to, in every language, under the Pictures
/// folder (`~/Pictures` when `user-dirs.dirs` names none) and under `~/Pictures` itself.
pub(crate) fn screenshot_folders(home: &Path, pictures: Option<&Path>) -> Vec<PathBuf> {
    let default = home.join("Pictures");
    let mut parents = vec![pictures.unwrap_or(&default).to_path_buf()];
    if parents[0] != default {
        parents.push(default);
    }
    parents
        .iter()
        .flat_map(|parent| {
            LANGUAGES
                .iter()
                .flat_map(|language| language.folders)
                .map(|folder| parent.join(folder))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_and_spectacle_names_are_screenshots_in_every_language() {
        for name in [
            "Screenshot From 2026-10-21 10-20-30.png",
            "Screenshot from 2026-10-21 10-20-30.png",
            "Screenshot from 2026-10-21 10-20-30-1.png",
            "Screenshot from 2026-10-21 10-20-30 - 2.png",
            "Screenshot from 2013-01-31 10:20:30.png",
            "Schermafdruk van 2026-10-21 10-20-30.png",
            "Bildschirmfoto vom 2026-10-21 10-20-30.png",
            "Capture d’écran du 2026-10-21 10-20-30.png",
            "Capture d'écran du 2026-10-21 10-20-30.png",
            "Captura desde 2026-10-21 10-20-30.png",
            "Captura de pantalla de 2026-10-21 10-20-30.png",
            "Schermata del 2026-10-21 10-20-30.png",
            "Captura de ecrã de 2026-10-21 10-20-30.png",
            "Captura de tela de 2026-10-21 10-20-30.png",
            "Screenshot_20261021_102030.png",
            "Screenshot_20261021_102030-1.jpg",
            "Schermafdruk_20261021_102030.png",
            "Bildschirmfoto_20261021_102030.png",
            "Capture d'écran_20261021_102030.png",
            "Captura de pantalla_20261021_102030.png",
            "Schermata_20261021_102030.png",
            "Captura de ecrã_20261021_102030.png",
            "Captura_de_tela_20261021_102030.png",
        ] {
            assert!(is_screenshot_name(name), "{name}");
        }
    }

    #[test]
    fn other_names_are_not_screenshots() {
        for name in [
            "holiday.png",
            "Screenshot.png",
            "Screenshot from the meeting.png",
            "Screenshot from 2026-10-21.png",
            "Screenshot_2026.png",
            "My Screenshot from 2026-10-21 10-20-30.png",
            "Screenshot from 2026-1x-21 10-20-30.png",
            "Schermafbeelding 2026-10-21 om 10.20.30.png",
        ] {
            assert!(!is_screenshot_name(name), "{name}");
        }
    }

    #[test]
    fn the_pictures_folder_comes_from_user_dirs() {
        let home = Path::new("/home/zoe");
        let text = "\
# This file is written by xdg-user-dirs-update
XDG_DESKTOP_DIR=\"$HOME/Bureaublad\"
XDG_PICTURES_DIR=\"$HOME/Afbeeldingen\"
";
        assert_eq!(
            pictures_dir(text, home),
            Some(PathBuf::from("/home/zoe/Afbeeldingen"))
        );
        if cfg!(unix) {
            // Not absolute on Windows, which has no user-dirs.dirs anyway.
            assert_eq!(
                pictures_dir("XDG_PICTURES_DIR=\"/data/Fotos\"\n", home),
                Some(PathBuf::from("/data/Fotos"))
            );
        }
        assert_eq!(
            pictures_dir(
                "XDG_PICTURES_DIR=\"$HOME/a\"\nXDG_PICTURES_DIR=\"$HOME/My \\\"b\\\"\"\n",
                home
            ),
            Some(PathBuf::from("/home/zoe/My \"b\"")),
            "the last one wins; escapes are undone"
        );
        assert_eq!(
            pictures_dir("XDG_PICTURES_DIR=\"$HOME\"\n", home),
            Some(home.into())
        );
        assert_eq!(pictures_dir("XDG_PICTURES_DIR=\"Pictures\"\n", home), None);
        assert_eq!(pictures_dir("# XDG_PICTURES_DIR=\"$HOME/x\"\n", home), None);
        assert_eq!(pictures_dir("", home), None);
    }

    #[test]
    fn screenshot_folders_in_the_pictures_folder_and_in_the_default_one() {
        let home = Path::new("/home/zoe");
        let folders = screenshot_folders(home, Some(Path::new("/home/zoe/Afbeeldingen")));
        assert!(folders.contains(&PathBuf::from("/home/zoe/Afbeeldingen/Schermafdrukken")));
        assert!(folders.contains(&PathBuf::from("/home/zoe/Afbeeldingen/Screenshots")));
        assert!(folders.contains(&PathBuf::from("/home/zoe/Pictures/Screenshots")));
        assert_eq!(
            screenshot_folders(home, None),
            screenshot_folders(home, Some(Path::new("/home/zoe/Pictures"))),
            "the default folder is listed once"
        );
    }
}
