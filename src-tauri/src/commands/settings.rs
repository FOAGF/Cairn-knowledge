//! Settings commands (UI-SET-*).

use cairn_core::paths;
use cairn_core::settings::{self, Settings, SettingsPatch};
use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::sidecar::Sidecars;
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn get_settings(state: State<'_, AppState>) -> ApiResult<Settings> {
    state.with_db(|db| settings::get(db.conn())).await
}

/// Changes only the fields that are given and returns the new settings.
#[tauri::command]
#[specta::specta]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    patch: SettingsPatch,
) -> ApiResult<Settings> {
    let idle_changed = patch.embedding_idle_timeout_min.is_some();
    let s = state.with_db(move |db| settings::update(db, patch)).await?;
    if idle_changed {
        // FR-RAG-EM-1a: applies to both AI workers, also while they run.
        sidecars.set_idle_timeout(Duration::from_secs(
            u64::from(s.embedding_idle_timeout_min.max(1)) * 60,
        ));
    }
    emit_changed(&app, &[Entity::Settings]);
    Ok(s)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DataFolderInfo {
    /// The folder in use right now.
    pub current: String,
    /// The folder saved for the next start, if it differs from `current`.
    pub next_start: Option<String>,
    /// False in development builds, where config.json belongs to the real installation.
    pub can_change: bool,
}

/// Debug builds never read or write the real `config.json` (see `paths::resolve_data_dir`),
/// so changing the folder is simply unavailable there.
const CAN_CHANGE: bool = !cfg!(debug_assertions);

#[tauri::command]
#[specta::specta]
pub async fn get_data_folder(state: State<'_, AppState>) -> ApiResult<DataFolderInfo> {
    let current = state.paths.root.clone();
    let next_start = if CAN_CHANGE {
        paths::config_file()
            .map(|f| paths::read_config(&f))
            .and_then(|c| c.data_folder)
            .filter(|f| *f != current)
            .map(|f| f.display().to_string())
    } else {
        None
    };
    Ok(DataFolderInfo {
        current: current.display().to_string(),
        next_start,
        can_change: CAN_CHANGE,
    })
}

/// TC-9: saves the folder for the next start. Existing data is not moved.
#[tauri::command]
#[specta::specta]
pub async fn set_data_folder(app: AppHandle, folder: String) -> ApiResult<()> {
    if !CAN_CHANGE {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Changing the data folder is not available in development builds.".into(),
        });
    }
    let config = paths::config_file()
        .ok_or_else(|| ApiError::internal("no config folder on this system"))?;
    tauri::async_runtime::spawn_blocking(move || settings::set_data_folder(&config, &folder))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))??;
    emit_changed(&app, &[Entity::Settings]);
    Ok(())
}
