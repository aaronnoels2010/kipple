// SPDX-License-Identifier: GPL-3.0-or-later
//! Enriching a Location's Items (ARCHITECTURE §7.1 steps 4–5): the download origin and the
//! screenshot flag of every file and bundle, the type sniffed from the first bytes where the
//! name can't tell, and EXIF and dimensions of images. A placeholder is never opened: reading
//! it would download it.
//!
//! A pass pages through the Items still to enrich, [`BATCH`] at a time: it reads the files
//! outside any transaction, then stores the batch in one. Pause and cancel take effect between
//! batches. What a rescan changed meanwhile isn't stored, and waits for the next pass. A job does
//! one slice of a pass, [`SLICE`] Items at most, and then ends, saying where the next slice
//! starts: a running job is never interrupted, so this is how a scan queued meanwhile gets its
//! turn (ARCHITECTURE §7.1).

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use exif::{Exif, In, Tag, Value};
use kipple_core::category::{Sniffed, TypeCategory, needs_sniffing};
use kipple_core::enrichment::{Enrichment, MediaMeta};
use kipple_core::item::{Item, ItemId, ItemKind};
use kipple_core::location::Location;
use kipple_core::time::Timestamp;
use kipple_index::IndexDb;
use kipple_platform::{DownloadOrigins, Placeholders, Screenshots, Volumes};

use crate::jobs::JobError;
use crate::locations::is_online;
use crate::scan::Control;

/// The OS facts the enrichers need.
pub struct EnrichPorts<'a> {
    pub volumes: &'a dyn Volumes,
    /// Asked again before any read: the index's flag is as of the last scan.
    pub placeholders: &'a dyn Placeholders,
    pub origins: &'a dyn DownloadOrigins,
    pub screenshots: &'a dyn Screenshots,
}

impl std::fmt::Debug for EnrichPorts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnrichPorts").finish_non_exhaustive()
    }
}

/// What a run looked at and stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnrichSummary {
    pub items: u64,
    /// Fewer than `items` when some changed meanwhile, vanished, or turned into placeholders:
    /// those wait for the next scan and run.
    pub stored: u64,
    /// Where the next slice starts; `None` when this one reached the end.
    pub next: Option<ItemId>,
}

/// Why a run stopped. Each has a stable code for the UI (ADR-0004).
#[derive(Debug, thiserror::Error)]
pub enum EnrichError {
    /// Missing, or on another volume than the one it was added on: nothing is stored.
    #[error("location offline")]
    Offline,
    #[error(transparent)]
    Index(#[from] kipple_index::Error),
    #[error("cancelled")]
    Cancelled,
}

impl EnrichError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Offline => "enrich_offline",
            Self::Index(_) => "enrich_index_failed",
            Self::Cancelled => "enrich_cancelled",
        }
    }
}

impl From<EnrichError> for JobError {
    fn from(error: EnrichError) -> Self {
        match error {
            EnrichError::Cancelled => JobError::Cancelled,
            other => JobError::Failed(other.code()),
        }
    }
}

/// Items per batch. Each one can mean opening the file (for a sniff, EXIF, dimensions), so a
/// batch takes well under a second on an SSD and a few on a slow share: short enough for a
/// pause or cancel to feel immediate, and large enough that the one write per batch is cheap.
pub const BATCH: usize = 256;
/// Items per slice: what one job does before it ends and the next slice is queued. A few
/// seconds on a slow share, so a scan never waits long behind it for a one-slot volume. A
/// count rather than a time, so a slice is the same on every machine.
pub const SLICE: usize = 4 * BATCH;
/// Read for sniffing: enough for `infer`'s matchers (the zip-based Office formats look
/// furthest) and for 43 transport-stream packets.
const SNIFF_BYTES: u64 = 8 * 1024;
/// Read of an image for its EXIF and dimensions, which cameras and encoders write near the
/// start. Neither reader stops by itself: `image` reads a whole JPEG before its headers, and
/// `kamadak-exif` reads to the end of a file that has no EXIF. EXIF that a WebP, PNG or HEIF
/// file keeps after more image data than this isn't read; a TIFF's first IFD is (see
/// [`trailing_ifd0`]).
const IMAGE_HEAD_BYTES: u64 = 1024 * 1024;
/// The most entries read from a TIFF's first IFD past the head: 3 KiB. Entries are sorted by
/// tag, and the ones wanted (256, 257, 274) come early.
const IFD_ENTRIES: usize = 256;

/// Enriches one slice of `location`'s Items that changed since they were last enriched (or never
/// were): up to [`SLICE`] of them, after `from`. A pass starts at `None` and goes on from each
/// slice's [`EnrichSummary::next`] until that's `None`. The cursor moves past what couldn't be
/// stored too, so a pass always ends; those Items wait for the next pass. An offline Location is
/// left alone; a file that can't be read gets what its metadata gives.
pub fn enrich(
    location: &Location,
    from: Option<ItemId>,
    index: &mut IndexDb,
    ports: &EnrichPorts,
    control: &dyn Control,
    now: Timestamp,
) -> Result<EnrichSummary, EnrichError> {
    let enricher = Enricher::new(ports);
    let mut summary = EnrichSummary::default();
    let mut after = from;
    for _ in 0..SLICE / BATCH {
        control.checkpoint().map_err(|_| EnrichError::Cancelled)?;
        // Checked per batch: an ejected drive would otherwise look like files gone one by one.
        if !is_online(location, ports.volumes) {
            return Err(EnrichError::Offline);
        }
        let items = index.items_to_enrich(location.id, after, BATCH)?;
        let Some(last) = items.last() else {
            return Ok(summary);
        };
        after = last.id;
        let learned: Vec<Enrichment> = items.iter().filter_map(|i| enricher.enrich(i)).collect();
        summary.items += items.len() as u64;
        summary.stored += index.apply_enrichment(&learned, now)? as u64;
        control.progress(summary.items, None);
        if items.len() < BATCH {
            return Ok(summary);
        }
    }
    // Possibly nothing left after it: then the next slice finds that out.
    summary.next = after;
    Ok(summary)
}

struct Enricher<'a> {
    ports: &'a EnrichPorts<'a>,
    sniffer: infer::Infer,
}

impl<'a> Enricher<'a> {
    fn new(ports: &'a EnrichPorts<'a>) -> Self {
        let mut sniffer = infer::Infer::new();
        // `infer` has no matcher for these (`.ts` video).
        sniffer.add("video/mp2t", "ts", is_transport_stream);
        Self { ports, sniffer }
    }

    /// What can be learned about `item`. `None` when it has to wait: gone since the scan (the
    /// next one drops it), or no longer the placeholder the index says it is, or the other way
    /// round (the next scan records that).
    fn enrich(&self, item: &Item) -> Option<Enrichment> {
        let path = &item.identity.path;
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
            // Not a file any more (a FIFO or a link in its place): the next scan records that.
            Ok(meta) if item.kind == ItemKind::File && !meta.is_file() => return None,
            _ => {}
        }
        // Unsupported (Linux has no placeholders) or unreadable: as scanned.
        let placeholder = self.ports.placeholders.is_placeholder(path);
        if placeholder.unwrap_or(item.is_placeholder) != item.is_placeholder {
            return None;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // A port that can't answer (unsupported, unreadable) leaves its field empty.
        let origin_url = self
            .ports
            .origins
            .origin_of(path)
            .ok()
            .flatten()
            .map(without_blob);
        let is_screenshot = self.ports.screenshots.is_screenshot(path).unwrap_or(false);
        // Bundles are folders: their origin and flag only.
        let readable = item.kind == ItemKind::File && !item.is_placeholder;
        let sniffed = if readable && needs_sniffing(item.kind, &name) {
            self.sniff(path)
        } else {
            Sniffed::NotRead
        };
        let category = TypeCategory::classify(item.kind, &name, sniffed, is_screenshot);
        let media =
            if readable && matches!(category, TypeCategory::Images | TypeCategory::Screenshots) {
                media_of(path, is_raw(&name))
            } else {
                MediaMeta::default()
            };
        Some(Enrichment {
            item: item.id?,
            file: item.identity.file,
            content: item.content.clone(),
            is_placeholder: item.is_placeholder,
            mime: match sniffed {
                Sniffed::Mime(mime) => Some(mime.to_string()),
                Sniffed::NotRead | Sniffed::Unknown => None,
            },
            category,
            is_screenshot,
            origin_url,
            media,
        })
    }

    fn sniff(&self, path: &Path) -> Sniffed<'static> {
        let mut head = Vec::new();
        let read = open(path).and_then(|file| file.take(SNIFF_BYTES).read_to_end(&mut head));
        match read {
            Err(_) => Sniffed::NotRead,
            Ok(_) => self
                .sniffer
                .get(&head)
                .map_or(Sniffed::Unknown, |kind| Sniffed::Mime(kind.mime_type())),
        }
    }
}

/// Opens a file to read. On Windows the open asks for the data to stay in the cloud
/// (`FILE_FLAG_OPEN_NO_RECALL`), should the file have become a placeholder since it was checked.
fn open(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_NO_RECALL: u32 = 0x0010_0000;
        options.custom_flags(FILE_FLAG_OPEN_NO_RECALL);
    }
    // Swapped since it was checked: a link isn't followed, and a FIFO doesn't block.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options.open(path)
}

/// MPEG transport streams are 188-byte packets, each starting with the sync byte 0x47. At
/// least three must be there: plenty of text starts with a `G`.
fn is_transport_stream(buf: &[u8]) -> bool {
    const PACKET: usize = 188;
    buf.len() > 2 * PACKET && buf.iter().step_by(PACKET).all(|&b| b == 0x47)
}

/// Browsers record `blob:https://…` for a download the page built in script (macOS
/// `kMDItemWhereFroms`). The URL inside names the site, which is what's worth keeping.
fn without_blob(url: String) -> String {
    match url.strip_prefix("blob:") {
        Some(inner) => inner.to_string(),
        None => url,
    }
}

/// Camera RAW files. Most are TIFF-based, and the first image in them is a small preview.
fn is_raw(name: &str) -> bool {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some("raw" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "orf" | "rw2" | "raf" | "pef" | "srw")
    )
}

/// Dimensions and EXIF of an image, from its first [`IMAGE_HEAD_BYTES`]; empty where they
/// can't tell. Only headers are read, never the pixels. The decoder's dimensions win: EXIF's
/// are what the camera wrote, and an edited copy keeps them. They're the fallback for formats
/// `image` can't read (HEIC), and all there is for a `raw` file: the decoder would find its
/// preview.
fn media_of(path: &Path, raw: bool) -> MediaMeta {
    let Ok(mut file) = open(path) else {
        return MediaMeta::default();
    };
    let Ok(head) = head(&mut file) else {
        return MediaMeta::default();
    };
    // A TIFF whose first IFD is past the head, where neither reader looks.
    if let Some(ifd0) = trailing_ifd0(&head, &mut file) {
        // A RAW's first IFD is its preview, and its EXIF (with the size) isn't read here.
        let (width, height) = if raw {
            None
        } else {
            ifd0.width.zip(ifd0.height)
        }
        .unzip();
        return MediaMeta {
            width,
            height,
            orientation: ifd0.orientation.filter(|n| (1..=8).contains(n)),
            ..MediaMeta::default()
        };
    }
    // A head cut off mid-image is fine: the decoders read only the headers in it.
    let decoded = (!raw)
        .then(|| {
            image::ImageReader::new(Cursor::new(head.as_slice()))
                .with_guessed_format()
                .ok()?
                .into_dimensions()
                .ok()
        })
        .flatten();
    let exif = read_exif(head);
    let exif = exif.as_ref();
    let recorded = exif.and_then(|e| {
        Some((
            uint(e, Tag::PixelXDimension)?,
            uint(e, Tag::PixelYDimension)?,
        ))
    });
    let (width, height) = decoded.or(recorded).unzip();
    MediaMeta {
        width,
        height,
        taken_at: exif.and_then(taken_at),
        camera_make: exif.and_then(|e| text(e, Tag::Make)),
        camera_model: exif.and_then(|e| text(e, Tag::Model)),
        orientation: exif
            .and_then(|e| uint(e, Tag::Orientation))
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| (1..=8).contains(n)),
    }
}

/// The first [`IMAGE_HEAD_BYTES`] of `file`.
fn head(file: impl Read) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    file.take(IMAGE_HEAD_BYTES).read_to_end(&mut head)?;
    Ok(head)
}

/// The EXIF in the `head` of a JPEG, TIFF (most RAW formats), HEIF, PNG or WebP file; `None` if
/// it has none, it's further in, or it's unreadable.
fn read_exif(head: Vec<u8>) -> Option<Exif> {
    let mut exif = exif::Reader::new();
    exif.continue_on_error(true);
    let read = if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        exif.read_raw(head)
    } else {
        exif.read_from_container(&mut Cursor::new(head))
    };
    // Damaged fields are skipped: keep the ones that could be read.
    read.or_else(|e| e.distill_partial_result(|_| {})).ok()
}

/// What a TIFF's first IFD says, when it lies past the `head`: libtiff writes it after the
/// pixels by default (GIMP, ImageMagick, scanners). One more read of [`IFD_ENTRIES`] entries at
/// most, for the size and orientation; the EXIF IFD it points to (camera, date) isn't read.
/// `None` if `head` isn't a TIFF, or the IFD is in it.
fn trailing_ifd0(head: &[u8], file: &mut (impl Read + Seek)) -> Option<Ifd0> {
    let big_endian = match head.get(..4)? {
        b"MM\0*" => true,
        b"II*\0" => false,
        _ => return None,
    };
    let u16_at = |bytes: &[u8], at: usize| {
        let b = bytes.get(at..at + 2)?.try_into().ok()?;
        Some(if big_endian {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let u32_at = |bytes: &[u8], at: usize| {
        let b = bytes.get(at..at + 4)?.try_into().ok()?;
        Some(if big_endian {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    };
    let offset = u32_at(head, 4)?;
    let at = usize::try_from(offset).ok()?;
    if let Some(count) = u16_at(head, at)
        && at.saturating_add(2 + 12 * usize::from(count)) <= head.len()
    {
        return None;
    }
    file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
    let mut ifd = Vec::new();
    file.take(2 + 12 * IFD_ENTRIES as u64)
        .read_to_end(&mut ifd)
        .ok()?;
    let count = usize::from(u16_at(&ifd, 0)?);
    let mut found = Ifd0::default();
    for entry in ifd[2..].as_chunks::<12>().0.iter().take(count) {
        // SHORT (3) or LONG (4), one value, in the entry itself.
        let value = match u16_at(entry, 2)? {
            3 => u32::from(u16_at(entry, 8)?),
            4 => u32_at(entry, 8)?,
            _ => continue,
        };
        match u16_at(entry, 0)? {
            256 => found.width = Some(value),
            257 => found.height = Some(value),
            274 => found.orientation = u16::try_from(value).ok(),
            _ => {}
        }
    }
    Some(found)
}

/// The tags read from a TIFF's first IFD.
#[derive(Debug, Default, PartialEq, Eq)]
struct Ifd0 {
    width: Option<u32>,
    height: Option<u32>,
    orientation: Option<u16>,
}

fn field(exif: &Exif, tag: Tag) -> Option<&Value> {
    exif.get_field(tag, In::PRIMARY).map(|f| &f.value)
}

fn uint(exif: &Exif, tag: Tag) -> Option<u32> {
    field(exif, tag)?.get_uint(0)
}

/// An ASCII field without the NULs and spaces cameras pad it with; `None` if that leaves nothing.
fn text(exif: &Exif, tag: Tag) -> Option<String> {
    let Value::Ascii(parts) = field(exif, tag)? else {
        return None;
    };
    let text = String::from_utf8_lossy(parts.first()?);
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!text.is_empty()).then(|| text.to_string())
}

/// When the photo was taken. EXIF holds the camera's wall-clock time, and `OffsetTimeOriginal`
/// (EXIF 2.31, not every camera writes it) says which zone that was. Without it the time is read
/// as UTC: then it's off by the camera's offset, but the same on every machine, where the
/// machine's own zone would be no better a guess (photos travel) and make the index depend on
/// where it was built.
fn taken_at(exif: &Exif) -> Option<Timestamp> {
    let ascii = |tag| match field(exif, tag)? {
        Value::Ascii(parts) => parts.first().map(Vec::as_slice),
        _ => None,
    };
    let mut time = exif::DateTime::from_ascii(ascii(Tag::DateTimeOriginal)?).ok()?;
    // Optional: a blank or malformed one is ignored.
    if let Some(subsec) = ascii(Tag::SubSecTimeOriginal) {
        let _ = time.parse_subsec(subsec);
    }
    if let Some(offset) = ascii(Tag::OffsetTimeOriginal) {
        let _ = time.parse_offset(offset);
    }
    timestamp_of(&time)
}

/// `time` as a Timestamp; `None` for a date that can't be (`0000:00:00`, month 13, February
/// 30th) or is out of range.
fn timestamp_of(time: &exif::DateTime) -> Option<Timestamp> {
    if !(1..=12).contains(&time.month)
        || !(1..=days_in_month(time.year, time.month)).contains(&time.day)
        || time.hour > 23
        || time.minute > 59
        || time.second > 60
    {
        return None;
    }
    let days = days_from_civil(i64::from(time.year), time.month, time.day);
    let secs = days * 86_400
        + i64::from(time.hour) * 3_600
        + i64::from(time.minute) * 60
        + i64::from(time.second)
        - i64::from(time.offset.unwrap_or(0)) * 60;
    let nanos = secs
        .checked_mul(1_000_000_000)?
        .checked_add(i64::from(time.nanosecond.unwrap_or(0)))?;
    Some(Timestamp(nanos))
}

/// How many days `month` (1–12) of `year` has, in the proleptic Gregorian calendar.
fn days_in_month(year: u16, month: u8) -> u8 {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: u8, day: u8) -> i64 {
    let (month, day) = (i64::from(month), i64::from(day));
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &[u8], offset: Option<&[u8]>) -> Option<Timestamp> {
        let mut time = exif::DateTime::from_ascii(text).ok()?;
        if let Some(offset) = offset {
            time.parse_offset(offset).unwrap();
        }
        timestamp_of(&time)
    }

    #[test]
    fn exif_dates_become_timestamps() {
        let secs = |s: i64| Some(Timestamp::from_secs(s));
        assert_eq!(date(b"1970:01:01 00:00:00", None), secs(0));
        assert_eq!(date(b"2000:03:01 00:00:00", None), secs(951_868_800));
        assert_eq!(
            date(b"2024:02:29 12:00:00", None),
            secs(1_709_208_000),
            "a leap day"
        );
        assert_eq!(date(b"1969:12:31 23:59:59", None), secs(-1));
        assert_eq!(
            date(b"2026:05:06 07:08:09", Some(b"+02:00")),
            secs(1_778_044_089),
            "the offset says the zone"
        );
        assert_eq!(
            date(b"2026:05:06 07:08:09", Some(b"-05:30")),
            secs(1_778_051_289 + 5 * 3600 + 30 * 60)
        );
    }

    #[test]
    fn dates_that_cannot_be_have_no_timestamp() {
        for text in [
            &b"0000:00:00 00:00:00"[..],
            b"2026:13:01 00:00:00",
            b"2026:01:32 00:00:00",
            b"2026:02:29 00:00:00", // not a leap year
            b"1900:02:29 00:00:00", // nor is a century, unless it's a 400th
            b"2026:02:31 00:00:00",
            b"2026:04:31 00:00:00",
            b"2026:01:01 24:00:00",
            b"9999:12:31 23:59:59", // past 2262
        ] {
            assert_eq!(date(text, None), None, "{}", String::from_utf8_lossy(text));
        }
    }

    #[test]
    fn leap_days_are_dates() {
        assert!(date(b"2000:02:29 00:00:00", None).is_some());
        assert!(date(b"2024:02:29 00:00:00", None).is_some());
        assert!(date(b"2026:12:31 00:00:00", None).is_some());
    }

    #[test]
    fn only_the_start_of_an_image_is_read() {
        let endless = io::repeat(0xFF);
        assert_eq!(head(endless).unwrap().len() as u64, IMAGE_HEAD_BYTES);
    }

    #[test]
    fn transport_streams_have_a_sync_byte_every_188_bytes() {
        let stream = |packets: usize| -> Vec<u8> {
            (0..packets * 188)
                .map(|i| if i % 188 == 0 { 0x47 } else { 0 })
                .collect()
        };
        assert!(is_transport_stream(&stream(43)));
        assert!(is_transport_stream(&stream(3)));
        assert!(!is_transport_stream(&stream(2)), "too short to tell");
        let mut broken = stream(10);
        broken[188 * 5] = 0;
        assert!(!is_transport_stream(&broken));
        assert!(!is_transport_stream(b"Gulp"));
    }

    #[test]
    fn blob_urls_are_unwrapped() {
        assert_eq!(
            without_blob("blob:https://app.example.com/1b2c".into()),
            "https://app.example.com/1b2c"
        );
        assert_eq!(
            without_blob("https://example.com/a.zip".into()),
            "https://example.com/a.zip"
        );
    }
}
