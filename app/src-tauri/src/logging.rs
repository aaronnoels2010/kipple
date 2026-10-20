// SPDX-License-Identifier: GPL-3.0-or-later
//! Rolling log files in the app log dir, with file paths redacted before anything is written
//! (ARCHITECTURE §10, [`crate::redact`]). Logs never leave the machine; the diagnostics export
//! (later) bundles them, so they must be safe to share as they are.
//!
//! Log paths as fields named `path`, `*_path`, `file` or `dir`: those are hashed whole. Every
//! line is then scrubbed for anything else that looks like a path.
//!
//! Level: `info`, or set `KIPPLE_LOG` (e.g. `KIPPLE_LOG=debug` or `KIPPLE_LOG=kipple_app=trace`).
//! Messages from the `log` crate (Tauri, wry) are forwarded here too, so they're scrubbed as well.

use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;

use tracing::field::Field;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::field::MakeExt;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::{Writer, debug_fn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, Registry};

use crate::redact::Redactor;

/// Daily files, a week kept: enough to attach to a bug report, small enough to forget about.
const KEEP_DAYS: usize = 7;

/// Wraps a writer so each formatted log line is scrubbed before it reaches the file.
/// `tracing_subscriber::fmt` formats a whole event into one buffer and writes it in one call,
/// so a path is never split across two writes.
pub struct Scrubbing<M> {
    inner: M,
    redactor: Arc<Redactor>,
}

pub struct ScrubbingWriter<W> {
    inner: W,
    redactor: Arc<Redactor>,
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Scrubbing<M> {
    type Writer = ScrubbingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        ScrubbingWriter {
            inner: self.inner.make_writer(),
            redactor: Arc::clone(&self.redactor),
        }
    }
}

impl<W: Write> Write for ScrubbingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let clean = self.redactor.scrub(&String::from_utf8_lossy(buf));
        self.inner.write_all(clean.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// The level filter for a `KIPPLE_LOG` value. Targets without a default level (`kipple_app=trace`)
/// keep `info` for everything else, so panics and errors are never filtered out.
fn filter(spec: Option<&str>) -> Targets {
    let targets = match spec.map(str::parse::<Targets>) {
        Some(Ok(targets)) => targets,
        Some(Err(e)) => {
            eprintln!("Kipple: ignoring KIPPLE_LOG ({e}), logging at info");
            Targets::new()
        }
        None => Targets::new(),
    };
    match targets.default_level() {
        Some(_) => targets,
        None => targets.with_default(tracing::Level::INFO),
    }
}

/// Fields that hold a path: `path`, `file`, `dir`, or a name ending in `_path`, `_file`, `_dir`.
fn is_path_field(name: &str) -> bool {
    ["path", "file", "dir"].iter().any(|kind| {
        name == *kind
            || name
                .strip_suffix(kind)
                .is_some_and(|rest| rest.ends_with('_'))
    })
}

/// The subscriber: a plain-text fmt layer over `writer`, filtered by `KIPPLE_LOG`.
fn subscriber<M>(writer: M, redactor: Arc<Redactor>) -> impl tracing::Subscriber + Send + Sync
where
    M: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let tags = Arc::clone(&redactor);
    let fields = debug_fn(
        move |w: &mut Writer<'_>, field: &Field, value: &dyn fmt::Debug| match field.name() {
            "message" => write!(w, "{value:?}"),
            name if is_path_field(name) => {
                write!(w, "{name}={}", tags.tag_value(&format!("{value:?}")))
            }
            name => write!(w, "{name}={value:?}"),
        },
    )
    .delimited(" ");
    let file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .fmt_fields(fields)
        .with_writer(Scrubbing {
            inner: writer,
            redactor,
        })
        .with_filter(filter(std::env::var("KIPPLE_LOG").ok().as_deref()));
    Registry::default().with(file)
}

fn appender(log_dir: &Path) -> Result<RollingFileAppender, tracing_appender::rolling::InitError> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("kipple")
        .filename_suffix("log")
        .max_log_files(KEEP_DAYS)
        .build(log_dir)
}

/// Starts logging to `log_dir`. Never fails the app: without logs, Kipple still works.
pub fn init(log_dir: &Path, key_path: &Path) {
    let redactor = Arc::new(Redactor::new(load_or_create_key(key_path)));
    let appender = match appender(log_dir) {
        Ok(appender) => appender,
        Err(e) => {
            eprintln!(
                "Kipple: logging disabled: {}",
                redactor.scrub(&e.to_string())
            );
            return;
        }
    };
    if tracing::subscriber::set_global_default(subscriber(appender, redactor)).is_ok() {
        let _ = tracing_log::LogTracer::init();
        install_panic_hook();
    }
}

/// Panics go to the log too (scrubbed like every line), then to the default hook.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(target: "panic", "{info}");
        default(info);
    }));
}

/// The per-install salt for path hashes. Created on first run, and replaced if it's damaged
/// (not exactly 32 bytes). It's not a log, so it's never part of a diagnostics export. If it
/// can't be stored, this run uses a throwaway one (tags then only match within one run).
// Kipple's own data file, not a user file: ADR-0002's Executor rule covers managed locations.
#[allow(clippy::disallowed_methods)]
fn load_or_create_key(path: &Path) -> [u8; 32] {
    if let Some(key) = std::fs::read(path)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
    {
        return key;
    }
    let mut key = [0; 32];
    getrandom::fill(&mut key).expect("the OS random source is available");
    if let Err(e) = store_key(path, &key) {
        eprintln!("Kipple: log tags won't match across runs: {e}");
    }
    key
}

/// Writes the key atomically (synced temporary file + rename), readable only by the user on Unix.
#[allow(clippy::disallowed_methods)] // see `load_or_create_key`
fn store_key(path: &Path, key: &[u8; 32]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("key.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    file.write_all(key)?;
    file.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRETS: &[&str] = &["zoë", "Belastingen", "aangifte", "Zoe", "Vakantie", "nas"];

    /// Everything written to the log folder, as one string.
    fn read_logs(dir: &Path) -> String {
        let mut all = String::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            all += &std::fs::read_to_string(entry.unwrap().path()).unwrap();
        }
        all
    }

    fn with_test_logger(dir: &Path, log: impl FnOnce()) {
        let redactor = Arc::new(Redactor::new([1; 32]));
        let subscriber = subscriber(appender(dir).unwrap(), redactor);
        tracing::subscriber::with_default(subscriber, log);
    }

    #[test]
    fn no_raw_path_reaches_the_log_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = Path::new("/Users/zoë/Documents/Belastingen 2025/aangifte.pdf");
        let windows = Path::new(r"C:\Users\Zoe\Pictures\Vakantie\IMG_0001.jpg");
        let io_error = io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{}: denied", home.display()),
        );
        with_test_logger(dir.path(), || {
            tracing::info!(path = ?home, "scanned");
            tracing::info!(path = %windows.display(), "scanned");
            tracing::warn!("could not open {}: {io_error}", home.display());
            tracing::error!(error = ?io_error, "move failed");
            tracing::info!(r"share \\nas\family\Vakantie is offline");
        });
        let logs = read_logs(dir.path());
        assert_eq!(logs.lines().count(), 5, "{logs}");
        for secret in SECRETS {
            assert!(
                !logs.contains(secret),
                "{secret:?} leaked into the log:\n{logs}"
            );
        }
        assert!(logs.contains("<path#"), "{logs}");
        assert!(
            logs.contains("scanned") && logs.contains("move failed"),
            "{logs}"
        );
    }

    #[test]
    fn path_fields_are_hashed_whole_even_unquoted_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = Path::new("/Users/zoë/tax return 2025.pdf");
        with_test_logger(dir.path(), || {
            tracing::info!(path = %path.display(), "scanned");
            tracing::info!(path = ?path, "scanned");
            tracing::info!(source_path = %path.display(), file = "IMG 0001.jpg", dir = "Vakantie foto's", "moved");
        });
        let logs = read_logs(dir.path());
        for secret in ["zoë", "return", "2025", "IMG", "0001", "Vakantie", "foto"] {
            assert!(
                !logs.contains(secret),
                "{secret:?} leaked into the log:\n{logs}"
            );
        }
        let tag = Redactor::new([1; 32]).tag("/Users/zoë/tax return 2025.pdf");
        assert_eq!(
            logs.matches(&tag).count(),
            3,
            "Debug and Display give the same tag:\n{logs}"
        );
    }

    #[test]
    fn log_crate_messages_are_captured_and_scrubbed() {
        let dir = tempfile::tempdir().unwrap();
        let _ = tracing_log::LogTracer::init();
        with_test_logger(dir.path(), || {
            log::warn!("webview could not load /Users/zoë/Belastingen/x")
        });
        let logs = read_logs(dir.path());
        assert!(logs.contains("webview could not load <path#"), "{logs}");
        assert!(!logs.contains("zoë"), "{logs}");
    }

    #[test]
    fn kipple_log_keeps_an_info_default_and_ignores_nonsense() {
        use tracing::Level;
        let default = filter(None);
        assert!(
            default.would_enable("tauri", &Level::INFO)
                && !default.would_enable("tauri", &Level::DEBUG)
        );
        let targeted = filter(Some("kipple_app=trace"));
        assert!(targeted.would_enable("kipple_app::scan", &Level::TRACE));
        assert!(
            targeted.would_enable("panic", &Level::ERROR)
                && targeted.would_enable("tauri", &Level::INFO)
        );
        assert!(filter(Some("debug")).would_enable("tauri", &Level::DEBUG));
        assert!(filter(Some("nonsense!!=x")).would_enable("panic", &Level::ERROR));
    }

    #[test]
    fn panics_are_logged_without_paths() {
        let dir = tempfile::tempdir().unwrap();
        with_test_logger(dir.path(), || {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(|_| {})); // keep the test output quiet
            install_panic_hook();
            let result =
                std::panic::catch_unwind(|| panic!("cannot read /Users/zoë/Belastingen/x"));
            std::panic::set_hook(previous);
            assert!(result.is_err());
        });
        let logs = read_logs(dir.path());
        assert!(logs.contains("ERROR panic: panicked at"), "{logs}");
        assert!(logs.contains("cannot read <path#"), "{logs}");
        assert!(
            !logs.contains("zoë") && !logs.contains("Belastingen"),
            "{logs}"
        );
    }

    #[test]
    fn the_key_is_created_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("redaction.key");
        let first = load_or_create_key(&path);
        assert_eq!(load_or_create_key(&path), first);
        assert_ne!(first, [0; 32]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn a_truncated_key_is_replaced_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("redaction.key");
        std::fs::write(&path, b"short").unwrap(); // e.g. a crash between create and write
        let key = load_or_create_key(&path);
        assert_eq!(std::fs::read(&path).unwrap(), key);
        assert_eq!(load_or_create_key(&path), key);
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // test fixture
    fn an_unwritable_key_location_still_gives_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        std::fs::File::create(&blocker).unwrap();
        // A path "inside" a regular file can never be created.
        assert_ne!(load_or_create_key(&blocker.join("redaction.key")), [0; 32]);
    }
}
