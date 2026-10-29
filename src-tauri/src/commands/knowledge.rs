//! Knowledge commands (spec §5): groups, pages, history, task links, backlinks, images, drafts,
//! Markdown export, dev seed. Thin wrappers: the rules live in `cairn_core::knowledge` and friends.

use std::path::PathBuf;

use cairn_core::bulk::BulkResult;
use cairn_core::knowledge::{
    self, Attachment, Backlink, CheckStatus, Draft, ExportPlan, FolderExportInput,
    FolderExportResult, FolderImportInput, FolderImportResult, Group, GroupPatch, ImportPlan,
    KnowledgeSeed, MarkdownExportMode, MentionTarget, NewGroup, NewPage, Page, PageBulkChange,
    PageExport, PageFilter, PagePatch, PageSummary, PageVersion, TaskPageLink,
};
use cairn_core::{attachments, drafts, mentions};
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::state::AppState;

/// A page or group change: pages, activity, mentions and search results may all differ.
const CHANGED: &[Entity] = &[
    Entity::Knowledge,
    Entity::Activity,
    Entity::Mentions,
    Entity::Search,
];

// ---------------------------------------------------------------- groups

#[tauri::command]
#[specta::specta]
pub async fn list_groups(
    state: State<'_, AppState>,
    category_id: Option<String>,
) -> ApiResult<Vec<Group>> {
    state
        .with_db(move |db| knowledge::groups(db.conn(), category_id.as_deref()))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn create_group(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewGroup,
) -> ApiResult<Group> {
    let group = state
        .with_db(move |db| knowledge::create_group(db, input))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(group)
}

#[tauri::command]
#[specta::specta]
pub async fn update_group(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: GroupPatch,
) -> ApiResult<Group> {
    let group = state
        .with_db(move |db| knowledge::update_group(db, &id, patch))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(group)
}

/// Its pages become ungrouped.
#[tauri::command]
#[specta::specta]
pub async fn delete_group(app: AppHandle, state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state
        .with_db(move |db| knowledge::delete_group(db, &id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(())
}

// ---------------------------------------------------------------- pages

#[tauri::command]
#[specta::specta]
pub async fn list_pages(
    state: State<'_, AppState>,
    filter: PageFilter,
) -> ApiResult<Vec<PageSummary>> {
    state
        .with_db(move |db| knowledge::list_pages(db.conn(), &filter))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_page(state: State<'_, AppState>, id: String) -> ApiResult<Page> {
    state
        .with_db(move |db| knowledge::get_page(db.conn(), &id))
        .await
}

/// For page mention chips; unknown ids are skipped.
#[tauri::command]
#[specta::specta]
pub async fn get_pages_by_ids(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> ApiResult<Vec<PageSummary>> {
    state
        .with_db(move |db| knowledge::get_pages_by_ids(db.conn(), &ids))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_page_tags(state: State<'_, AppState>) -> ApiResult<Vec<String>> {
    state.with_db(move |db| knowledge::tags(db.conn())).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_page(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewPage,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::create_page(db, input))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(page)
}

#[tauri::command]
#[specta::specta]
pub async fn update_page(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: PagePatch,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::update_page(db, &id, patch))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(page)
}

#[tauri::command]
#[specta::specta]
pub async fn move_page(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    category_id: String,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::move_page(db, &id, &category_id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(page)
}

#[tauri::command]
#[specta::specta]
pub async fn archive_page(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::set_archived(db, &id, archived))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(page)
}

#[tauri::command]
#[specta::specta]
pub async fn set_page_check_status(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    status: CheckStatus,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::set_check_status(db, &id, status))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(page)
}

/// Permanent (FR-KN-ED-12). The UI asks for confirmation first.
#[tauri::command]
#[specta::specta]
pub async fn delete_page(app: AppHandle, state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state
        .with_db(move |db| knowledge::delete_page(db, &id))
        .await?;
    emit_changed(
        &app,
        &[
            Entity::Knowledge,
            Entity::Activity,
            Entity::Mentions,
            Entity::Search,
            Entity::Tasks,
        ],
    );
    Ok(())
}

/// One change for many pages (FR-KN-UI-3). The UI asks for confirmation first.
#[tauri::command]
#[specta::specta]
pub async fn update_pages(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    change: PageBulkChange,
) -> ApiResult<BulkResult> {
    let out = state
        .with_db(move |db| knowledge::update_many(db, &ids, change))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(out)
}

/// Permanent, as `delete_page` (FR-KN-UI-3). The UI asks for confirmation first.
#[tauri::command]
#[specta::specta]
pub async fn delete_pages(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> ApiResult<BulkResult> {
    let out = state
        .with_db(move |db| knowledge::delete_many(db, &ids))
        .await?;
    emit_changed(
        &app,
        &[
            Entity::Knowledge,
            Entity::Activity,
            Entity::Mentions,
            Entity::Search,
            Entity::Tasks,
        ],
    );
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub async fn list_page_versions(
    state: State<'_, AppState>,
    page_id: String,
) -> ApiResult<Vec<PageVersion>> {
    state
        .with_db(move |db| knowledge::versions(db.conn(), &page_id))
        .await
}

/// Saved as a new change (FR-KN-ED-9).
#[tauri::command]
#[specta::specta]
pub async fn restore_page_version(
    app: AppHandle,
    state: State<'_, AppState>,
    page_id: String,
    date: String,
) -> ApiResult<Page> {
    let page = state
        .with_db(move |db| knowledge::restore_version(db, &page_id, &date))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(page)
}

// ---------------------------------------------------------------- task ↔ page links, backlinks

#[tauri::command]
#[specta::specta]
pub async fn list_task_page_links(
    state: State<'_, AppState>,
    task_id: Option<String>,
    page_id: Option<String>,
) -> ApiResult<Vec<TaskPageLink>> {
    state
        .with_db(move |db| {
            knowledge::task_page_links(db.conn(), task_id.as_deref(), page_id.as_deref())
        })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn link_task_page(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    page_id: String,
) -> ApiResult<TaskPageLink> {
    let link = state
        .with_db(move |db| knowledge::link_task_page(db, &task_id, &page_id))
        .await?;
    emit_changed(&app, &[Entity::Knowledge]);
    Ok(link)
}

#[tauri::command]
#[specta::specta]
pub async fn unlink_task_page(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    state
        .with_db(move |db| knowledge::unlink_task_page(db, &id))
        .await?;
    emit_changed(&app, &[Entity::Knowledge]);
    Ok(())
}

/// "Mentioned in" (DM-MN-4).
#[tauri::command]
#[specta::specta]
pub async fn list_backlinks(
    state: State<'_, AppState>,
    target: MentionTarget,
    id: String,
) -> ApiResult<Vec<Backlink>> {
    state
        .with_db(move |db| mentions::backlinks(db.conn(), target, &id))
        .await
}

// ---------------------------------------------------------------- images (FR-KN-ED-3)

/// Stores a pasted or dropped image. `dataBase64` is the file content in base64.
#[tauri::command]
#[specta::specta]
pub async fn save_attachment(
    state: State<'_, AppState>,
    data_base64: String,
    mime: String,
) -> ApiResult<Attachment> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| ApiError {
            kind: "invalid".into(),
            message: format!("The image data is not valid: {e}"),
        })?;
    state
        .with_db(move |db| attachments::save(db, &bytes, &mime))
        .await
}

/// Absolute folder of the image files. The UI shows `attachment:<name>` with
/// `convertFileSrc(<dir>/<name>)` (Tauri asset protocol, allowed for this folder at startup).
#[tauri::command]
#[specta::specta]
pub async fn attachments_dir(state: State<'_, AppState>) -> ApiResult<String> {
    Ok(state.paths.attachments_dir().to_string_lossy().into_owned())
}

// ---------------------------------------------------------------- drafts (FR-KN-ED-10)

#[tauri::command]
#[specta::specta]
pub async fn get_draft(
    state: State<'_, AppState>,
    kind: String,
    ref_id: String,
) -> ApiResult<Option<Draft>> {
    state
        .with_db(move |db| drafts::get(db.conn(), &kind, &ref_id))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_drafts(state: State<'_, AppState>, kind: String) -> ApiResult<Vec<Draft>> {
    state
        .with_db(move |db| drafts::list(db.conn(), &kind))
        .await
}

/// No event: drafts are only read when an editor opens.
#[tauri::command]
#[specta::specta]
pub async fn save_draft(
    state: State<'_, AppState>,
    kind: String,
    ref_id: String,
    content: String,
) -> ApiResult<Draft> {
    state
        .with_db(move |db| drafts::save(db.conn(), &kind, &ref_id, &content))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn delete_draft(
    state: State<'_, AppState>,
    kind: String,
    ref_id: String,
) -> ApiResult<()> {
    state
        .with_db(move |db| drafts::delete(db.conn(), &kind, &ref_id))
        .await
}

// ---------------------------------------------------------------- export (TC-8a)

#[tauri::command]
#[specta::specta]
pub async fn export_page_markdown(
    state: State<'_, AppState>,
    page_id: String,
    path: String,
    mode: MarkdownExportMode,
) -> ApiResult<PageExport> {
    state
        .with_db(move |db| knowledge::export_markdown(db, &page_id, &PathBuf::from(path), mode))
        .await
}

// ---------------------------------------------------------------- Markdown folders (FR-KN-MD-1..4)

/// What importing the folder would do: pages created, replaced or skipped. Nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn plan_markdown_import(
    state: State<'_, AppState>,
    input: FolderImportInput,
) -> ApiResult<ImportPlan> {
    state
        .with_db(move |db| knowledge::plan_folder_import(db, &input))
        .await
}

/// Imports the folder. `confirmed` are the pages the user agreed to replace; if the folder would
/// now replace others, nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn import_markdown_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    input: FolderImportInput,
    confirmed: Vec<String>,
) -> ApiResult<FolderImportResult> {
    let r = state
        .with_db(move |db| knowledge::import_folder(db, input, confirmed))
        .await?;
    emit_changed(&app, CHANGED);
    if r.backup_path.is_some() {
        emit_changed(&app, &[Entity::Backups]);
    }
    Ok(r)
}

/// What exporting would write, and which files it would replace. Nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn plan_markdown_export(
    state: State<'_, AppState>,
    input: FolderExportInput,
) -> ApiResult<ExportPlan> {
    state
        .with_db(move |db| knowledge::plan_folder_export(db, &input))
        .await
}

/// Writes the pages to the folder. `confirmed` are the files the user agreed to replace; if others
/// would be replaced now, nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn export_markdown_folder(
    state: State<'_, AppState>,
    input: FolderExportInput,
    confirmed: Vec<String>,
) -> ApiResult<FolderExportResult> {
    state
        .with_db(move |db| knowledge::export_folder(db, input, confirmed))
        .await
}

// ---------------------------------------------------------------- dev seed

/// Debug builds only: inserts the prototype's sample groups, pages, history and task links where missing.
#[tauri::command]
#[specta::specta]
pub async fn dev_seed_knowledge(
    app: AppHandle,
    state: State<'_, AppState>,
    seed: KnowledgeSeed,
) -> ApiResult<()> {
    if !cfg!(debug_assertions) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Demo data is only available in development builds.".into(),
        });
    }
    state
        .with_db(move |db| knowledge::seed_if_missing(db, seed))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(())
}
