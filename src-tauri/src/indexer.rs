//! Background indexing (FR-RAG-IX-2, -5): fills the vectors of waiting pieces.
//!
//! - Woken by [`Indexer::wake`] after any change to pages or tasks (`events::emit_changed` calls it
//!   for the `tasks`, `knowledge` and `all` entities), and once shortly after startup.
//! - Loop: take a small batch with `cairn_core::vectors::next_jobs` (under the database lock),
//!   embed it with `Sidecars::embed_documents` (outside the lock), save it with `store_vectors`;
//!   repeat until nothing is waiting, then sleep until woken.
//! - At startup it calls `vectors::ensure_model` with the sidecar's model id and dims (a different
//!   model empties every vector, FR-RAG-EM-2).
//! - Without a search model (a lite package before one is added, D-82) it does nothing: the stored
//!   vectors stay as they are, and adding a model wakes it.
//! - If the sidecar fails, it records the error, backs off (about 30 s, then longer) and tries
//!   again; search keeps working by keyword. Nothing is embedded (and no process is started)
//!   while no piece is waiting.
//! - Progress is sent to the UI as the `IndexProgress` event, at most about once a second, and
//!   once more at the end of a run. Open search results refresh once per run (`DataChanged { search }`).
//!
//! Owned by the M4 sidecar-link agent.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cairn_core::vectors::{self, EmbedJob, IndexModel, IndexStatus};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use tokio::sync::Notify;

use crate::error::ApiError;
use crate::events::{Entity, emit_changed};
use crate::sidecar::{EMBED_DIMS, EMBED_MODEL_ID, SidecarError, Sidecars};
use crate::state::AppState;

/// Pieces per request. Small, so a search never waits long behind indexing (PROTOCOL.md).
const BATCH: u32 = 8;
/// Indexing starts a little after the window opens, so startup stays fast (NFR-PERF-2).
const STARTUP_DELAY: Duration = Duration::from_secs(7);
/// The UI gets at most one progress event per this long while running.
const EVENT_EVERY: Duration = Duration::from_secs(1);
/// First wait after a failure; doubles each time up to `MAX_BACKOFF`.
const FIRST_BACKOFF: Duration = Duration::from_secs(30);
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);

/// Settings › Search and the "index not ready" warnings (FR-RAG-IX-5, -6).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub status: IndexStatus,
    /// True while the indexer is working through waiting pieces.
    pub running: bool,
    /// The last indexing error, in plain language (cleared after a successful batch).
    pub error: Option<String>,
}

/// Managed Tauri state: `app.state::<Arc<Indexer>>()`.
pub struct Indexer {
    app: AppHandle,
    /// Wakes the loop; a wake while it is busy is remembered, so the next pass follows at once.
    notify: Notify,
    progress: Mutex<IndexProgress>,
    last_event: Mutex<Option<Instant>>,
}

impl Indexer {
    /// Creates the indexer and spawns its loop on the async runtime.
    pub fn start(app: AppHandle) -> Arc<Self> {
        let indexer = Arc::new(Self {
            app,
            notify: Notify::new(),
            progress: Mutex::new(IndexProgress::default()),
            last_event: Mutex::new(None),
        });
        tauri::async_runtime::spawn(indexer.clone().run());
        indexer
    }

    /// Something changed: look for waiting pieces soon. Cheap; safe to call often.
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    pub fn progress(&self) -> IndexProgress {
        self.progress
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Stores a fresh count from the database (Settings › Search reads it on opening) and returns
    /// the progress including it.
    pub fn with_status(&self, status: IndexStatus) -> IndexProgress {
        self.update(|p| p.status = status);
        self.progress()
    }

    fn update(&self, change: impl FnOnce(&mut IndexProgress)) {
        change(&mut self.progress.lock().unwrap_or_else(|e| e.into_inner()));
    }

    /// Sends the progress to the UI; while running, at most once per `EVENT_EVERY` unless `force`.
    fn emit_progress(&self, force: bool) {
        {
            let mut last = self.last_event.lock().unwrap_or_else(|e| e.into_inner());
            if !force && last.is_some_and(|t| t.elapsed() < EVENT_EVERY) {
                return;
            }
            *last = Some(Instant::now());
        }
        if let Err(e) = self.progress().emit(&self.app) {
            tracing::warn!(error = %e, "failed to emit IndexProgress");
        }
    }

    async fn run(self: Arc<Self>) {
        tokio::time::sleep(STARTUP_DELAY).await;
        let mut failures = 0u32;
        loop {
            match self.run_once().await {
                Ok(()) => {
                    failures = 0;
                    // Sleeps until something changes (a wake during the run is remembered).
                    self.notify.notified().await;
                }
                Err(message) => {
                    failures += 1;
                    let wait = backoff(failures);
                    tracing::warn!(error = %message, retry_in_s = wait.as_secs(), "indexing stopped");
                    self.update(|p| {
                        p.running = false;
                        p.error = Some(message);
                    });
                    self.emit_progress(true);
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    /// One pass: record the model, then embed waiting pieces until none are left.
    async fn run_once(&self) -> Result<(), String> {
        let state = self.app.state::<AppState>();
        let sidecars = self.app.state::<Sidecars>();

        if !sidecars.model_installed() {
            // D-82: nothing to index with. `ensure_model` isn't called, so the vectors are kept.
            let status = state
                .with_db(|db| vectors::status(db.conn()))
                .await
                .map_err(api_message)?;
            self.update(|p| {
                p.status = status;
                p.running = false;
                p.error = None;
            });
            self.emit_progress(true);
            return Ok(());
        }

        let status = state
            .with_db(|db| {
                let model = IndexModel {
                    id: EMBED_MODEL_ID.into(),
                    dims: EMBED_DIMS,
                };
                vectors::ensure_model(db.conn(), &model)?;
                vectors::status(db.conn())
            })
            .await
            .map_err(api_message)?;
        self.update(|p| p.status = status);

        let mut embedded_any = false;
        let mut empty_saves = 0;
        loop {
            let jobs = state
                .with_db(|db| vectors::next_jobs(db.conn(), BATCH))
                .await
                .map_err(api_message)?;
            if jobs.is_empty() {
                break;
            }
            self.update(|p| p.running = true);
            self.emit_progress(false);

            // The model works outside the database lock, so saving a page is never held up.
            let items = jobs
                .iter()
                .map(|j| (j.title.clone(), j.text.clone()))
                .collect();
            let embedded = sidecars
                .embed_documents(items)
                .await
                .map_err(sidecar_message)?;
            let done: Vec<(EmbedJob, Vec<f32>)> = jobs.into_iter().zip(embedded).collect();
            let (stored, status) = state
                .with_db(move |db| {
                    let stored = vectors::store_vectors(db.conn(), &done)?;
                    Ok((stored, vectors::status(db.conn())?))
                })
                .await
                .map_err(api_message)?;
            embedded_any = true;
            self.update(|p| {
                p.status = status;
                p.error = None;
            });
            // Pieces edited while embedding are skipped and come round again. Only a long run of
            // batches that save nothing means something is wrong; then stop instead of spinning.
            empty_saves = if stored == 0 { empty_saves + 1 } else { 0 };
            if empty_saves >= 3 {
                return Err("Indexing keeps skipping pieces that changed while it worked.".into());
            }
        }

        self.update(|p| {
            p.running = false;
            p.error = None;
        });
        self.emit_progress(true);
        if embedded_any {
            // Open search results may now include more meaning-based hits.
            emit_changed(&self.app, &[Entity::Search]);
        }
        Ok(())
    }
}

/// 30 s after the first failure, doubling up to 15 minutes.
fn backoff(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(10);
    (FIRST_BACKOFF * 2u32.pow(doublings)).min(MAX_BACKOFF)
}

fn api_message(e: ApiError) -> String {
    e.message
}

fn sidecar_message(e: SidecarError) -> String {
    match e.hint {
        Some(h) if !h.is_empty() => format!("{} ({h})", e.message),
        _ => e.message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_stops_at_the_maximum() {
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(2), Duration::from_secs(60));
        assert_eq!(backoff(3), Duration::from_secs(120));
        assert_eq!(backoff(50), MAX_BACKOFF);
    }
}
