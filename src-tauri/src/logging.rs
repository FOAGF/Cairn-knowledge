//! Rotated log files in `<data>/logs` (NFR-SEC-3, D-47): one file per day, kept for 7 days and
//! at most 50 MB in total (the oldest go first). Logs never contain API keys and no page, task or
//! chat text, even with *Detailed logs* on.
//!
//! The size limit has two parts. [`prune`] runs at start and then hourly: it deletes files older
//! than 7 days and, while the folder is over the limit, the oldest files. [`CappedWriter`] stops a
//! runaway day from breaking the limit between two runs: once the folder holds the limit it drops
//! further lines after one last "log size limit reached" line, until the next prune made room.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{EnvFilter, Registry, fmt, prelude::*, reload};

/// NFR-SEC-3: the days of logs kept.
pub const KEEP_DAYS: i64 = 7;
/// NFR-SEC-3: the most the log files take in total.
pub const MAX_BYTES: u64 = 50 * 1024 * 1024;
/// Room kept below the limit for the final "limit reached" line.
const RESERVE: u64 = 4096;

const FILE_PREFIX: &str = "cairn";
const FILE_SUFFIX: &str = "log";

/// What *Detailed logs* switches on: the debug output of Cairn's own code and of the sidecar,
/// not of the libraries.
const DETAILED: &str = "info,cairn_lib=debug,cairn_core=debug,sidecar=debug";

/// The running log setup, kept as Tauri state for the whole run (dropping it loses buffered lines).
pub struct Logs {
    pub dir: PathBuf,
    detailed: AtomicBool,
    /// Puts back the filter of the start (`CAIRN_LOG` or `info`) when *Detailed logs* goes off.
    base: String,
    filter: Option<reload::Handle<EnvFilter, Registry>>,
    _guard: Option<WorkerGuard>,
}

impl Logs {
    /// The bytes the log files take now.
    pub fn total_bytes(&self) -> u64 {
        folder_bytes(&self.dir)
    }

    pub fn detailed(&self) -> bool {
        self.detailed.load(Ordering::SeqCst)
    }

    /// *Detailed logs* on or off. Not saved: the next start begins with it off (NFR-SEC-3).
    pub fn set_detailed(&self, on: bool) {
        let spec = if on { DETAILED } else { self.base.as_str() };
        if let Some(handle) = &self.filter {
            let _ = handle.reload(EnvFilter::new(spec));
        }
        self.detailed.store(on, Ordering::SeqCst);
        tracing::info!(on, "detailed logs");
    }
}

/// Starts logging.
pub fn init(dir: &Path) -> Logs {
    let base = std::env::var("CAIRN_LOG")
        .ok()
        .filter(|s| EnvFilter::try_new(s).is_ok())
        .unwrap_or_else(|| "info".into());
    let (filter_layer, filter) = reload::Layer::new(EnvFilter::new(&base));

    let budget = Arc::new(Budget::new(MAX_BYTES));
    let file = std::fs::create_dir_all(dir).ok().and_then(|_| {
        prune(dir, today(), KEEP_DAYS, MAX_BYTES);
        budget.used.store(folder_bytes(dir), Ordering::SeqCst);
        Builder::new()
            .rotation(Rotation::DAILY)
            .filename_prefix(FILE_PREFIX)
            .filename_suffix(FILE_SUFFIX)
            .build(dir)
            .ok()
    });
    let (file_layer, guard) = match file {
        Some(appender) => {
            let capped = CappedWriter::new(appender, budget.clone());
            let (writer, guard) = tracing_appender::non_blocking(capped);
            (
                Some(fmt::layer().with_ansi(false).with_writer(writer)),
                Some(guard),
            )
        }
        None => (None, None),
    };
    let stderr_layer = cfg!(debug_assertions).then(|| fmt::layer().with_writer(std::io::stderr));
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();
    install_panic_hook();
    spawn_maintenance(dir.to_path_buf(), budget);
    Logs {
        dir: dir.to_path_buf(),
        detailed: AtomicBool::new(false),
        base,
        filter: Some(filter),
        _guard: guard,
    }
}

/// NFR-SEC-3: a Rust crash is logged (message and location) before the default hook runs, so the
/// usual message on stderr and the unwinding stay as they were.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".into());
        tracing::error!(target: "panic", %location, "panic: {}", one_line(&message, 500));
        previous(info);
    }));
}

/// Hourly: apply the 7-day and 50 MB limits again and tell the writer how much room is left.
fn spawn_maintenance(dir: PathBuf, budget: Arc<Budget>) {
    let _ = std::thread::Builder::new()
        .name("log-maintenance".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(3600));
                prune(&dir, today(), KEEP_DAYS, MAX_BYTES);
                budget.reset(folder_bytes(&dir));
            }
        });
}

/// Collapses a message to one line and cuts it at `max` characters (the screen and the message of
/// a UI error, a panic).
pub fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut cut: String = flat.chars().take(max).collect();
        cut.push('…');
        cut
    }
}

// ------------------------------------------------------------------------------ the files

fn today() -> chrono_date::Date {
    chrono_date::today()
}

/// A tiny civil-date helper so this crate needs no date library: log files are named by UTC day.
mod chrono_date {
    pub type Date = i64; // days since 1970-01-01

    pub fn today() -> Date {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        (secs / 86_400) as i64
    }

    /// `YYYY-MM-DD` to days since the epoch (Howard Hinnant's algorithm).
    pub fn parse(s: &str) -> Option<Date> {
        let mut it = s.split('-');
        let y: i64 = it.next()?.parse().ok()?;
        let m: i64 = it.next()?.parse().ok()?;
        let d: i64 = it.next()?.parse().ok()?;
        if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Some(era * 146_097 + doe - 719_468)
    }
}

/// `cairn.2026-10-16.log` to its day; other files in the folder are not ours to delete.
fn file_day(name: &str) -> Option<chrono_date::Date> {
    let rest = name.strip_prefix(FILE_PREFIX)?.strip_prefix('.')?;
    let day = rest.strip_suffix(FILE_SUFFIX)?.strip_suffix('.')?;
    chrono_date::parse(day)
}

/// Our log files with their day and size, oldest first.
fn log_files(dir: &Path) -> Vec<(chrono_date::Date, u64, PathBuf)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let day = file_day(e.file_name().to_str()?)?;
            let len = e.metadata().ok()?.len();
            Some((day, len, e.path()))
        })
        .collect();
    files.sort();
    files
}

/// The bytes the log files take.
pub fn folder_bytes(dir: &Path) -> u64 {
    log_files(dir).iter().map(|f| f.1).sum()
}

/// NFR-SEC-3: deletes files older than `keep_days` days (today and the days before make
/// `keep_days`) and then the oldest files while the folder holds more than `max_bytes`. The newest
/// file stays, since the app may be writing to it.
fn prune(dir: &Path, today: chrono_date::Date, keep_days: i64, max_bytes: u64) {
    let mut files = log_files(dir);
    files.retain(|(day, _, path)| {
        if *day <= today - keep_days {
            if let Err(e) = std::fs::remove_file(path) {
                tracing::warn!(error = %e, "could not delete an old log file");
                return true;
            }
            return false;
        }
        true
    });
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    while total > max_bytes && files.len() > 1 {
        let (_, len, path) = files.remove(0);
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

// ------------------------------------------------------------------------------ the writer

/// How much of the limit is used, shared by the writer and the hourly prune.
struct Budget {
    limit: u64,
    used: AtomicU64,
    /// The "limit reached" line has been written; nothing more is written until [`Budget::reset`].
    capped: AtomicBool,
}

impl Budget {
    fn new(limit: u64) -> Self {
        Self {
            limit,
            used: AtomicU64::new(0),
            capped: AtomicBool::new(false),
        }
    }

    fn reset(&self, used: u64) {
        self.used.store(used, Ordering::SeqCst);
        if used + RESERVE < self.limit {
            self.capped.store(false, Ordering::SeqCst);
        }
    }
}

/// Writes through to the log file until the folder holds the limit.
struct CappedWriter<W: Write> {
    inner: W,
    budget: Arc<Budget>,
}

impl<W: Write> CappedWriter<W> {
    fn new(inner: W, budget: Arc<Budget>) -> Self {
        Self { inner, budget }
    }
}

impl<W: Write> Write for CappedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let used = self.budget.used.load(Ordering::SeqCst);
        if used + buf.len() as u64 + RESERVE > self.budget.limit {
            if !self.budget.capped.swap(true, Ordering::SeqCst) {
                let line = b"WARN cairn: log size limit reached (50 MB); logging stops until old files are removed\n";
                let _ = self.inner.write_all(line);
            }
            // Dropped on purpose; the caller must not retry.
            return Ok(buf.len());
        }
        let n = self.inner.write(buf)?;
        self.budget.used.fetch_add(n as u64, Ordering::SeqCst);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str, bytes: usize) {
        std::fs::write(dir.join(name), vec![b'x'; bytes]).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn civil_dates_round_trip() {
        assert_eq!(chrono_date::parse("1970-01-01"), Some(0));
        assert_eq!(chrono_date::parse("2026-10-16"), Some(20_742));
        assert_eq!(chrono_date::parse("2026-13-01"), None);
        assert_eq!(file_day("cairn.2026-10-16.log"), Some(20_742));
        assert_eq!(file_day("cairn.log"), None);
        assert_eq!(file_day("other.2026-10-16.log"), None);
    }

    #[test]
    fn prune_keeps_seven_days() {
        let dir = tempfile::tempdir().unwrap();
        let today = chrono_date::parse("2026-10-16").unwrap();
        for d in ["09", "10", "11", "15", "16"] {
            touch(dir.path(), &format!("cairn.2026-10-{d}.log"), 10);
        }
        touch(dir.path(), "notes.txt", 10);
        prune(dir.path(), today, 7, MAX_BYTES);
        // 10-09 is 7 days old: today and the 6 days before it are kept.
        assert_eq!(
            names(dir.path()),
            [
                "cairn.2026-10-10.log",
                "cairn.2026-10-11.log",
                "cairn.2026-10-15.log",
                "cairn.2026-10-16.log",
                "notes.txt"
            ]
        );
    }

    #[test]
    fn prune_drops_the_oldest_over_the_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let today = chrono_date::parse("2026-10-16").unwrap();
        touch(dir.path(), "cairn.2026-10-14.log", 400);
        touch(dir.path(), "cairn.2026-10-15.log", 400);
        touch(dir.path(), "cairn.2026-10-16.log", 400);
        prune(dir.path(), today, 7, 1000);
        assert_eq!(
            names(dir.path()),
            ["cairn.2026-10-15.log", "cairn.2026-10-16.log"]
        );
        // The newest file stays even when it alone is over the limit.
        prune(dir.path(), today, 7, 100);
        assert_eq!(names(dir.path()), ["cairn.2026-10-16.log"]);
        assert_eq!(folder_bytes(dir.path()), 400);
    }

    #[test]
    fn writer_stops_at_the_limit_after_one_last_line() {
        let budget = Arc::new(Budget::new(RESERVE + 100));
        let mut out = Vec::new();
        {
            let mut w = CappedWriter::new(&mut out, budget.clone());
            for _ in 0..10 {
                // Pretends to write all of it every time, so the logger never retries.
                assert_eq!(w.write(&[b'a'; 30]).unwrap(), 30);
            }
        }
        let text = String::from_utf8(out.clone()).unwrap();
        assert_eq!(text.matches("limit reached").count(), 1);
        assert!(text.starts_with(&"a".repeat(90)));
        assert!(out.len() < 100 + RESERVE as usize);
        // Room made by a prune opens the writer again.
        budget.reset(0);
        let mut again = Vec::new();
        CappedWriter::new(&mut again, budget).write_all(b"back").unwrap();
        assert_eq!(again, b"back");
    }

    #[test]
    fn one_line_flattens_and_cuts() {
        assert_eq!(one_line("a\n  b\tc", 10), "a b c");
        assert_eq!(one_line("abcdef", 3), "abc…");
        assert_eq!(one_line("错误错误错误", 2), "错误…");
    }

    #[test]
    fn detailed_switch_changes_the_filter() {
        use std::sync::Mutex;
        #[derive(Clone, Default)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
            type Writer = Sink;
            fn make_writer(&'a self) -> Sink {
                self.clone()
            }
        }
        let sink = Sink::default();
        let (layer, handle) = reload::Layer::new(EnvFilter::new("info"));
        let subscriber = tracing_subscriber::registry()
            .with(layer)
            .with(fmt::layer().with_ansi(false).with_writer(sink.clone()));
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("hidden");
            handle.reload(EnvFilter::new(DETAILED)).unwrap();
            tracing::debug!(target: "cairn_core::x", "shown");
            tracing::debug!(target: "hyper::x", "library debug");
        });
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("shown"));
        assert!(!text.contains("hidden"));
        assert!(!text.contains("library debug"));
    }
}
