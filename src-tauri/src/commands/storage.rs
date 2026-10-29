//! Where knowledge lives (spec §5.11, D-72): the mode and its folder, the first-start choice,
//! switching, rescanning, the switch backups, importing an external-mode backup, the bulk bar's
//! name check, pasted images and the save conflict. Thin wrappers over
//! `cairn_core::knowledge::{mode, external}`; the background work is `crate::knowledge_folder`.

use std::path::PathBuf;
use std::sync::Arc;

use cairn_core::export::{self, ImportSummary};
use cairn_core::knowledge::mode::{self, BackupInfo, KnowledgeStorage, SwitchBackups};
use cairn_core::knowledge::{self, NameClash, Page, PageBulkChange, SavedImage};
use cairn_core::settings::KnowledgeMode;
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::knowledge_folder::KnowledgeFolder;
use crate::state::AppState;

/// The mode (unset until the first-start choice), the root folder, whether it is there, and why
/// it isn't watched when the watcher couldn't start.
#[tauri::command]
#[specta::specta]
pub async fn get_knowledge_storage(
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
) -> ApiResult<KnowledgeStorage> {
    let mut storage = state.with_db(|db| mode::storage(db.conn())).await?;
    if storage.mode == Some(KnowledgeMode::External) {
        storage.watch_error = folder.watch_error();
    }
    Ok(storage)
}

/// FR-KN-EXT-7: the full-screen choice on a new data folder. `root` is required for external
/// mode. In external mode the folder is then scanned in the background (`KnowledgeProgress`
/// events with `task: scan`) and watched.
#[tauri::command]
#[specta::specta]
pub async fn choose_knowledge_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    mode: KnowledgeMode,
    root: Option<String>,
) -> ApiResult<KnowledgeStorage> {
    let storage = state
        .with_db(move |db| mode::choose_mode(db, mode, root.as_deref()))
        .await?;
    emit_changed(&app, &[Entity::Settings]);
    folder.restart_soon();
    Ok(storage)
}

/// FR-KN-EXT-8, internal → external: checks the folder, then switches in the background and
/// returns at once. Progress arrives as `KnowledgeProgress` events (`task: switch`); the last one
/// has `finished` and either `error` or `switched`; then `DataChanged { all }`.
#[tauri::command]
#[specta::specta]
pub async fn switch_knowledge_to_external(
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    root: String,
) -> ApiResult<()> {
    let root = state
        .with_db(move |db| {
            if knowledge::external::is_external(db.conn())? {
                return Err(cairn_core::CoreError::Invalid(
                    "Knowledge is already kept in a folder.".into(),
                ));
            }
            mode::check_root(db, &root)
        })
        .await?;
    tauri::async_runtime::spawn(folder.inner().clone().switch_to_external(root));
    Ok(())
}

/// Another knowledge folder while in external mode: as switching to internal mode and back to
/// `root`, in one switch with one switch backup (of the old folder's index). Checks the folder
/// (refused when not in external mode, when it is the folder in use, or when it fails the checks
/// of `switch_knowledge_to_external`), then switches in the background like
/// `switch_knowledge_to_external`: `KnowledgeProgress` events (`task: switch`), then
/// `DataChanged { all }`. Neither folder's files are changed.
#[tauri::command]
#[specta::specta]
pub async fn switch_knowledge_folder(
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    root: String,
) -> ApiResult<()> {
    let root = state
        .with_db(move |db| mode::check_new_root(db, &root))
        .await?;
    tauri::async_runtime::spawn(folder.inner().clone().switch_folder(root));
    Ok(())
}

/// FR-KN-EXT-8, external → internal: `backup` is the file name of a switch backup made in internal
/// mode whose knowledge is loaded, or none to start with no pages. One made in external mode is
/// refused here (kind `invalid`), before anything starts. Runs in the background like
/// `switch_knowledge_to_external`.
#[tauri::command]
#[specta::specta]
pub async fn switch_knowledge_to_internal(
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    backup: Option<String>,
) -> ApiResult<()> {
    let chosen = backup.clone();
    let is_external = state
        .with_db(move |db| {
            // Checked here, so a refusal comes back from the command before anything starts.
            if let Some(name) = chosen.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                mode::check_internal_backup(db, name)?;
            }
            knowledge::external::is_external(db.conn())
        })
        .await?;
    if !is_external {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Knowledge is already kept in Cairn.".into(),
        });
    }
    tauri::async_runtime::spawn(folder.inner().clone().switch_to_internal(backup));
    Ok(())
}

/// *Rescan*: reads the whole folder again in the background (`KnowledgeProgress`, `task: scan`),
/// first trying the watcher again if it couldn't start before.
#[tauri::command]
#[specta::specta]
pub async fn rescan_knowledge_folder(
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
) -> ApiResult<()> {
    let is_external = state
        .with_db(|db| knowledge::external::is_external(db.conn()))
        .await?;
    if !is_external {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Knowledge is not kept in a folder.".into(),
        });
    }
    let folder = folder.inner().clone();
    tauri::async_runtime::spawn(async move { folder.rescan().await });
    Ok(())
}

/// FR-KN-EXT-9: the switch backups (newest first), their total size and the warning size. The
/// start-up pop-up shows when `overWarning` is set.
#[tauri::command]
#[specta::specta]
pub async fn list_switch_backups(state: State<'_, AppState>) -> ApiResult<SwitchBackups> {
    state.with_db(|db| mode::switch_backups(db)).await
}

/// *Delete* in the switch backup list; returns the list as it is then.
#[tauri::command]
#[specta::specta]
pub async fn delete_switch_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    file_name: String,
) -> ApiResult<SwitchBackups> {
    let list = state
        .with_db(move |db| {
            mode::delete_switch_backup(db, &file_name)?;
            mode::switch_backups(db)
        })
        .await?;
    emit_changed(&app, &[Entity::Backups]);
    Ok(list)
}

/// FR-KN-EXT-10: the file names a bulk change would give twice in one folder (external mode;
/// always empty in internal mode). The bulk bar lists them and locks its button.
#[tauri::command]
#[specta::specta]
pub async fn plan_page_bulk_change(
    state: State<'_, AppState>,
    ids: Vec<String>,
    change: PageBulkChange,
) -> ApiResult<Vec<NameClash>> {
    state
        .with_db(move |db| knowledge::plan_bulk_change(db.conn(), &ids, &change))
        .await
}

/// FR-KN-EXT-14: what a backup file says before it is imported: its mode, its root folder and
/// whether that folder is on this computer. A knowledge-only switch backup is refused (kind
/// `invalid`, the same message as the imports): it is loaded by switching to internal mode.
#[tauri::command]
#[specta::specta]
pub async fn read_backup_info(path: String) -> ApiResult<BackupInfo> {
    tauri::async_runtime::spawn_blocking(move || mode::backup_info(&PathBuf::from(path)))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(ApiError::from)
}

/// FR-KN-EXT-14: imports an external-mode backup with its knowledge in `root` (where the user
/// says the folder is now), replacing all data after a backup; the folder is then scanned.
#[tauri::command]
#[specta::specta]
pub async fn import_data_with_root(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    path: String,
    root: String,
) -> ApiResult<ImportSummary> {
    let path = PathBuf::from(path);
    let summary = state
        .with_db_exclusive(move |db| export::import_with_root(db, &path, &root))
        .await?;
    emit_changed(&app, &[Entity::All]);
    folder.restart_soon();
    Ok(summary)
}

/// FR-KN-EXT-14: imports only the tasks (with their comments, history, links, labels and images)
/// and the categories of a backup. This computer's settings and AI providers stay; pages, glossary
/// and conversations end up empty (after the usual backup); Cairn is then in internal mode.
#[tauri::command]
#[specta::specta]
pub async fn import_tasks_only(
    app: AppHandle,
    state: State<'_, AppState>,
    folder: State<'_, Arc<KnowledgeFolder>>,
    path: String,
) -> ApiResult<ImportSummary> {
    let path = PathBuf::from(path);
    let summary = state
        .with_db_exclusive(move |db| export::import_tasks_only(db, &path))
        .await?;
    emit_changed(&app, &[Entity::All]);
    folder.restart_soon();
    Ok(summary)
}

/// A pasted or dropped image for a page of `categoryId` (FR-KN-ED-3, FR-KN-EXT-3), in either
/// mode: `target` goes into the Markdown (`attachment:<name>` in internal mode,
/// `folder:<category folder>/attachments/<name>` in external mode); `file` is shown with
/// `convertFileSrc`. `dataBase64` is the file content in base64.
#[tauri::command]
#[specta::specta]
pub async fn save_page_image(
    state: State<'_, AppState>,
    category_id: String,
    data_base64: String,
    mime: String,
) -> ApiResult<SavedImage> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| ApiError {
            kind: "invalid".into(),
            message: format!("The image data is not valid: {e}"),
        })?;
    state
        .with_db(move |db| knowledge::save_image(db, &category_id, &bytes, &mime))
        .await
}

/// FR-KN-EXT-11, *Show differences*: the page as its file says now (the index is not changed);
/// `null` when the file is gone.
#[tauri::command]
#[specta::specta]
pub async fn get_page_on_disk(state: State<'_, AppState>, id: String) -> ApiResult<Option<Page>> {
    state.with_db(move |db| knowledge::page_on_disk(db, &id)).await
}

/// FR-KN-EXT-11, *Take the disk version*: reads the page's file into the index now.
#[tauri::command]
#[specta::specta]
pub async fn reload_page_from_disk(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::reload_page(db, &id))
        .await?;
    emit_changed(
        &app,
        &[Entity::Knowledge, Entity::Activity, Entity::Mentions, Entity::Search],
    );
    Ok(page)
}

/// Development builds only: a copy of the sample external folder next to the data folder, for the
/// mode chooser (FR-KN-EXT-7). Returns its path, to pass as the root.
#[tauri::command]
#[specta::specta]
pub async fn dev_sample_knowledge_folder(state: State<'_, AppState>) -> ApiResult<String> {
    if !cfg!(debug_assertions) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "The sample knowledge folder is only available in development builds.".into(),
        });
    }
    state
        .with_db(|db| mode::dev_copy_sample(db).map(|p| p.to_string_lossy().into_owned()))
        .await
}
