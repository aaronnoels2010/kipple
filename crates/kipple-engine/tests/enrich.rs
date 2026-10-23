// SPDX-License-Identifier: GPL-3.0-or-later
//! The enrichers against real temp trees: sniffed types, the screenshot flag, download origins,
//! EXIF and dimensions, placeholders left unread, and how a run pages, stops and races a rescan.
// Test fixtures change files.
#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kipple_core::category::TypeCategory;
use kipple_core::identity::VolumeId;
use kipple_core::item::{Item, ItemId, ItemKind};
use kipple_core::location::{
    Location, LocationId, LocationKind, StorageKind, TrashSupport, VolumeCaps,
};
use kipple_core::time::Timestamp;
use kipple_engine::enrich::{BATCH, EnrichError, EnrichPorts, EnrichSummary, SLICE, enrich};
use kipple_engine::jobs::JobError;
use kipple_engine::scan::{Control, ScanOptions, ScanPorts, scan};
use kipple_index::IndexDb;
use kipple_platform::fake::{FakeDownloadOrigins, FakePlaceholders, FakeScreenshots, FakeVolumes};
use kipple_platform::{
    DirEntry, Directories, DownloadOrigins, NativeDirectories, Os, PortResult, ProtectedPaths,
};

const CAPS: VolumeCaps = VolumeCaps {
    trash: TrashSupport::Yes,
    case_sensitive: true,
    stable_file_ids: true,
    storage: StorageKind::Ssd,
};

/// A Location on a temp folder with an index file next to it, and fake ports that know
/// nothing until a test tells them.
struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
    location: Location,
    index: IndexDb,
    volumes: FakeVolumes,
    protected: ProtectedPaths,
    placeholders: FakePlaceholders,
    origins: FakeDownloadOrigins,
    screenshots: FakeScreenshots,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let base = kipple_platform::canonical(dir.path()).unwrap();
    let root = base.join("files");
    fs::create_dir_all(&root).unwrap();
    let location = Location {
        id: LocationId(1),
        path: root.clone(),
        kind: LocationKind::Custom,
        enabled: true,
        volume: VolumeId("temp".into()),
        online: true,
        last_seen_online: None,
        caps: CAPS,
        in_sync_root: false,
    };
    let db = base.join("index.db");
    let mut index = IndexDb::open(&db).unwrap();
    index.put_location(&location).unwrap();
    World {
        _dir: dir,
        volumes: FakeVolumes::single("temp", base.clone(), CAPS),
        // Linux rules: macOS's protect /private, where its temp folder is.
        protected: ProtectedPaths::for_os(Os::Linux, &base, Path::new("/"), &[]),
        placeholders: FakePlaceholders::default(),
        origins: FakeDownloadOrigins {
            origins: Some(BTreeMap::new()),
        },
        screenshots: FakeScreenshots {
            screenshots: Some(BTreeSet::new()),
        },
        root,
        db,
        location,
        index,
    }
}

impl World {
    fn file(&self, rel: &str, content: impl AsRef<[u8]>) {
        let path = self.root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn scan_with(&mut self, directories: &dyn Directories) {
        let ports = ScanPorts {
            directories,
            volumes: &self.volumes,
            protected: &self.protected,
        };
        let options = ScanOptions {
            threads: 2,
            exclusions: Default::default(),
        };
        scan(
            &self.location,
            &mut self.index,
            &ports,
            &options,
            &(),
            Timestamp(1),
        )
        .unwrap();
    }

    fn scan(&mut self) {
        self.scan_with(&NativeDirectories);
    }

    fn enrich_with(
        &mut self,
        origins: &dyn DownloadOrigins,
        control: &dyn Control,
    ) -> Result<EnrichSummary, EnrichError> {
        self.enrich_slice(origins, control, None)
    }

    /// One slice, starting after `from`.
    fn enrich_slice(
        &mut self,
        origins: &dyn DownloadOrigins,
        control: &dyn Control,
        from: Option<ItemId>,
    ) -> Result<EnrichSummary, EnrichError> {
        let ports = EnrichPorts {
            volumes: &self.volumes,
            placeholders: &self.placeholders,
            origins,
            screenshots: &self.screenshots,
        };
        enrich(
            &self.location,
            from,
            &mut self.index,
            &ports,
            control,
            Timestamp(2),
        )
    }

    fn enrich(&mut self) -> EnrichSummary {
        let origins = self.origins.clone();
        self.enrich_with(&origins, &()).unwrap()
    }

    fn origin(&mut self, rel: &str, url: &str) {
        let path = self.root.join(rel);
        self.origins
            .origins
            .as_mut()
            .unwrap()
            .insert(path, url.into());
    }

    fn screenshot(&mut self, rel: &str) {
        let path = self.root.join(rel);
        self.screenshots.screenshots.as_mut().unwrap().insert(path);
    }

    fn item(&self, rel: &str) -> Item {
        let path = self.root.join(rel);
        self.index
            .items_in(LocationId(1), path.parent().unwrap())
            .unwrap()
            .into_iter()
            .find(|i| i.identity.path == path)
            .expect("indexed")
    }

    /// What the enrichers stored beyond `Item`'s fields: (mime, origin, meta_json).
    fn stored(&self, rel: &str) -> Stored {
        let id = self.item(rel).id.unwrap();
        rusqlite::Connection::open(&self.db)
            .unwrap()
            .query_row(
                "SELECT mime, origin_url, meta_json, enriched_at FROM item WHERE id = ?1",
                [id.0],
                |r| {
                    Ok(Stored {
                        mime: r.get(0)?,
                        origin: r.get(1)?,
                        meta: r.get(2)?,
                        enriched: r.get::<_, Option<i64>>(3)?.is_some(),
                    })
                },
            )
            .unwrap()
    }

    /// The Items still to enrich, by path relative to the root.
    fn to_enrich(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .index
            .items_to_enrich(LocationId(1), None, usize::MAX)
            .unwrap()
            .into_iter()
            .map(|i| {
                let rel = i.identity.path.strip_prefix(&self.root).unwrap();
                let rel: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
                rel.join("/")
            })
            .collect();
        names.sort();
        names
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Stored {
    mime: Option<String>,
    origin: Option<String>,
    meta: Option<String>,
    enriched: bool,
}

fn png(width: u32, height: u32) -> Vec<u8> {
    encoded(
        &image::RgbImage::new(width, height),
        image::ImageFormat::Png,
    )
}

fn encoded(image: &image::RgbImage, format: image::ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, format).unwrap();
    out.into_inner()
}

/// Pseudo-random pixels: they don't compress, so the file comes out large.
fn noise(width: u32, height: u32) -> image::RgbImage {
    let mut seed = 1_u32;
    image::RgbImage::from_fn(width, height, |_, _| {
        image::Rgb([0; 3].map(|_: u8| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed.to_be_bytes()[0]
        }))
    })
}

/// MPEG transport stream packets: 188 bytes, each starting with the sync byte 0x47.
fn transport_stream(packets: usize) -> Vec<u8> {
    (0..packets)
        .flat_map(|_| {
            let mut packet = vec![0xFF; 188];
            packet[0] = 0x47;
            packet
        })
        .collect()
}

/// The start of a zip file (a local file header), which is what Keynote documents are.
const ZIP: &[u8] = b"PK\x03\x04\x14\x00\x00\x00\x08\x00\x00\x00\x00\x00Index/Document.iwa";

#[test]
fn files_the_name_cannot_tell_are_sniffed() {
    let mut w = world();
    w.file("IMG_0042", png(5, 3));
    w.file("main.ts", "export const x = 1;\n");
    w.file("recording.ts", transport_stream(8));
    w.file("Talk.key", ZIP);
    w.file("server.key", "-----BEGIN PRIVATE KEY-----\nMIIE\n");
    w.file("notes", "just text");
    w.file("report.pdf", png(1, 1)); // a known extension isn't sniffed
    w.scan();
    assert_eq!(w.item("IMG_0042").category, TypeCategory::Other, "before");

    let summary = w.enrich();
    assert_eq!((summary.items, summary.stored), (7, 7));
    let cases = [
        ("IMG_0042", TypeCategory::Images, Some("image/png")),
        ("main.ts", TypeCategory::Code, None),
        ("recording.ts", TypeCategory::Videos, Some("video/mp2t")),
        (
            "Talk.key",
            TypeCategory::Presentations,
            Some("application/zip"),
        ),
        // `infer` takes any PEM file for a certificate: still a key file.
        (
            "server.key",
            TypeCategory::Other,
            Some("application/x-x509-ca-cert"),
        ),
        ("notes", TypeCategory::Other, None),
        ("report.pdf", TypeCategory::Pdfs, None),
    ];
    for (name, category, mime) in cases {
        assert_eq!(w.item(name).category, category, "{name}");
        assert_eq!(w.stored(name).mime.as_deref(), mime, "{name}");
    }
    assert_eq!(
        w.stored("IMG_0042").meta.as_deref(),
        Some(r#"{"width":5,"height":3}"#),
        "a sniffed image gets its dimensions"
    );
}

#[test]
fn a_short_text_file_starting_with_the_sync_byte_is_not_a_transport_stream() {
    let mut w = world();
    w.file("Gulpfile.ts", "G");
    w.file("short.ts", transport_stream(2));
    w.scan();
    w.enrich();
    assert_eq!(w.item("Gulpfile.ts").category, TypeCategory::Code);
    assert_eq!(
        w.item("short.ts").category,
        TypeCategory::Code,
        "too short to tell"
    );
}

#[test]
fn the_screenshot_flag_turns_images_into_screenshots() {
    let mut w = world();
    w.file("Schermafbeelding 2026-10-21 om 10.00.00.png", png(4, 2));
    w.file("Holiday.png", png(4, 2));
    w.file("Screenshot of a document.pdf", "%PDF-1.7");
    w.screenshot("Schermafbeelding 2026-10-21 om 10.00.00.png");
    w.screenshot("Screenshot of a document.pdf");
    w.scan();
    w.enrich();
    let shot = w.item("Schermafbeelding 2026-10-21 om 10.00.00.png");
    assert!(shot.is_screenshot);
    assert_eq!(shot.category, TypeCategory::Screenshots);
    assert_eq!(
        w.stored("Schermafbeelding 2026-10-21 om 10.00.00.png")
            .meta
            .as_deref(),
        Some(r#"{"width":4,"height":2}"#)
    );
    assert_eq!(w.item("Holiday.png").category, TypeCategory::Images);
    let pdf = w.item("Screenshot of a document.pdf");
    assert!(pdf.is_screenshot, "flagged as the OS says");
    assert_eq!(
        pdf.category,
        TypeCategory::Pdfs,
        "but only images are Screenshots"
    );
}

#[test]
fn download_origins_are_stored_with_blob_urls_unwrapped() {
    let mut w = world();
    w.file("report.pdf", "%PDF-1.7");
    w.file("export.csv", "a,b");
    w.file("local.txt", "x");
    w.file("repo/.git/HEAD", "ref"); // a bundle: its origin, nothing read
    w.origin("report.pdf", "https://example.com/files/report.pdf");
    w.origin(
        "export.csv",
        "blob:https://app.example.com/3f2a9c1e-8d4b-4e0a-9b7c-1d2e3f4a5b6c",
    );
    w.origin("repo/.git", "https://example.com/repo.zip");
    w.scan();
    assert_eq!(w.item("repo/.git").kind, ItemKind::Bundle);
    w.enrich();
    let origin = |rel| w.stored(rel).origin;
    assert_eq!(
        origin("report.pdf").as_deref(),
        Some("https://example.com/files/report.pdf")
    );
    assert_eq!(
        origin("export.csv").as_deref(),
        Some("https://app.example.com/3f2a9c1e-8d4b-4e0a-9b7c-1d2e3f4a5b6c"),
        "the page's own URL, not the blob: wrapper"
    );
    assert_eq!(origin("local.txt"), None);
    assert_eq!(
        w.stored("repo/.git"),
        Stored {
            mime: None,
            origin: Some("https://example.com/repo.zip".into()),
            meta: None,
            enriched: true,
        }
    );
}

#[test]
fn a_port_that_cannot_answer_leaves_its_field_empty() {
    let mut w = world();
    w.file("Shot.png", png(2, 2));
    w.scan();
    w.screenshots.screenshots = None; // Unsupported
    let summary = w
        .enrich_with(&FakeDownloadOrigins { origins: None }, &())
        .unwrap();
    assert_eq!(summary.stored, 1);
    let shot = w.item("Shot.png");
    assert!(!shot.is_screenshot);
    assert_eq!(shot.category, TypeCategory::Images);
    assert_eq!(
        w.stored("Shot.png"),
        Stored {
            mime: None,
            origin: None,
            meta: Some(r#"{"width":2,"height":2}"#.into()),
            enriched: true,
        }
    );
}

/// Marks some paths as placeholders, as a cloud folder would.
struct CloudFolder(Vec<PathBuf>);

impl Directories for CloudFolder {
    fn list(&self, dir: &Path) -> PortResult<Vec<DirEntry>> {
        let mut entries = NativeDirectories.list(dir)?;
        for entry in &mut entries {
            entry.placeholder = self.0.contains(&dir.join(&entry.name));
        }
        Ok(entries)
    }
}

#[test]
fn a_placeholder_is_never_opened() {
    let mut w = world();
    // Readable here, so reading them would show: a placeholder's content isn't on disk.
    w.file("IMG_0042", png(5, 3));
    w.file("Shot.png", png(5, 3));
    let cloud = vec![w.root.join("IMG_0042"), w.root.join("Shot.png")];
    w.placeholders.placeholders = Some(cloud.iter().map(|p| (p.clone(), true)).collect());
    w.origin("IMG_0042", "https://example.com/IMG_0042");
    w.screenshot("Shot.png");
    w.scan_with(&CloudFolder(cloud));
    w.enrich();
    assert_eq!(
        w.stored("IMG_0042"),
        Stored {
            mime: None,
            origin: Some("https://example.com/IMG_0042".into()),
            meta: None,
            enriched: true,
        },
        "not sniffed, but its origin is metadata"
    );
    assert_eq!(w.item("IMG_0042").category, TypeCategory::Other);
    let shot = w.item("Shot.png");
    assert_eq!(shot.category, TypeCategory::Screenshots);
    assert_eq!(w.stored("Shot.png").meta, None, "no dimensions, no EXIF");
}

#[test]
fn a_file_that_became_a_placeholder_since_the_scan_waits_for_the_next_one() {
    let mut w = world();
    w.file("IMG_0042", png(5, 3));
    w.scan();
    // Evicted to the cloud after the scan: the index still says it's local.
    w.placeholders.placeholders = Some([(w.root.join("IMG_0042"), true)].into());
    let summary = w.enrich();
    assert_eq!((summary.items, summary.stored), (1, 0));
    assert_eq!(w.to_enrich(), ["IMG_0042"], "left for after the next scan");
}

#[test]
fn a_file_gone_since_the_scan_is_left_for_the_next_one() {
    let mut w = world();
    w.file("gone.png", png(1, 1));
    w.scan();
    fs::remove_file(w.root.join("gone.png")).unwrap();
    assert_eq!(w.enrich().stored, 0);
    assert_eq!(w.to_enrich(), ["gone.png"]);
}

/// A JPEG of `width` × `height` with an EXIF segment holding `fields`. `frame: false` leaves
/// the image data out, so the decoder can't tell its size and only EXIF can.
fn jpeg_with_exif(width: u32, height: u32, fields: &[exif::Field], frame: bool) -> Vec<u8> {
    jpeg_of_with_exif(&image::RgbImage::new(width, height), fields, frame)
}

fn jpeg_of_with_exif(image: &image::RgbImage, fields: &[exif::Field], frame: bool) -> Vec<u8> {
    let mut tiff = Cursor::new(Vec::new());
    let mut writer = exif::experimental::Writer::new();
    for field in fields {
        writer.push_field(field);
    }
    writer.write(&mut tiff, false).unwrap();
    let tiff = tiff.into_inner();
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&u16::try_from(2 + 6 + tiff.len()).unwrap().to_be_bytes());
    app1.extend_from_slice(b"Exif\0\0");
    app1.extend_from_slice(&tiff);

    let jpeg = encoded(image, image::ImageFormat::Jpeg);
    let mut out = jpeg[..2].to_vec(); // SOI
    out.extend_from_slice(&app1);
    if frame {
        out.extend_from_slice(&jpeg[2..]);
    } else {
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    }
    out
}

fn ascii(tag: exif::Tag, text: &[u8]) -> exif::Field {
    exif::Field {
        tag,
        ifd_num: exif::In::PRIMARY,
        value: exif::Value::Ascii(vec![text.to_vec()]),
    }
}

fn short(tag: exif::Tag, n: u16) -> exif::Field {
    exif::Field {
        tag,
        ifd_num: exif::In::PRIMARY,
        value: exif::Value::Short(vec![n]),
    }
}

#[test]
fn exif_and_dimensions_are_read_from_images() {
    use exif::Tag;
    let mut w = world();
    w.file(
        "IMG_0001.jpg",
        jpeg_with_exif(
            8,
            6,
            &[
                ascii(Tag::Make, b"Kipple Optics\0\0"),
                ascii(Tag::Model, b"  K-1 Mark II "),
                short(Tag::Orientation, 6),
                ascii(Tag::DateTimeOriginal, b"2026:05:06 07:08:09"),
                ascii(Tag::OffsetTimeOriginal, b"+02:00"),
                // What the camera claims: the decoder's answer wins.
                short(Tag::PixelXDimension, 4000),
                short(Tag::PixelYDimension, 3000),
            ],
            true,
        ),
    );
    // The decoder can't read it, as with HEIC (which `image` doesn't know): EXIF has the size.
    w.file(
        "IMG_0002.jpg",
        jpeg_with_exif(
            8,
            6,
            &[
                ascii(Tag::DateTimeOriginal, b"2026:05:06 07:08:09"),
                short(Tag::PixelXDimension, 4032),
                short(Tag::PixelYDimension, 3024),
            ],
            false,
        ),
    );
    w.file("broken.jpg", b"\xFF\xD8\xFF\xE1\x00\x10Exif\0\0garbage");
    // TIFF, the container of most RAW formats: the EXIF is the file itself.
    let mut tiff = Cursor::new(Vec::new());
    let make = ascii(Tag::Make, b"Nikon");
    let mut writer = exif::experimental::Writer::new();
    writer.push_field(&make);
    writer.write(&mut tiff, true).unwrap();
    w.file("scan.tif", tiff.into_inner());
    w.scan();
    w.enrich();
    // 2026-05-06 05:08:09 UTC.
    let taken = 1_778_044_089_000_000_000_i64;
    assert_eq!(
        w.stored("IMG_0001.jpg").meta,
        Some(format!(
            r#"{{"width":8,"height":6,"taken_at":{taken},"camera_make":"Kipple Optics","camera_model":"K-1 Mark II","orientation":6}}"#
        ))
    );
    let utc = taken + 2 * 3600 * 1_000_000_000;
    assert_eq!(
        w.stored("IMG_0002.jpg").meta,
        Some(format!(
            r#"{{"width":4032,"height":3024,"taken_at":{utc}}}"#
        )),
        "sizes from EXIF where the decoder can't tell; no offset: read as UTC"
    );
    assert_eq!(
        w.stored("broken.jpg"),
        Stored {
            mime: None,
            origin: None,
            meta: None,
            enriched: true,
        },
        "bad EXIF is no EXIF"
    );
    assert_eq!(
        w.stored("scan.tif").meta.as_deref(),
        Some(r#"{"camera_make":"Nikon"}"#)
    );
}

/// Changes one file the first time the enricher asks about it, and records the change in
/// the index the way a rescan would: between the enricher's read and its write.
struct RescanDuring {
    path: PathBuf,
    db: PathBuf,
    done: Mutex<bool>,
}

impl DownloadOrigins for RescanDuring {
    fn origin_of(&self, path: &Path) -> PortResult<Option<String>> {
        let mut done = self.done.lock().unwrap();
        if path == self.path && !*done {
            *done = true;
            fs::write(path, "changed, and longer").unwrap();
            let mut index = IndexDb::open(&self.db).unwrap();
            let mut item = index
                .items_in(LocationId(1), path.parent().unwrap())
                .unwrap()
                .into_iter()
                .find(|i| i.identity.path == path)
                .unwrap();
            item.content.size = fs::metadata(path).unwrap().len();
            index.upsert_items(&[item], Timestamp(3)).unwrap();
        }
        Ok(None)
    }
}

#[test]
fn an_item_changed_by_a_rescan_meanwhile_is_left_to_the_next_pass() {
    let mut w = world();
    w.file("notes", "first");
    w.file("other.txt", "x");
    w.scan();
    let racing = RescanDuring {
        path: w.root.join("notes"),
        db: w.db.clone(),
        done: Mutex::new(false),
    };
    let summary = w.enrich_with(&racing, &()).unwrap();
    assert_eq!((summary.items, summary.stored), (2, 1));
    assert_eq!(w.to_enrich(), ["notes"]);
    assert_eq!(w.enrich().stored, 1, "the next pass gets it");
    assert!(w.to_enrich().is_empty());
}

#[test]
fn a_second_run_finds_nothing_to_do() {
    let mut w = world();
    w.file("a.png", png(1, 1));
    w.file("b.txt", "x");
    w.scan();
    assert_eq!(w.enrich().stored, 2);
    assert_eq!(w.enrich(), EnrichSummary::default());
}

/// Cancels after `limit` checkpoints, and records progress.
struct CancelAfter {
    checkpoints: Mutex<usize>,
    limit: usize,
    progress: Mutex<Vec<u64>>,
}

impl CancelAfter {
    fn new(limit: usize) -> Self {
        Self {
            checkpoints: Mutex::new(0),
            limit,
            progress: Mutex::default(),
        }
    }
}

impl Control for CancelAfter {
    fn checkpoint(&self) -> Result<(), JobError> {
        let mut n = self.checkpoints.lock().unwrap();
        *n += 1;
        if *n > self.limit {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn progress(&self, done: u64, _total: Option<u64>) {
        self.progress.lock().unwrap().push(done);
    }
}

#[test]
fn a_run_goes_in_batches_and_stops_between_them_when_cancelled() {
    let mut w = world();
    for i in 0..BATCH + 5 {
        w.file(&format!("f{i:04}.txt"), "x");
    }
    w.scan();
    let origins = w.origins.clone();

    let cancel = CancelAfter::new(1);
    assert!(matches!(
        w.enrich_with(&origins, &cancel),
        Err(EnrichError::Cancelled)
    ));
    assert_eq!(
        *cancel.progress.lock().unwrap(),
        [BATCH as u64],
        "per batch"
    );
    assert_eq!(w.to_enrich().len(), 5, "the first batch is stored");

    let full = CancelAfter::new(usize::MAX);
    let summary = w.enrich_with(&origins, &full).unwrap();
    assert_eq!(summary.stored, 5);
    assert_eq!(*full.progress.lock().unwrap(), [5]);
}

#[test]
fn an_offline_location_is_left_alone() {
    let mut w = world();
    w.file("a.png", png(1, 1));
    w.scan();
    w.volumes.volumes[0].id = VolumeId("another-drive".into());
    let origins = w.origins.clone();
    assert!(matches!(
        w.enrich_with(&origins, &()),
        Err(EnrichError::Offline)
    ));
    assert_eq!(w.to_enrich(), ["a.png"]);
}

/// A TIFF of `width` × `height` grey pixels in one strip, with `fields`. Its IFD comes first and
/// the pixels after it, as cameras write them.
fn tiff(width: u32, height: u32, fields: &[exif::Field]) -> Vec<u8> {
    use exif::{Field, In, Tag, Value};
    let long = |tag, n| Field {
        tag,
        ifd_num: In::PRIMARY,
        value: Value::Long(vec![n]),
    };
    let layout = [
        long(Tag::ImageWidth, width),
        long(Tag::ImageLength, height),
        short(Tag::BitsPerSample, 8),
        short(Tag::Compression, 1),
        short(Tag::PhotometricInterpretation, 1),
        short(Tag::SamplesPerPixel, 1),
        long(Tag::RowsPerStrip, height),
    ];
    let pixels = vec![0x80; usize::try_from(width * height).unwrap()];
    let strips = [pixels.as_slice()];
    let mut writer = exif::experimental::Writer::new();
    for field in layout.iter().chain(fields) {
        writer.push_field(field);
    }
    writer.set_strips(&strips, In::PRIMARY);
    let mut out = Cursor::new(Vec::new());
    writer.write(&mut out, true).unwrap();
    out.into_inner()
}

#[test]
fn large_images_get_their_metadata_from_the_start_of_the_file() {
    use exif::Tag;
    let mut w = world();
    let make = ascii(Tag::Make, b"Kipple Optics");
    w.file(
        "big.jpg",
        jpeg_of_with_exif(&noise(1200, 900), std::slice::from_ref(&make), true),
    );
    w.file(
        "big.png",
        encoded(&noise(800, 600), image::ImageFormat::Png),
    );
    w.file(
        "big.webp",
        encoded(&noise(800, 600), image::ImageFormat::WebP),
    );
    w.file(
        "big.bmp",
        encoded(&noise(800, 600), image::ImageFormat::Bmp),
    );
    // An animation: the first frame is in the prefix, the rest (a long comment here) isn't.
    let small = encoded(&noise(4, 2), image::ImageFormat::Gif);
    let (frame, trailer) = small.split_at(small.len() - 1);
    let mut gif = frame.to_vec();
    gif.extend_from_slice(&[0x21, 0xFE]);
    for _ in 0..(2 << 20) / 255 {
        gif.push(255);
        gif.extend_from_slice(&[b'x'; 255]);
    }
    gif.push(0);
    gif.extend_from_slice(trailer);
    w.file("big.gif", gif);
    w.file("big.tif", tiff(1500, 1000, std::slice::from_ref(&make)));
    for name in [
        "big.jpg", "big.png", "big.webp", "big.bmp", "big.gif", "big.tif",
    ] {
        let size = fs::metadata(w.root.join(name)).unwrap().len();
        assert!(size > 1 << 20, "{name} is larger than what's read: {size}");
    }
    w.scan();
    w.enrich();
    let cases = [
        (
            "big.jpg",
            r#"{"width":1200,"height":900,"camera_make":"Kipple Optics"}"#,
        ),
        ("big.png", r#"{"width":800,"height":600}"#),
        ("big.webp", r#"{"width":800,"height":600}"#),
        ("big.bmp", r#"{"width":800,"height":600}"#),
        ("big.gif", r#"{"width":4,"height":2}"#),
        (
            "big.tif",
            r#"{"width":1500,"height":1000,"camera_make":"Kipple Optics"}"#,
        ),
    ];
    for (name, meta) in cases {
        assert_eq!(w.stored(name).meta.as_deref(), Some(meta), "{name}");
    }
}

#[test]
fn raw_files_get_their_size_from_exif_not_from_the_preview() {
    use exif::Tag;
    let mut w = world();
    let make = ascii(Tag::Make, b"Nikon");
    // TIFF-based: what the decoder finds first is a small preview.
    w.file(
        "DSC_0001.NEF",
        tiff(
            160,
            120,
            &[
                make.clone(),
                short(Tag::PixelXDimension, 6000),
                short(Tag::PixelYDimension, 4000),
            ],
        ),
    );
    w.file("IMG_0001.dng", tiff(160, 120, std::slice::from_ref(&make)));
    w.scan();
    w.enrich();
    assert_eq!(
        w.stored("DSC_0001.NEF").meta.as_deref(),
        Some(r#"{"width":6000,"height":4000,"camera_make":"Nikon"}"#)
    );
    assert_eq!(
        w.stored("IMG_0001.dng").meta.as_deref(),
        Some(r#"{"camera_make":"Nikon"}"#),
        "no size rather than the preview's"
    );
}

#[test]
fn a_slice_stops_at_its_bound_and_the_next_one_goes_on_from_there() {
    let mut w = world();
    for i in 0..SLICE + 5 {
        w.file(&format!("f{i:04}.txt"), "x");
    }
    w.scan();
    let origins = w.origins.clone();
    let first = w.enrich_slice(&origins, &(), None).unwrap();
    assert_eq!((first.items, first.stored), (SLICE as u64, SLICE as u64));
    assert!(first.next.is_some(), "more to do");
    assert_eq!(w.to_enrich().len(), 5);
    let second = w.enrich_slice(&origins, &(), first.next).unwrap();
    assert_eq!((second.items, second.stored, second.next), (5, 5, None));
    assert!(w.to_enrich().is_empty());
}

#[test]
fn a_pass_ends_even_when_nothing_in_it_could_be_stored() {
    let mut w = world();
    for i in 0..=SLICE {
        w.file(&format!("f{i:04}.txt"), "x");
    }
    w.scan();
    // Gone since the scan: none is stored, and they're still the first to enrich.
    for i in 0..=SLICE {
        fs::remove_file(w.root.join(format!("f{i:04}.txt"))).unwrap();
    }
    let origins = w.origins.clone();
    let first = w.enrich_slice(&origins, &(), None).unwrap();
    assert_eq!((first.items, first.stored), (SLICE as u64, 0));
    let second = w.enrich_slice(&origins, &(), first.next).unwrap();
    assert_eq!(
        (second.items, second.stored, second.next),
        (1, 0, None),
        "past them, to the end"
    );
    assert_eq!(w.to_enrich().len(), SLICE + 1, "left for the next scan");
}

#[cfg(unix)]
#[test]
fn a_file_swapped_for_a_fifo_or_a_link_since_the_scan_is_not_opened() {
    let mut w = world();
    w.file("pipe.png", png(1, 1));
    w.file("link.png", png(1, 1));
    w.file("photo.png", png(2, 2));
    w.scan();
    let pipe = w.root.join("pipe.png");
    fs::remove_file(&pipe).unwrap();
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .unwrap();
    assert!(made.success());
    let link = w.root.join("link.png");
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&pipe, &link).unwrap();
    // Opening a FIFO nobody writes to blocks: on another thread, so a hang fails the test.
    let (done, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let summary = w.enrich();
        done.send((w, summary)).unwrap();
    });
    let (w, summary) = ended
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the run ends");
    assert_eq!((summary.items, summary.stored), (3, 1));
    assert_eq!(
        w.to_enrich(),
        ["link.png", "pipe.png"],
        "the next scan sees what they are"
    );
}
