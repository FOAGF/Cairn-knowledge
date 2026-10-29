//! Shared app state: the open database. Commands reach it through `with_db`.

use std::sync::{Arc, Mutex};

use cairn_core::secrets::SecretStore;
use cairn_core::startup;
use cairn_core::{DataPaths, Db};

use crate::error::{ApiError, ApiResult};

pub struct AppState {
    /// `Err` holds the reason the database could not be opened; the UI still starts and shows it.
    db: Result<Arc<Mutex<Db>>, String>,
    pub paths: DataPaths,
    /// API keys (NFR-SEC-1). Clone the `Arc` into `with_db` closures that need it.
    pub secrets: Arc<dyn SecretStore>,
    /// What start-up did that the user should hear about (NFR-REL-5); taken once by the UI.
    startup_notices: Mutex<Vec<String>>,
}

impl AppState {
    pub fn open(paths: DataPaths, secrets: Arc<dyn SecretStore>) -> Self {
        let mut notices = Vec::new();
        // NFR-REL-5: an import that stopped halfway is undone before the database opens.
        match startup::recover_interrupted_import(&paths) {
            Ok(true) => notices.push(startup::IMPORT_INTERRUPTED.to_string()),
            Ok(false) => {}
            Err(e) => {
                tracing::error!(error = %e, "could not put back the data from before an import");
                notices.push(startup::IMPORT_NOT_RESTORED.to_string());
            }
        }
        let db = Db::open(paths.clone())
            .map(|mut db| {
                // D-72, development builds: CAIRN_DEV_KNOWLEDGE_MODE (`internal` or
                // `external=<folder>`) presets the mode of a fresh data folder, so scripted
                // probes skip the full-screen choice.
                if cfg!(debug_assertions)
                    && let Ok(value) = std::env::var("CAIRN_DEV_KNOWLEDGE_MODE")
                {
                    match cairn_core::knowledge::mode::dev_preset(&mut db, &value) {
                        Ok(true) => tracing::info!("knowledge mode preset (CAIRN_DEV_KNOWLEDGE_MODE)"),
                        Ok(false) => {}
                        Err(e) => tracing::warn!(error = %e, "CAIRN_DEV_KNOWLEDGE_MODE was not used"),
                    }
                }
                // D-52: Cairn's half-made files (unfinished imports, `*.part`) are deleted.
                let folder = cairn_core::backup::backup_folder(&db).unwrap_or_else(|_| paths.backups_dir());
                if let Err(e) = startup::sweep_leftovers(&paths, &[folder]) {
                    tracing::warn!(error = %e, "removing leftovers at startup failed");
                }
                // FR-KN-ED-11: remove images nothing refers to any more. Failures only get logged.
                match cairn_core::attachments::collect_garbage(&mut db) {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(removed = n, "removed unreferenced images"),
                    Err(e) => tracing::warn!(error = %e, "image cleanup at startup failed"),
                }
                Arc::new(Mutex::new(db))
            })
            .map_err(|e| {
            tracing::error!(error = %e, data_dir = %paths.root.display(), "could not open the database");
            e.to_string()
        });
        Self {
            db,
            paths,
            secrets,
            startup_notices: Mutex::new(notices),
        }
    }

    /// The start-up messages, once (NFR-REL-5).
    pub fn take_startup_notices(&self) -> Vec<String> {
        self.startup_notices
            .lock()
            .map(|mut n| std::mem::take(&mut *n))
            .unwrap_or_default()
    }

    /// The AI workers' idle timeout from settings (FR-RAG-EM-1a); 5 minutes if it can't be read.
    /// Only for startup: commands use `with_db`.
    pub fn idle_timeout_min(&self) -> u32 {
        self.db
            .as_ref()
            .ok()
            .and_then(|db| db.lock().ok())
            .and_then(|db| cairn_core::settings::get(db.conn()).ok())
            .map_or(5, |s| s.embedding_idle_timeout_min)
    }

    /// The external knowledge root, if the database is in external mode (D-72). Only for
    /// start-up: commands use `with_db`.
    pub fn knowledge_root(&self) -> Option<String> {
        self.db
            .as_ref()
            .ok()
            .and_then(|db| db.lock().ok())
            .and_then(|db| cairn_core::knowledge::mode::storage(db.conn()).ok())
            .and_then(|s| s.root)
    }

    /// The open database, for work that must outlive a future that gets dropped (*Stop* on a chat
    /// job writes its result from a `Drop`). `None` when the database could not be opened.
    pub fn db_handle(&self) -> Option<Arc<Mutex<Db>>> {
        self.db.as_ref().ok().cloned()
    }

    /// Like [`Self::with_db`], but no backup runs at the same time: for work that swaps the data
    /// files (an import, the dev reset). It waits for a backup in progress first.
    pub async fn with_db_exclusive<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Db) -> cairn_core::Result<T> + Send + 'static,
    {
        let db = self.db_handle().ok_or_else(|| ApiError {
            kind: "database".into(),
            message: "The database could not be opened.".into(),
        })?;
        tauri::async_runtime::spawn_blocking(move || {
            // The backup lock first, then the database lock: the order `backups::make` uses.
            let _no_backup = crate::backups::exclusive();
            let mut guard = db
                .lock()
                .map_err(|_| ApiError::internal("database lock poisoned"))?;
            f(&mut guard).map_err(ApiError::from)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    }

    /// Runs `f` with the database on a blocking thread, so the UI thread never waits (spec TC-15).
    pub async fn with_db<T, F>(&self, f: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Db) -> cairn_core::Result<T> + Send + 'static,
    {
        let db = self
            .db
            .as_ref()
            .map_err(|e| ApiError {
                kind: "database".into(),
                message: format!("The database could not be opened: {e}"),
            })?
            .clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut guard = db
                .lock()
                .map_err(|_| ApiError::internal("database lock poisoned"))?;
            f(&mut guard).map_err(ApiError::from)
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
    }
}
