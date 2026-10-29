//! Watching the folder for changes made outside Cairn (FR-KN-EXT-6, D-72).
//!
//! [`FolderWatcher`] subscribes to the system's file-change notification (`notify`: inotify on
//! Linux, ReadDirectoryChangesW on Windows) for the whole root and collects the paths that
//! changed. It hands them over as a [`WatchBatch`] **at most once a second**, and not before a
//! short settling time after the first event, so a burst (a git checkout, a sync) costs one pass,
//! not one per file. When the system reports that it missed events (or the watcher fails), the
//! batch asks for a full scan instead.
//!
//! Ignored on the way: hidden paths (`.git`, `.obsidian`, Cairn's temporary files), events that
//! only read a file, and Cairn's own writes, recognised by their hash (`is_own_write`). What is
//! left goes through the same code as a scan ([`super::scan::Scope::Paths`]), which also skips a
//! page whose row already has the file's hash, so an echo that gets through changes nothing.
//! Links (symbolic links, junctions) are not followed, so nothing outside the root is watched.
//!
//! A change Cairn made and then rolled back (`journal`) hands its paths to the watcher of their
//! root ([`rescan_later`]) as if the system had reported them; with no watcher running they wait
//! for the next one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecursiveMode, Watcher};

use super::scan::relevant;
use super::{Ext, is_own_write};
use crate::{CoreError, Result};

/// At most one batch per this long (FR-KN-EXT-6).
pub const PASS_EVERY: Duration = Duration::from_secs(1);
/// A batch waits at least this long after its first event, so the rest of a burst joins it.
const SETTLE: Duration = Duration::from_millis(200);

/// Changes seen since the last batch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchBatch {
    /// Root-relative paths (files or folders) that changed, sorted.
    pub paths: Vec<String>,
    /// The system missed events: scan the whole folder.
    pub rescan: bool,
}

/// A running watch of one root folder. Stops when dropped.
pub struct FolderWatcher {
    id: u64,
    watcher: Option<notify::RecommendedWatcher>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

type EventSender = Sender<notify::Result<Event>>;

/// The watchers running now: their id, root and the channel their thread reads.
static WATCHERS: LazyLock<Mutex<Vec<(u64, PathBuf, EventSender)>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Paths of rolled-back changes that no running watcher covered yet.
static PENDING: LazyLock<Mutex<Vec<PathBuf>>> = LazyLock::new(|| Mutex::new(Vec::new()));

fn changed(path: PathBuf) -> notify::Result<Event> {
    Ok(Event::new(EventKind::Any).add_path(path))
}

/// Paths whose files Cairn changed in a transaction that was rolled back: the watcher of their
/// root gets them as changes (its next batch scans them); without one they wait for the next
/// watcher of that root.
pub(crate) fn rescan_later(paths: Vec<PathBuf>) {
    let watchers = WATCHERS.lock().unwrap_or_else(|e| e.into_inner());
    let mut left = Vec::new();
    for p in paths {
        match watchers.iter().find(|(_, root, _)| p.starts_with(root)) {
            Some((_, _, tx)) => {
                let _ = tx.send(changed(p));
            }
            None => left.push(p),
        }
    }
    drop(watchers);
    if !left.is_empty() {
        PENDING.lock().unwrap_or_else(|e| e.into_inner()).extend(left);
    }
}

fn watch_error(e: notify::Error) -> CoreError {
    let why = match e.kind {
        // Linux counts watched folders per user (`fs.inotify.max_user_watches`), across every
        // program: a code editor open on a large folder can use nearly all of them.
        notify::ErrorKind::MaxFilesWatch => "the system's limit on watched folders is reached. \
            Other programs, such as a code editor, may be using most of it; on Linux, raising \
            fs.inotify.max_user_watches fixes this"
            .to_string(),
        _ => e.to_string(),
    };
    CoreError::Io(std::io::Error::other(format!(
        "The knowledge folder can't be watched for changes: {why}."
    )))
}

impl FolderWatcher {
    /// Starts watching `root`; `deliver` gets each batch on the watcher's own thread.
    pub fn start(root: &Path, deliver: impl FnMut(WatchBatch) + Send + 'static) -> Result<FolderWatcher> {
        let (tx, rx) = channel::<notify::Result<Event>>();
        let config = Config::default().with_follow_symlinks(false);
        let mut watcher = notify::RecommendedWatcher::new(tx.clone(), config).map_err(watch_error)?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(watch_error)?;
        let stop = Arc::new(AtomicBool::new(false));
        let ext = Ext::new(root.to_path_buf());
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name("knowledge-watch".into())
            .spawn(move || run(ext, rx, deliver, flag))?;
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        // Rolled-back changes that waited for a watcher of this root.
        {
            let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
            let (mine, others): (Vec<PathBuf>, Vec<PathBuf>) =
                pending.drain(..).partition(|p| p.starts_with(root));
            *pending = others;
            for p in mine {
                let _ = tx.send(changed(p));
            }
        }
        WATCHERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, root.to_path_buf(), tx));
        Ok(FolderWatcher {
            id,
            watcher: Some(watcher),
            stop,
            thread: Some(thread),
        })
    }

    /// Stops watching and waits for the thread (a batch being delivered finishes first).
    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the watcher and the registry's sender closes the channel, which wakes the thread.
        WATCHERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(id, _, _)| *id != self.id);
        self.watcher.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for FolderWatcher {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn run(
    ext: Ext,
    rx: Receiver<notify::Result<Event>>,
    mut deliver: impl FnMut(WatchBatch),
    stop: Arc<AtomicBool>,
) {
    let mut paths: BTreeSet<String> = BTreeSet::new();
    let mut rescan = false;
    let mut first: Option<Instant> = None;
    let mut last: Option<Instant> = None;
    loop {
        let wait = match first {
            None => Duration::from_secs(60),
            Some(f) => {
                let due = (f + SETTLE).max(last.map_or(f, |l| l + PASS_EVERY));
                due.saturating_duration_since(Instant::now())
            }
        };
        match rx.recv_timeout(wait) {
            Ok(Ok(event)) => {
                if event.need_rescan() {
                    rescan = true;
                }
                if !event.kind.is_access() {
                    for p in &event.paths {
                        if let Some(rel) = relative(&ext, p)
                            && !is_own_write(p)
                        {
                            paths.insert(rel);
                        }
                    }
                }
                if (rescan || !paths.is_empty()) && first.is_none() {
                    first = Some(Instant::now());
                }
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "the knowledge folder watcher reported an error");
                rescan = true;
                first.get_or_insert_with(Instant::now);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Some(f) = first {
            let due = (f + SETTLE).max(last.map_or(f, |l| l + PASS_EVERY));
            if Instant::now() >= due {
                let batch = WatchBatch {
                    paths: std::mem::take(&mut paths).into_iter().collect(),
                    rescan: std::mem::take(&mut rescan),
                };
                first = None;
                last = Some(Instant::now());
                deliver(batch);
            }
        }
    }
}

/// The root-relative path of an event's path, if a scan would look at it.
fn relative(ext: &Ext, p: &Path) -> Option<String> {
    let rel = ext.rel(p)?;
    relevant(&rel).then_some(rel)
}
