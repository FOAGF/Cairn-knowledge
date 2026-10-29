//! Export / import commands (FR-DATA-*).

use std::path::PathBuf;
use std::sync::Arc;

use cairn_core::export::{self, ExportSummary, ImportSummary};
use tauri::{AppHandle, State};

use crate::error::ApiResult;
use crate::events::{self, Entity};
use crate::knowledge_folder::KnowledgeFolder;
use crate::state::AppState;

/// Exports all data and attachments to the zip file at `path` (FR-DATA-1).
#[tauri::command]
#[specta::specta]
pub async fn export_data(state: State<'_, AppState>, path: String) -> ApiResult<ExportSummary> {
    let path = PathBuf::from(path);
    state.with_db(move |db| export::export_to(db, &path)).await
}

/// Replaces all data with the export at `path`, after backing up the current data (FR-DATA-3, -6).
/// An external-mode backup whose folder isn't on this computer is refused (FR-KN-EXT-14; see
/// `read_backup_info`, `import_data_with_root` and `import_tasks_only`). An external-mode
/// backup's folder is scanned and watched afterwards.
#[tauri::command]
#[specta::specta]
pub async fn import_data(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    path: String,
) -> ApiResult<ImportSummary> {
    let path = PathBuf::from(path);
    let summary = state
        .with_db_exclusive(move |db| export::import_from(db, &path))
        .await?;
    events::emit_changed(&app, &[Entity::All]);
    folder.restart_soon();
    Ok(summary)
}
