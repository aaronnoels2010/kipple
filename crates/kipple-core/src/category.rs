// SPDX-License-Identifier: GPL-3.0-or-later
//! Type categories (FUNCTIONAL_PLAN §2): every Item has exactly one. Mostly decided by the file
//! extension; by the content's magic bytes where the extension is missing, unknown or ambiguous
//! (ARCHITECTURE §7.1 step 4); Screenshots by the platform's screenshot flag, since screenshot
//! names are localised and their extension is just `.png`.

use crate::item::ItemKind;

/// What sniffing a file's first bytes (`infer`) found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sniffed<'a> {
    /// The content wasn't read: a placeholder (reading would download it), or it couldn't be.
    NotRead,
    /// Read, but no type was recognised (most text files).
    Unknown,
    /// The MIME type it was recognised as, e.g. `"image/png"`.
    Mime(&'a str),
}

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

    /// The category for a sniffed MIME type (as `infer` names them), or `None` when it says
    /// nothing useful: a program isn't an installer, and a certificate or a database is `Other`
    /// whatever its extension claims.
    pub fn from_mime(mime: &str) -> Option<Self> {
        let category = match mime {
            "image/vnd.djvu" => Self::Ebooks,
            "image/vnd.dwg" => return None, // a CAD drawing
            _ if mime.starts_with("image/") => Self::Images,
            _ if mime.starts_with("video/") => Self::Videos,
            _ if mime.starts_with("audio/") => Self::Audio,
            _ if mime.starts_with("font/") => Self::Fonts,
            "application/font-sfnt" | "application/font-woff" => Self::Fonts,
            "application/pdf" => Self::Pdfs,
            "application/msword"
            | "application/rtf"
            | "application/vnd.oasis.opendocument.text"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                Self::Documents
            }
            "application/vnd.ms-excel"
            | "application/vnd.oasis.opendocument.spreadsheet"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
                Self::Spreadsheets
            }
            "application/vnd.ms-powerpoint"
            | "application/vnd.oasis.opendocument.presentation"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                Self::Presentations
            }
            "application/zip"
            | "application/gzip"
            | "application/x-tar"
            | "application/vnd.rar"
            | "application/x-7z-compressed"
            | "application/x-bzip2"
            | "application/vnd.bzip3"
            | "application/x-xz"
            | "application/zstd"
            | "application/x-lz4"
            | "application/x-lzip"
            | "application/x-compress"
            | "application/x-cpio"
            | "application/x-unix-archive"
            | "application/vnd.ms-cab-compressed" => Self::Archives,
            // A Windows program or library (`.dll`, `.sys`) looks the same: only `.exe` and `.msi`
            // say installer, and those are known extensions that are never sniffed.
            "application/vnd.microsoft.portable-executable" => return None,
            "application/x-apple-diskimage"
            | "application/vnd.debian.binary-package"
            | "application/x-rpm" => Self::Installers,
            "application/epub+zip" | "application/x-mobipocket-ebook" => Self::Ebooks,
            "text/html" | "text/xml" | "text/x-shellscript" => Self::Code,
            _ => return None,
        };
        Some(category)
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
        category.screenshot_if(is_screenshot)
    }

    /// Like [`TypeCategory::of`], but where the name can't tell ([`needs_sniffing`]) a
    /// recognised sniffed type wins over the extension. A `.key` file is a Keynote presentation
    /// when it's a zip, and otherwise (a key file) `Other`; a `.ts` file that isn't a transport
    /// stream is TypeScript. Content that wasn't read keeps the extension's guess.
    pub fn classify(kind: ItemKind, name: &str, sniffed: Sniffed, is_screenshot: bool) -> Self {
        let by_name = Self::of(kind, name, is_screenshot);
        if !needs_sniffing(kind, name) {
            return by_name;
        }
        let key_file = extension_of(name).as_deref() == Some("key");
        let category = match sniffed {
            Sniffed::NotRead => return by_name,
            Sniffed::Mime("application/zip") if key_file => Self::Presentations,
            Sniffed::Mime(mime) => match Self::from_mime(mime) {
                Some(category) => category,
                None if key_file => Self::Other,
                None => by_name,
            },
            Sniffed::Unknown if key_file => Self::Other,
            Sniffed::Unknown => by_name,
        };
        category.screenshot_if(is_screenshot)
    }

    fn screenshot_if(self, is_screenshot: bool) -> Self {
        if is_screenshot && self == Self::Images {
            Self::Screenshots
        } else {
            self
        }
    }
}

/// Whether a file's category needs its content sniffed: its name has no extension, one this
/// table doesn't know, or an ambiguous one (`.ts`: TypeScript or an MPEG transport stream;
/// `.key`: Keynote or a key file). Only files: a bundle is a folder, and links aren't followed.
pub fn needs_sniffing(kind: ItemKind, name: &str) -> bool {
    kind == ItemKind::File
        && extension_of(name).is_none_or(|ext| {
            matches!(ext.as_str(), "ts" | "key")
                || TypeCategory::from_lowercase_extension(&ext) == TypeCategory::Other
        })
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
    fn names_that_need_sniffing() {
        let cases = [
            (ItemKind::File, "notes", true, "no extension"),
            (ItemKind::File, "download.bak", true, "unknown extension"),
            (
                ItemKind::File,
                "index.ts",
                true,
                "TypeScript or an MPEG transport stream",
            ),
            (ItemKind::File, "server.KEY", true, "Keynote or a key file"),
            (ItemKind::File, "Holiday.jpg", false, "known extension"),
            (ItemKind::File, "Setup.dmg", false, "known extension"),
            (
                ItemKind::Bundle,
                "Talk.key",
                false,
                "a bundle's contents are a folder",
            ),
            (ItemKind::Dir, "folder", false, "a folder"),
            (ItemKind::Symlink, "link", false, "links aren't followed"),
        ];
        for (kind, name, sniff, why) in cases {
            assert_eq!(needs_sniffing(kind, name), sniff, "{name}: {why}");
        }
    }

    #[test]
    fn categories_by_sniffed_mime() {
        let cases = [
            ("image/png", Some(Images)),
            ("image/heif", Some(Images)),
            ("image/vnd.djvu", Some(Ebooks)),
            ("video/mp2t", Some(Videos)),
            ("video/quicktime", Some(Videos)),
            ("audio/mpeg", Some(Audio)),
            ("application/pdf", Some(Pdfs)),
            ("application/zip", Some(Archives)),
            ("application/gzip", Some(Archives)),
            ("application/x-apple-diskimage", Some(Installers)),
            ("application/vnd.microsoft.portable-executable", None),
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                Some(Documents),
            ),
            ("application/vnd.ms-excel", Some(Spreadsheets)),
            (
                "application/vnd.oasis.opendocument.presentation",
                Some(Presentations),
            ),
            ("application/epub+zip", Some(Ebooks)),
            ("font/woff2", Some(Fonts)),
            ("application/font-sfnt", Some(Fonts)),
            ("text/x-shellscript", Some(Code)),
            ("application/x-executable", None), // a program, not an installer
            ("application/x-x509-ca-cert", None),
            ("application/octet-stream", None),
            ("", None),
        ];
        for (mime, category) in cases {
            assert_eq!(TypeCategory::from_mime(mime), category, "{mime:?}");
        }
    }

    #[test]
    fn a_sniffed_type_wins_only_where_the_name_cant_tell() {
        use Sniffed::{Mime, NotRead, Unknown};
        let file = ItemKind::File;
        #[rustfmt::skip]
        let cases = [
            // (kind, name, sniffed, screenshot, category, why)
            (file, "IMG_0042", Mime("image/jpeg"), false, Images, "no extension"),
            (file, "photo.download", Mime("image/png"), true, Screenshots, "a screenshot after sniffing"),
            (file, "archive.bin", Mime("application/zip"), false, Archives, "unknown extension"),
            (file, "archive.bin", Unknown, false, Other, "read, but nothing recognised"),
            (file, "archive.bin", NotRead, false, Other, "a placeholder isn't read"),
            (file, "recording.ts", Mime("video/mp2t"), false, Videos, "a transport stream"),
            (file, "main.ts", Unknown, false, Code, "TypeScript is text"),
            (file, "main.ts", NotRead, false, Code, "not read: the extension's guess"),
            (file, "Talk.key", Mime("application/zip"), false, Presentations, "Keynote files are zips"),
            (file, "server.key", Unknown, false, Other, "a key file"),
            (file, "server.key", Mime("application/x-x509-ca-cert"), false, Other, "a DER key file"),
            (file, "Talk.key", NotRead, false, Presentations, "not read: the extension's guess"),
            (file, "report.pdf", Mime("application/zip"), false, Pdfs, "a known extension isn't sniffed"),
            (file, "System.Data.dll", Mime("application/vnd.microsoft.portable-executable"), false, Other, "a library, not an installer"),
            (file, "Shot.png", NotRead, true, Screenshots, "like `of`"),
            (ItemKind::Bundle, "Talk.key", Mime("application/zip"), false, Presentations, "bundles aren't sniffed"),
            (ItemKind::Dir, "data", Mime("image/png"), false, Other, "folders aren't sniffed"),
        ];
        for (kind, name, sniffed, screenshot, category, why) in cases {
            assert_eq!(
                TypeCategory::classify(kind, name, sniffed, screenshot),
                category,
                "{name}: {why}"
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
