//! The search model added after installing (FR-RAG-EM-3, D-82). The lite packages leave the
//! embedding model out; this downloads it or takes it from a folder or a `.zip`, checks it, and
//! switches search by meaning on.
//!
//! - **Download:** the AI worker fetches the pinned files from Hugging Face (`model.download`,
//!   resumable) into `<data>/models/.download`. The state file remembers a download under way, so
//!   it resumes at the next start until it finishes or is cancelled; a cancel deletes its files, a
//!   failure keeps them for *Try again*.
//! - **Checks** (`cairn_core::search_model::stage`): sizes and SHA-256 on the bytes written into a
//!   temporary folder; then the search worker must load that folder and embed a test sentence;
//!   only then is it renamed into place and used. The worker is stopped after the test, so it
//!   doesn't keep a second copy of the model loaded under the temporary path.
//! - **State** goes to the UI as the `SearchModelStatus` event (Settings › Search, Home, the
//!   sidebar); the end of a download or an import also sends `SearchModelDone` (a toast).
//! - One download or import at a time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cairn_core::DataPaths;
use cairn_core::search_model::{self as core_model, ModelState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::Notify;

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::indexer::Indexer;
use crate::sidecar::{SidecarError, Sidecars, Worker};

/// Where the model is downloaded from (the pinned repository and revision are appended).
const DOWNLOAD_BASE: &str = "https://huggingface.co";
/// The UI gets at most one progress event per this long.
const EVENT_EVERY: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ModelInstall {
    /// Part of the package (the full packages, or a development setup).
    Bundled,
    /// Added in Settings › Search (download or file); can be removed.
    Added,
    NotInstalled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ModelTaskKind {
    Download,
    Import,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ModelPhase {
    Downloading,
    /// Sizes and hashes, then the test load.
    Checking,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelTask {
    pub kind: ModelTaskKind,
    pub phase: ModelPhase,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Why it failed, in plain language (`phase = failed`).
    pub error: Option<String>,
}

/// The search model's state. Returned by `get_search_model_status`; sent whenever it changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct SearchModelStatus {
    pub install: ModelInstall,
    /// A download or import running, or the last one that failed.
    pub task: Option<ModelTask>,
    pub terms_accepted: bool,
    /// The one-time Home notice was dismissed.
    pub notice_dismissed: bool,
    /// The model's size in bytes (about 200 MB), for the texts.
    pub size_bytes: u64,
}

/// A download or import ended (Home and Settings show a toast).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct SearchModelDone {
    pub kind: ModelTaskKind,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct Cancel {
    flag: AtomicBool,
    notify: Notify,
}

impl Cancel {
    fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    fn is_set(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Inner {
    task: Option<ModelTask>,
    /// Set while a task runs.
    cancel: Option<Arc<Cancel>>,
    last_event: Option<Instant>,
}

/// Managed Tauri state: `app.state::<Arc<SearchModel>>()`.
pub struct SearchModel {
    app: AppHandle,
    paths: DataPaths,
    inner: Mutex<Inner>,
}

enum Outcome {
    Done,
    Cancelled,
}

impl SearchModel {
    /// Creates the manager, removes what an earlier run left half done, and resumes a download
    /// that was under way (FR-RAG-EM-3).
    pub fn start(app: AppHandle, paths: DataPaths) -> Arc<Self> {
        let model = Arc::new(Self {
            app,
            paths,
            inner: Mutex::new(Inner::default()),
        });
        core_model::clean_leftovers(&model.paths);
        let state = core_model::load_state(&model.paths);
        if state.downloading {
            if model.install() == ModelInstall::NotInstalled {
                tracing::info!("resuming the search model download");
                let _ = model.clone().begin(ModelTaskKind::Download, None);
            } else {
                // Installed meanwhile (e.g. a full package): the download isn't needed any more.
                let _ = model.update_state(|s| s.downloading = false);
                let _ = std::fs::remove_dir_all(core_model::download_dir(&model.paths));
            }
        }
        model
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sidecars(&self) -> tauri::State<'_, Sidecars> {
        self.app.state::<Sidecars>()
    }

    fn install(&self) -> ModelInstall {
        match self.sidecars().model_dir() {
            None => ModelInstall::NotInstalled,
            Some(dir) if dir == core_model::added_dir(&self.paths) => ModelInstall::Added,
            Some(_) => ModelInstall::Bundled,
        }
    }

    pub fn status(&self) -> SearchModelStatus {
        let state = core_model::load_state(&self.paths);
        SearchModelStatus {
            install: self.install(),
            task: self.lock().task.clone(),
            terms_accepted: state.terms_accepted,
            notice_dismissed: state.notice_dismissed,
            size_bytes: core_model::pins().total_size(),
        }
    }

    fn emit(&self) {
        self.lock().last_event = Some(Instant::now());
        if let Err(e) = self.status().emit(&self.app) {
            tracing::warn!(error = %e, "failed to emit SearchModelStatus");
        }
    }

    fn update_state(&self, change: impl FnOnce(&mut ModelState)) -> ApiResult<()> {
        let mut state = core_model::load_state(&self.paths);
        change(&mut state);
        core_model::save_state(&self.paths, &state)?;
        Ok(())
    }

    fn set_task(&self, task: Option<ModelTask>) {
        self.lock().task = task;
        self.emit();
    }

    /// New byte count of the running task; sent at most every `EVENT_EVERY`.
    fn progress(&self, done: u64) {
        let send = {
            let mut inner = self.lock();
            let Some(task) = inner.task.as_mut() else {
                return;
            };
            task.done_bytes = done;
            !inner.last_event.is_some_and(|t| t.elapsed() < EVENT_EVERY)
        };
        if send {
            self.emit();
        }
    }

    fn running(&self) -> bool {
        self.lock().cancel.is_some()
    }

    /// Starts a download (`source` None) or an import from `source`, in the background.
    fn begin(self: Arc<Self>, kind: ModelTaskKind, source: Option<PathBuf>) -> ApiResult<()> {
        let cancel = Arc::new(Cancel::default());
        {
            let mut inner = self.lock();
            if inner.cancel.is_some() {
                return Err(conflict("The search model is already being added."));
            }
            inner.cancel = Some(cancel.clone());
            inner.task = Some(ModelTask {
                kind,
                phase: if kind == ModelTaskKind::Download { ModelPhase::Downloading } else { ModelPhase::Checking },
                done_bytes: 0,
                total_bytes: core_model::pins().total_size(),
                error: None,
            });
        }
        self.emit();
        tauri::async_runtime::spawn(async move {
            let result = match &source {
                None => self.download(&cancel).await,
                Some(path) => self.import(path, &cancel).await,
            };
            self.lock().cancel = None;
            let error = match result {
                Ok(Outcome::Done) => None,
                Ok(Outcome::Cancelled) => {
                    if kind == ModelTaskKind::Download {
                        // A cancelled download is forgotten, with its files.
                        let _ = self.update_state(|s| s.downloading = false);
                        let _ = std::fs::remove_dir_all(core_model::download_dir(&self.paths));
                    }
                    self.set_task(None);
                    return;
                }
                Err(message) => Some(message),
            };
            if kind == ModelTaskKind::Download {
                // Finished or failed: no automatic resume (a failure keeps the files for *Try again*).
                let _ = self.update_state(|s| s.downloading = false);
            }
            match &error {
                None => self.set_task(None),
                Some(message) => {
                    tracing::warn!(error = %message, kind = ?kind, "adding the search model failed");
                    let mut task = self.lock().task.clone();
                    if let Some(t) = task.as_mut() {
                        t.phase = ModelPhase::Failed;
                        t.error = Some(message.clone());
                    }
                    self.set_task(task);
                }
            }
            let done = SearchModelDone { kind, ok: error.is_none(), error };
            if let Err(e) = done.emit(&self.app) {
                tracing::warn!(error = %e, "failed to emit SearchModelDone");
            }
        });
        Ok(())
    }

    async fn download(self: &Arc<Self>, cancel: &Arc<Cancel>) -> Result<Outcome, String> {
        let pins = core_model::pins();
        let dest = core_model::download_dir(&self.paths);
        // Finished files and parts already there count; only the rest needs room.
        let have: u64 = pins
            .files
            .iter()
            .map(|f| {
                let done = dest.join(&f.path);
                let part = dest.join(format!("{}.part", f.path));
                std::fs::metadata(&done)
                    .or_else(|_| std::fs::metadata(&part))
                    .map_or(0, |m| m.len().min(f.size))
            })
            .sum();
        check_space(&self.paths, pins.total_size().saturating_sub(have))?;
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

        let base = download_base();
        let files: Vec<Value> = pins
            .files
            .iter()
            .map(|f| json!({"url": pins.url(&base, f), "path": f.path, "size": f.size, "sha256": f.sha256}))
            .collect();
        let params = json!({"dest_dir": dest, "files": files});
        let me = self.clone();
        let on_progress = Box::new(move |text: String| {
            if let Some(done) = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["done"].as_u64()) {
                me.progress(done);
            }
        });
        let sidecars = self.sidecars();
        let call = sidecars.call_streaming(Worker::Ai, "model.download", params, on_progress);
        // Dropping the call (on cancel) sends `cancel` to the worker; its files stay until deleted.
        let reply = tokio::select! {
            r = call => r,
            () = cancel.notify.notified() => return Ok(Outcome::Cancelled),
        };
        let reply = reply.map_err(message_of)?;
        if reply.get("cancelled").is_some() || cancel.is_set() {
            return Ok(Outcome::Cancelled);
        }

        self.set_phase(ModelPhase::Checking);
        self.install_from(dest, true, cancel).await
    }

    async fn import(self: &Arc<Self>, source: &Path, cancel: &Arc<Cancel>) -> Result<Outcome, String> {
        check_space(&self.paths, core_model::pins().total_size())?;
        self.install_from(source.to_path_buf(), false, cancel).await
    }

    fn set_phase(&self, phase: ModelPhase) {
        let task = self.lock().task.clone().map(|t| ModelTask { phase, done_bytes: 0, ..t });
        self.set_task(task);
    }

    /// Checks the files of `source` into a temporary folder, has the search worker load them, then
    /// puts them in place and switches search by meaning on.
    async fn install_from(self: &Arc<Self>, source: PathBuf, move_files: bool, cancel: &Arc<Cancel>) -> Result<Outcome, String> {
        let (me, paths, flag) = (self.clone(), self.paths.clone(), cancel.clone());
        let staged = tauri::async_runtime::spawn_blocking(move || {
            core_model::stage(&paths, &source, move_files, &mut |done| me.progress(done), &flag.flag)
        })
        .await
        .map_err(|e| e.to_string())?;
        if cancel.is_set() {
            return Ok(Outcome::Cancelled);
        }
        let staging = staged.map_err(|e| e.to_string())?;

        let sidecars = self.sidecars();
        let checked = sidecars.check_model(&staging).await;
        // The worker let go of the temporary folder's model (loaded under that path).
        sidecars.stop(Worker::Embed).await;
        if let Err(e) = checked {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("The search model couldn't be loaded: {}", message_of(e)));
        }
        if cancel.is_set() {
            let _ = std::fs::remove_dir_all(&staging);
            return Ok(Outcome::Cancelled);
        }
        let paths = self.paths.clone();
        let dir = tauri::async_runtime::spawn_blocking(move || core_model::activate(&paths, &staging))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        if move_files {
            let _ = std::fs::remove_dir_all(core_model::download_dir(&self.paths));
        }
        sidecars.set_model_dir(Some(dir));
        tracing::info!("search model added");
        if let Some(indexer) = self.app.try_state::<Arc<Indexer>>() {
            indexer.wake();
        }
        emit_changed(&self.app, &[Entity::Search]);
        Ok(Outcome::Done)
    }

    pub fn download_now(self: &Arc<Self>, accept_terms: bool) -> ApiResult<()> {
        self.ready_to_add()?;
        let state = core_model::load_state(&self.paths);
        if !accept_terms && !state.terms_accepted {
            return Err(invalid("The Gemma Terms of Use must be accepted before downloading."));
        }
        self.update_state(|s| {
            s.terms_accepted = true;
            s.downloading = true;
        })?;
        self.clone().begin(ModelTaskKind::Download, None)
    }

    pub fn import_now(self: &Arc<Self>, source: PathBuf) -> ApiResult<()> {
        self.ready_to_add()?;
        if !source.exists() {
            return Err(invalid(format!("{} doesn't exist.", source.display())));
        }
        self.clone().begin(ModelTaskKind::Import, Some(source))
    }

    fn ready_to_add(&self) -> ApiResult<()> {
        if self.install() != ModelInstall::NotInstalled {
            return Err(conflict("A search model is already installed."));
        }
        if self.running() {
            return Err(conflict("The search model is already being added."));
        }
        Ok(())
    }

    /// Stops a running download or import; clears a failed one's message.
    pub fn cancel(&self) {
        let cancel = self.lock().cancel.clone();
        match cancel {
            Some(c) => c.cancel(),
            None => {
                let failed_download = self
                    .lock()
                    .task
                    .as_ref()
                    .is_some_and(|t| t.kind == ModelTaskKind::Download);
                if failed_download {
                    let _ = std::fs::remove_dir_all(core_model::download_dir(&self.paths));
                }
                self.set_task(None);
            }
        }
    }

    /// Deletes an added model (a bundled one stays). The stored vectors are kept.
    pub async fn remove(&self) -> ApiResult<()> {
        if self.install() != ModelInstall::Added {
            return Err(invalid("Only a search model added in Settings can be removed."));
        }
        let sidecars = self.sidecars();
        sidecars.set_model_dir(None);
        sidecars.stop(Worker::Embed).await;
        let paths = self.paths.clone();
        let removed = tauri::async_runtime::spawn_blocking(move || core_model::remove(&paths))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?;
        if let Err(e) = removed {
            // Still there (e.g. a file in use on Windows): keep using it.
            let dir = core_model::added_dir(&self.paths);
            if core_model::is_complete(&dir) {
                sidecars.set_model_dir(Some(dir));
            }
            self.emit();
            return Err(e.into());
        }
        tracing::info!("search model removed");
        self.emit();
        emit_changed(&self.app, &[Entity::Search]);
        Ok(())
    }

    pub fn dismiss_notice(&self) -> ApiResult<()> {
        self.update_state(|s| s.notice_dismissed = true)?;
        self.emit();
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> ApiError {
    cairn_core::CoreError::Invalid(message.into()).into()
}

fn conflict(message: impl Into<String>) -> ApiError {
    cairn_core::CoreError::Conflict(message.into()).into()
}

/// Hugging Face, or in development builds `CAIRN_DEV_MODEL_URL` (a local test server).
fn download_base() -> String {
    if cfg!(debug_assertions)
        && let Ok(url) = std::env::var("CAIRN_DEV_MODEL_URL")
    {
        return url;
    }
    DOWNLOAD_BASE.into()
}

fn message_of(e: SidecarError) -> String {
    match e.hint {
        Some(h) if !h.is_empty() => format!("{} {h}", e.message),
        _ => e.message,
    }
}

/// Refuses to start when the data folder's disk has less room than `needed` plus a margin.
fn check_space(paths: &DataPaths, needed: u64) -> Result<(), String> {
    const MARGIN: u64 = 50 * 1024 * 1024;
    let Some(free) = free_space(&paths.root) else {
        return Ok(()); // unknown: a full disk still fails cleanly while writing
    };
    if free >= needed + MARGIN {
        return Ok(());
    }
    let mb = |b: u64| b.div_ceil(1024 * 1024);
    Err(format!(
        "There isn't enough free space for the search model: it needs about {} MB, and {} MB are free.",
        mb(needed + MARGIN),
        mb(free)
    ))
}

#[cfg(unix)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is a valid NUL-terminated string and `stats` is a plain C struct that
    // `statvfs` fills in.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    Some(stats.f_bavail as u64 * stats.f_frsize as u64)
}

#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL-terminated; the other outputs may be null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_is_measured_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        assert!(free_space(dir.path()).is_some_and(|b| b > 0));
        assert!(check_space(&paths, 1).is_ok());
        let err = check_space(&paths, u64::MAX / 4).unwrap_err();
        assert!(err.contains("isn't enough free space"), "{err}");
    }
}
