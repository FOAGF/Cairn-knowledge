//! External knowledge mode in the running app (D-72): the start-up scan, the folder watcher, the
//! asset protocol's scope, and the long switches. The work itself is
//! `cairn_core::knowledge::{external, mode}`.
//!
//! - **Start-up:** a few seconds after the window opens, an external-mode database gets a full
//!   scan of its folder in the background (reading without the database lock, applying in
//!   transactions of about 100 ms each, `scan::BATCH_TIME`), with `KnowledgeProgress` events.
//! - **Watcher:** the core watcher hands over at most one batch a second; a single loop here
//!   applies each batch through the same scan code (only the paths it names, or everything when
//!   the system missed events). A pass that changed something emits `DataChanged` for knowledge,
//!   which also wakes the indexer, so new pieces get embedded. Cairn's own writes change nothing.
//! - **Mode changes** (the first choice, switching, importing, the dev reset): [`KnowledgeFolder::restart`]
//!   stops the watcher, allows the new root in the asset protocol (so `folder:` images show) and,
//!   in external mode, scans and watches again.
//! - **One at a time:** scans and switches share one lock, so a watcher pass never runs in the
//!   middle of a switch.
//!
//! The asset protocol's scope only grows: a root used earlier stays allowed for the rest of the run
//! (Tauri can't take a directory back without forbidding it for good).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cairn_core::knowledge::external::scan::{self, Scope, ScanProgress, ScanSummary};
use cairn_core::knowledge::external::watch::{FolderWatcher, WatchBatch};
use cairn_core::knowledge::mode::{self, SwitchResult};
use cairn_core::CoreError;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::mpsc;

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::state::AppState;

/// The start-up scan waits this long after the window opens (NFR-PERF-2: start-up stays fast).
const STARTUP_DELAY: Duration = Duration::from_secs(3);

/// What a `KnowledgeProgress` event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeTask {
    /// Reading the folder (start-up, *Rescan*, the first choice, after an import).
    Scan,
    /// Switching between internal and external mode (Settings › Data).
    Switch,
}

/// Progress of a scan or a switch, for the UI. The last event of a run has `finished` set, and
/// either `error` or the results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeProgress {
    pub task: KnowledgeTask,
    /// The step in plain words, e.g. "Backing up", "Reading the folder".
    pub step: String,
    /// Files (or changes) done of `total` in this step; both 0 when the step has no count.
    pub done: u32,
    pub total: u32,
    pub finished: bool,
    /// The run failed: why, in plain words.
    pub error: Option<String>,
    /// A finished scan (also a switch's): what changed and the files left out.
    pub scan: Option<ScanSummary>,
    /// A finished switch.
    pub switched: Option<SwitchResult>,
}

impl KnowledgeProgress {
    fn step(task: KnowledgeTask, step: &str, done: u32, total: u32) -> Self {
        KnowledgeProgress {
            task,
            step: step.into(),
            done,
            total,
            finished: false,
            error: None,
            scan: None,
            switched: None,
        }
    }
}

/// Managed state: `app.state::<Arc<KnowledgeFolder>>()`.
pub struct KnowledgeFolder {
    app: AppHandle,
    watcher: Mutex<Option<FolderWatcher>>,
    /// Why the folder isn't watched (the watcher couldn't start), for the *Knowledge storage* card.
    watch_error: Mutex<Option<String>>,
    /// Scans and switches, one at a time.
    busy: tokio::sync::Mutex<()>,
    batches: mpsc::UnboundedSender<WatchBatch>,
}

impl KnowledgeFolder {
    /// Creates the state, starts the loop that applies watcher batches, and schedules the
    /// start-up scan.
    pub fn start(app: AppHandle) -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let folder = Arc::new(KnowledgeFolder {
            app,
            watcher: Mutex::new(None),
            watch_error: Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
            batches: tx,
        });
        tauri::async_runtime::spawn(folder.clone().apply_batches(rx));
        let start = folder.clone();
        tauri::async_runtime::spawn(async move {
            // The root is allowed at once, so `folder:` images show before the scan is done.
            start.allow_root().await;
            tokio::time::sleep(STARTUP_DELAY).await;
            start.restart().await;
        });
        folder
    }

    fn emit(&self, p: KnowledgeProgress) {
        if let Err(e) = p.emit(&self.app) {
            tracing::warn!(error = %e, "failed to emit KnowledgeProgress");
        }
    }

    /// The external root, when the database is in external mode and the folder is there.
    async fn root(&self) -> Option<PathBuf> {
        let state = self.app.state::<AppState>();
        let storage = state
            .with_db(|db| mode::storage(db.conn()))
            .await
            .ok()?;
        storage
            .root
            .filter(|_| storage.root_exists)
            .map(PathBuf::from)
    }

    /// Lets the webview show files of the external root (`folder:` images).
    async fn allow_root(&self) {
        let state = self.app.state::<AppState>();
        let root = state
            .with_db(|db| mode::storage(db.conn()))
            .await
            .ok()
            .and_then(|s| s.root);
        if let Some(root) = root
            && let Err(e) = self.app.asset_protocol_scope().allow_directory(&root, true)
        {
            tracing::warn!(error = %e, "could not allow the knowledge folder for images");
        }
    }

    fn stop_watcher(&self) {
        let old = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(w) = old {
            w.stop();
        }
        *self.watch_error.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn start_watcher(&self, root: &std::path::Path) {
        let tx = self.batches.clone();
        let failed = match FolderWatcher::start(root, move |batch| {
            let _ = tx.send(batch);
        }) {
            Ok(w) => {
                *self.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(w);
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "the knowledge folder is not watched for changes");
                // The reason as written for the user, without the error kind's "file error:" prefix.
                Some(match e {
                    CoreError::Io(io) => io.to_string(),
                    other => other.to_string(),
                })
            }
        };
        *self.watch_error.lock().unwrap_or_else(|e| e.into_inner()) = failed;
    }

    /// Why the folder isn't watched, when the watcher couldn't start.
    pub fn watch_error(&self) -> Option<String> {
        self.watch_error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// *Rescan*: tries the watcher again when it couldn't start before, then scans everything.
    pub async fn rescan(self: &Arc<Self>) {
        let watching = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        if !watching && let Some(root) = self.root().await {
            self.start_watcher(&root);
        }
        let _ = self.scan(Scope::All, true).await;
    }

    /// After the mode or the root changed (or at start-up): stops watching, allows the root for
    /// images and, in external mode, watches the folder and scans it. The watcher starts first, so
    /// a change made while the scan runs is not missed: its batch waits until the scan is applied.
    pub async fn restart(self: &Arc<Self>) {
        self.stop_watcher();
        self.allow_root().await;
        let Some(root) = self.root().await else { return };
        self.start_watcher(&root);
        if let Err(e) = self.scan(Scope::All, true).await {
            tracing::warn!(error = %e.message, "the knowledge folder scan failed");
        }
    }

    /// [`Self::restart`] in the background.
    pub fn restart_soon(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move { me.restart().await });
    }

    /// A scan of `scope`; with `report`, progress events and a finished event. Emits
    /// `DataChanged` when the index changed.
    pub async fn scan(&self, scope: Scope, report: bool) -> ApiResult<ScanSummary> {
        let _one = self.busy.lock().await;
        let r = self.scan_locked(scope, report, KnowledgeTask::Scan).await;
        if report {
            let mut done = KnowledgeProgress::step(KnowledgeTask::Scan, "Done", 0, 0);
            done.finished = true;
            match &r {
                Ok(s) => done.scan = Some(s.clone()),
                Err(e) => done.error = Some(e.message.clone()),
            }
            self.emit(done);
        }
        r
    }

    /// The three steps of a scan (see `cairn_core::knowledge::external::scan`); the caller holds
    /// `busy`.
    async fn scan_locked(&self, scope: Scope, report: bool, task: KnowledgeTask) -> ApiResult<ScanSummary> {
        let state = self.app.state::<AppState>();
        let job = state.with_db(move |db| scan::prepare(db.conn(), scope)).await?;
        if report {
            self.emit(KnowledgeProgress::step(task, "Reading the folder", 0, 0));
        }
        let app = self.app.clone();
        let mut plan = tauri::async_runtime::spawn_blocking(move || {
            let mut last = std::time::Instant::now();
            job.read(&mut |p: ScanProgress| {
                if report && last.elapsed() >= Duration::from_millis(250) {
                    last = std::time::Instant::now();
                    let e = KnowledgeProgress::step(task, "Reading the folder", p.done, p.total);
                    let _ = e.emit(&app);
                }
            })
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;
        let total = plan.total() as u32;
        let mut done = 0u32;
        loop {
            let (back, n) = state
                .with_db(move |db| {
                    let n = plan.apply_some(db, scan::BATCH)?;
                    Ok((plan, n))
                })
                .await?;
            plan = back;
            if n == 0 {
                break;
            }
            done += n as u32;
            if report {
                self.emit(KnowledgeProgress::step(task, "Updating the index", done.min(total), total));
            }
            // Each batch holds the database for about `scan::BATCH_TIME`; a short pause lets the
            // commands that waited for it go first.
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let summary = plan.summary().clone();
        if summary.changed_anything() {
            emit_changed(&self.app, KNOWLEDGE_CHANGED);
        }
        Ok(summary)
    }

    /// Applies watcher batches, one pass at a time; batches that arrived during a pass are merged.
    async fn apply_batches(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<WatchBatch>) {
        while let Some(first) = rx.recv().await {
            let mut paths = first.paths;
            let mut rescan = first.rescan;
            while let Ok(more) = rx.try_recv() {
                paths.extend(more.paths);
                rescan |= more.rescan;
            }
            // Batches from before a switch to internal mode are dropped.
            if (!rescan && paths.is_empty()) || self.root().await.is_none() {
                continue;
            }
            paths.sort();
            paths.dedup();
            let scope = if rescan { Scope::All } else { Scope::Paths(paths) };
            if let Err(e) = self.scan(scope, rescan).await {
                tracing::warn!(error = %e.message, "applying changes from the knowledge folder failed");
            }
        }
    }

    /// FR-KN-EXT-8, internal → external, in the background: the backup, clearing and the
    /// folder's earlier index (one exclusive step), the scan, then the rebuilt indexes.
    pub async fn switch_to_external(self: Arc<Self>, root: String) {
        self.switch_into_folder(root, false).await;
    }

    /// Another knowledge folder while in external mode, in the background: one switch with one
    /// backup (of the old folder's index), as `switch_to_external` from there on.
    pub async fn switch_folder(self: Arc<Self>, root: String) {
        self.switch_into_folder(root, true).await;
    }

    /// Into the folder `root`: from internal mode, or (`change`) from another folder.
    async fn switch_into_folder(self: Arc<Self>, root: String, change: bool) {
        let _one = self.busy.lock().await;
        self.stop_watcher();
        let task = KnowledgeTask::Switch;
        let state = self.app.state::<AppState>();
        self.emit(KnowledgeProgress::step(task, "Backing up and clearing", 0, 0));
        let r: ApiResult<SwitchResult> = async {
            let (backup, restored) = state
                .with_db_exclusive(move |db| {
                    if change {
                        mode::begin_switch_external_root(db, &root)
                    } else {
                        mode::begin_switch_to_external(db, &root)
                    }
                })
                .await?;
            self.allow_root().await;
            // Watched before the scan: changes made meanwhile wait for it.
            if let Some(root) = self.root().await {
                self.start_watcher(&root);
            }
            let scanned = self.scan_locked(Scope::All, true, task).await?;
            self.emit(KnowledgeProgress::step(task, "Rebuilding the search index", 0, 0));
            state.with_db(mode::finish_switch).await?;
            Ok(SwitchResult {
                backup,
                restored,
                scan: Some(scanned),
            })
        }
        .await;
        self.finish_switch(r).await;
    }

    /// FR-KN-EXT-8, external → internal, in the background.
    pub async fn switch_to_internal(self: Arc<Self>, backup: Option<String>) {
        let _one = self.busy.lock().await;
        self.stop_watcher();
        let task = KnowledgeTask::Switch;
        let state = self.app.state::<AppState>();
        self.emit(KnowledgeProgress::step(task, "Backing up, clearing and loading", 0, 0));
        let r = state
            .with_db_exclusive(move |db| mode::switch_to_internal(db, backup.as_deref()))
            .await;
        self.finish_switch(r).await;
    }

    async fn finish_switch(self: &Arc<Self>, r: ApiResult<SwitchResult>) {
        let mut done = KnowledgeProgress::step(KnowledgeTask::Switch, "Done", 0, 0);
        done.finished = true;
        match r {
            Ok(result) => {
                done.scan = result.scan.clone();
                done.switched = Some(result);
            }
            Err(e) => done.error = Some(e.message),
        }
        emit_changed(&self.app, &[Entity::All]);
        self.emit(done);
        // Watch the folder (external; it may already be watched) or stay idle (internal).
        let watching = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        match self.root().await {
            Some(root) if !watching => self.start_watcher(&root),
            Some(_) => {}
            None => self.stop_watcher(),
        }
    }
}

/// A change of the index: pages, groups, categories (folders), activity, mentions and search.
const KNOWLEDGE_CHANGED: &[Entity] = &[
    Entity::Knowledge,
    Entity::Categories,
    Entity::Activity,
    Entity::Mentions,
    Entity::Search,
];
