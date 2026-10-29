//! Backups and start-up notices (NFR-REL-2, NFR-REL-5, UI-SET-4, FR-DATA-8; M7 part A).

use std::path::Path;

use cairn_core::backup::{self, BackupFile, BackupKind, BackupStatus};
use cairn_core::export;
use cairn_core::settings::{self, Settings, SettingsPatch};
use tauri::{AppHandle, State};

use crate::backups;
use crate::error::{ApiError, ApiResult};
use crate::events::{self, Entity};
use crate::state::AppState;

/// The *Last backup* line: folder, last success, last failure, database check (UI-SET-4).
#[tauri::command]
#[specta::specta]
pub async fn backup_status(state: State<'_, AppState>) -> ApiResult<BackupStatus> {
    state.with_db(|db| backup::status(db)).await
}

/// *Back up now*: a manual backup, kept until the user deletes it (NFR-REL-2).
#[tauri::command]
#[specta::specta]
pub async fn backup_now(app: AppHandle, state: State<'_, AppState>) -> ApiResult<BackupFile> {
    let db = state.db_handle().ok_or_else(|| ApiError {
        kind: "database".into(),
        message: "The database could not be opened.".into(),
    })?;
    tauri::async_runtime::spawn_blocking(move || backups::make(&app, &db, BackupKind::Manual))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(ApiError::from)
}

/// Cairn's backups in the backup folder, newest first (UI-SET-4's list).
#[tauri::command]
#[specta::specta]
pub async fn list_backups(state: State<'_, AppState>) -> ApiResult<Vec<BackupFile>> {
    state.with_db(|db| backup::list(db)).await
}

/// *Choose folder…*: checks that Cairn can write to `folder`, then saves it. `None` goes back to
/// the data folder's `backups` folder.
#[tauri::command]
#[specta::specta]
pub async fn set_backup_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: Option<String>,
) -> ApiResult<Settings> {
    let s = state
        .with_db(move |db| {
            let folder = folder.map(|f| f.trim().to_string()).unwrap_or_default();
            if !folder.is_empty() {
                backup::check_folder(Path::new(&folder))?;
            }
            let s = settings::update(
                db,
                SettingsPatch {
                    backup_folder: Some(folder),
                    ..Default::default()
                },
            )?;
            // Today's backup goes to the new folder soon instead of waiting for tomorrow.
            backup::folder_changed(db)?;
            Ok(s)
        })
        .await?;
    events::emit_changed(&app, &[Entity::Settings, Entity::Backups]);
    Ok(s)
}

/// A name for a *Save as* dialog that is valid on Windows and Linux (D-58); `fallback` is used
/// when nothing is left of `name`.
#[tauri::command]
#[specta::specta]
pub async fn safe_file_name(name: String, fallback: String) -> ApiResult<String> {
    Ok(cairn_core::util::safe_file_name(&name, &fallback))
}

/// Messages about what start-up did (e.g. an interrupted import was undone, NFR-REL-5). Each is
/// returned once.
#[tauri::command]
#[specta::specta]
pub async fn take_startup_notices(state: State<'_, AppState>) -> ApiResult<Vec<String>> {
    Ok(state.take_startup_notices())
}

/// *Demo data › Reset* (development builds only, UI-SET-4): replaces all data with an empty
/// database; the UI then adds the samples again. Fails in a release build.
#[tauri::command]
#[specta::specta]
pub async fn dev_reset_data(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: State<'_, std::sync::Arc<crate::knowledge_folder::KnowledgeFolder>>,
) -> ApiResult<()> {
    if !cfg!(debug_assertions) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Demo data can only be reset in a development build.".into(),
        });
    }
    state.with_db_exclusive(export::reset_to_empty).await?;
    events::emit_changed(&app, &[Entity::All]);
    // The reset data is internal (demo data): the folder watcher stops.
    folder.restart_soon();
    Ok(())
}
