// SPDX-License-Identifier: GPL-3.0-or-later
//! Type categories (FUNCTIONAL_PLAN §2): every Item has exactly one. Mostly decided by the file
//! extension; Screenshots by the platform's screenshot flag, since screenshot names are
//! localised and their extension is just `.png`.

use crate::item::ItemKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TypeCategory {
    Images,
    Screenshots,
    Videos,
    Audio,
    Documents,
    Spreadsheets,
    Presentations,
    Pdfs,
    Archives,
    Installers,
    Code,
    Fonts,
    Ebooks,
    Other,
}

impl TypeCategory {
    pub const ALL: [TypeCategory; 14] = [
        Self::Images,
        Self::Screenshots,
        Self::Videos,
        Self::Audio,
        Self::Documents,
        Self::Spreadsheets,
        Self::Presentations,
        Self::Pdfs,
        Self::Archives,
        Self::Installers,
        Self::Code,
        Self::Fonts,
        Self::Ebooks,
        Self::Other,
    ];

    /// The stable code stored in the index and sent to the UI, which shows the translated name
    /// (ADR-0004). Never change an existing code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Images => "images",
            Self::Screenshots => "screenshots",
            Self::Videos => "videos",
            Self::Audio => "audio",
            Self::Documents => "documents",
            Self::Spreadsheets => "spreadsheets",
            Self::Presentations => "presentations",
            Self::Pdfs => "pdfs",
            Self::Archives => "archives",
            Self::Installers => "installers",
            Self::Code => "code",
            Self::Fonts => "fonts",
            Self::Ebooks => "ebooks",
            Self::Other => "other",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.code() == code)
    }

    /// The category for a file extension (without the dot, any case). Unknown → `Other`.
    pub fn from_extension(ext: &str) -> Self {
        Self::from_lowercase_extension(&ext.to_lowercase())
    }

    fn from_lowercase_extension(ext: &str) -> Self {
        match ext {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "heif" | "avif" | "jxl" | "bmp"
            | "tif" | "tiff" | "svg" | "ico" | "psd" | "eps" | "jfif" | "raw" | "dng" | "cr2"
            | "cr3" | "nef" | "arw" | "orf" | "rw2" | "raf" | "photoslibrary" => Self::Images,
            "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "flv" | "mpg" | "mpeg"
            | "3gp" | "mts" | "m2ts" | "fcpbundle" | "imovielibrary" => Self::Videos,
            "mp3" | "m4a" | "m4b" | "aac" | "wav" | "flac" | "ogg" | "oga" | "opus" | "wma"
            | "aif" | "aiff" | "mid" | "midi" | "logicx" | "band" => Self::Audio,
            "doc" | "docx" | "docm" | "odt" | "rtf" | "txt" | "md" | "pages" | "tex" => {
                Self::Documents
            }
            "xls" | "xlsx" | "xlsm" | "ods" | "csv" | "tsv" | "numbers" => Self::Spreadsheets,
            "ppt" | "pptx" | "ppsx" | "odp" | "key" => Self::Presentations,
            "pdf" => Self::Pdfs,
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "lz4" | "cab"
            | "iso" => Self::Archives,
            "dmg" | "pkg" | "mpkg" | "exe" | "msi" | "msix" | "msixbundle" | "appx" | "deb"
            | "rpm" | "appimage" | "apk" | "snap" | "flatpakref" => Self::Installers,
            "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "py" | "rb" | "go" | "java" | "kt"
            | "c" | "h" | "cpp" | "hpp" | "cs" | "swift" | "php" | "sh" | "ps1" | "html"
            | "css" | "scss" | "json" | "yaml" | "yml" | "toml" | "xml" | "sql" | "ipynb"
            | "lua" | "dart" | "vue" | "svelte" => Self::Code,
            "ttf" | "otf" | "ttc" | "woff" | "woff2" => Self::Fonts,
            "epub" | "mobi" | "azw" | "azw3" | "fb2" | "djvu" | "cbz" | "cbr" => Self::Ebooks,
            _ => Self::Other,
        }
    }

    /// The category of an Item. Folders and symlinks are `Other`; bundles count by their
    /// extension (`.photoslibrary` is Images). An image the platform flags as a screenshot is
    /// `Screenshots`.
    pub fn of(kind: ItemKind, name: &str, is_screenshot: bool) -> Self {
        let category = match kind {
            ItemKind::Dir | ItemKind::Symlink => return Self::Other,
            ItemKind::File | ItemKind::Bundle => {
                extension_of(name).map_or(Self::Other, |ext| Self::from_lowercase_extension(&ext))
            }
        };
        if is_screenshot && category == Self::Images {
            Self::Screenshots
        } else {
            category
        }
    }
}

/// The extension of a file name, lowercased, without the dot: `"Photo.JPG"` → `"jpg"`,
/// `"backup.tar.gz"` → `"gz"`. Hidden files without one (`".bashrc"`) and names ending in a dot
/// have none.
pub fn extension_of(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::TypeCategory::*;
    use super::*;

    #[test]
    fn extensions() {
        let cases = [
            ("Photo.JPG", Some("jpg")),
            ("backup.tar.gz", Some("gz")),
            ("README", None),
            (".bashrc", None),
            (".config.json", Some("json")),
            ("odd.", None),
            ("Setup 2.0.exe", Some("exe")),
            ("café.Pages", Some("pages")),
        ];
        for (name, ext) in cases {
            assert_eq!(extension_of(name).as_deref(), ext, "{name}");
        }
    }

    #[test]
    fn categories_by_extension() {
        let cases = [
            (
                Images,
                &[
                    "jpg", "JPEG", "png", "heic", "webp", "gif", "svg", "dng", "cr3", "jfif",
                    "eps", "psd", "avif",
                ][..],
            ),
            (Videos, &["mp4", "MOV", "mkv", "webm", "avi", "m4v"]),
            (
                Audio,
                &["mp3", "m4a", "m4b", "flac", "wav", "ogg", "opus", "aiff"],
            ),
            (
                Documents,
                &["docx", "docm", "doc", "odt", "rtf", "txt", "md", "pages"],
            ),
            (
                Spreadsheets,
                &["xlsx", "xlsm", "xls", "ods", "csv", "numbers"],
            ),
            (Presentations, &["pptx", "ppsx", "ppt", "odp", "key"]),
            (Pdfs, &["pdf", "PDF"]),
            (
                Archives,
                &["zip", "rar", "7z", "gz", "tgz", "xz", "zst", "iso"],
            ),
            (
                Installers,
                &[
                    "dmg",
                    "pkg",
                    "exe",
                    "msi",
                    "msix",
                    "msixbundle",
                    "deb",
                    "rpm",
                    "appimage",
                    "AppImage",
                ],
            ),
            (
                Code,
                &[
                    "rs", "ts", "py", "js", "go", "java", "c", "cpp", "sh", "json", "ipynb",
                ],
            ),
            (Fonts, &["ttf", "otf", "woff2"]),
            (Ebooks, &["epub", "mobi", "azw3", "cbz"]),
            (Other, &["", "xyz", "bak", "tmp", "lnk"]),
        ];
        for (category, extensions) in cases {
            for ext in extensions {
                assert_eq!(TypeCategory::from_extension(ext), category, "{ext:?}");
            }
        }
    }

    #[test]
    fn categories_by_item() {
        let cases = [
            (ItemKind::File, "Holiday.HEIC", false, Images),
            (
                ItemKind::File,
                "Screenshot 2026-10-21 at 09.12.png",
                true,
                Screenshots,
            ),
            (
                ItemKind::File,
                "Schermafbeelding 2026-10-21 om 09.12.png",
                true,
                Screenshots,
            ),
            (ItemKind::File, "screen recording.mov", true, Videos), // only images become Screenshots
            (ItemKind::File, "notes", false, Other),
            (ItemKind::Dir, "Photos.jpg", false, Other), // a folder, whatever its name
            (ItemKind::Symlink, "latest.pdf", false, Other),
            (
                ItemKind::Bundle,
                "Photos Library.photoslibrary",
                false,
                Images,
            ),
            (ItemKind::Bundle, "Kipple.app", false, Other),
            (ItemKind::Bundle, "Talk.key", false, Presentations),
        ];
        for (kind, name, screenshot, category) in cases {
            assert_eq!(
                TypeCategory::of(kind, name, screenshot),
                category,
                "{kind:?} {name}"
            );
        }
    }

    #[test]
    fn codes_are_unique_and_round_trip() {
        let codes: std::collections::BTreeSet<_> =
            TypeCategory::ALL.iter().map(|c| c.code()).collect();
        assert_eq!(codes.len(), TypeCategory::ALL.len());
        for category in TypeCategory::ALL {
            assert_eq!(TypeCategory::from_code(category.code()), Some(category));
            assert!(
                category.code().chars().all(|c| c.is_ascii_lowercase()),
                "{}",
                category.code()
            );
        }
        assert_eq!(TypeCategory::from_code("nope"), None);
    }
}
