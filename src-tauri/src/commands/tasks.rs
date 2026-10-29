//! Task commands (spec §4). Thin wrappers: the rules live in `cairn_core::tasks`.
//! Every change emits `DataChanged { tasks, activity }`; query keys under `['tasks', ...]`
//! cover tasks, comments, links, versions and summaries.

use cairn_core::bulk::BulkResult;
use cairn_core::tasks::{
    self, ActivityEvent, CompletionSummary, NewTask, Task, TaskBulkChange, TaskComment, TaskFilter,
    TaskLink, TaskLinkType, TaskPatch, TaskSeed, TaskVersion,
};
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::state::AppState;

/// A task change: its text may change mentions and search results too.
const CHANGED: &[Entity] = &[
    Entity::Tasks,
    Entity::Activity,
    Entity::Mentions,
    Entity::Search,
];

// ---------------------------------------------------------------- reading

#[tauri::command]
#[specta::specta]
pub async fn list_tasks(state: State<'_, AppState>, filter: TaskFilter) -> ApiResult<Vec<Task>> {
    state
        .with_db(move |db| tasks::list(db.conn(), &filter))
        .await
}

/// By id, current key or old key (alias).
#[tauri::command]
#[specta::specta]
pub async fn get_task(state: State<'_, AppState>, id_or_key: String) -> ApiResult<Task> {
    state
        .with_db(move |db| tasks::get(db.conn(), &id_or_key))
        .await
}

/// For rendering `#KEY` mentions; unknown keys are skipped.
#[tauri::command]
#[specta::specta]
pub async fn get_tasks_by_keys(
    state: State<'_, AppState>,
    keys: Vec<String>,
) -> ApiResult<Vec<Task>> {
    state
        .with_db(move |db| tasks::get_by_keys(db.conn(), &keys))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_labels(state: State<'_, AppState>) -> ApiResult<Vec<String>> {
    state.with_db(move |db| tasks::labels(db.conn())).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_comments(
    state: State<'_, AppState>,
    task_id: String,
) -> ApiResult<Vec<TaskComment>> {
    state
        .with_db(move |db| tasks::comments(db.conn(), &task_id))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_links(
    state: State<'_, AppState>,
    task_id: String,
) -> ApiResult<Vec<TaskLink>> {
    state
        .with_db(move |db| tasks::links(db.conn(), &task_id))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn list_task_versions(
    state: State<'_, AppState>,
    task_id: String,
) -> ApiResult<Vec<TaskVersion>> {
    state
        .with_db(move |db| tasks::versions(db.conn(), &task_id))
        .await
}

/// `null` unless the task is done (FR-TASK-SUM-1).
#[tauri::command]
#[specta::specta]
pub async fn get_completion_summary(
    state: State<'_, AppState>,
    task_id: String,
) -> ApiResult<Option<CompletionSummary>> {
    state
        .with_db(move |db| tasks::completion_summary(db.conn(), &task_id))
        .await
}

/// Newest first.
#[tauri::command]
#[specta::specta]
pub async fn list_activity(
    state: State<'_, AppState>,
    limit: u32,
) -> ApiResult<Vec<ActivityEvent>> {
    state
        .with_db(move |db| tasks::activity(db.conn(), limit))
        .await
}

// ---------------------------------------------------------------- changing

#[tauri::command]
#[specta::specta]
pub async fn create_task(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewTask,
) -> ApiResult<Task> {
    let task = state.with_db(move |db| tasks::create(db, input)).await?;
    emit_changed(&app, CHANGED);
    Ok(task)
}

#[tauri::command]
#[specta::specta]
pub async fn update_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: TaskPatch,
) -> ApiResult<Task> {
    let task = state
        .with_db(move |db| tasks::update(db, &id, patch))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(task)
}

/// New key in the target category; the old key stays as an alias (FR-TASK-UI-6).
#[tauri::command]
#[specta::specta]
pub async fn move_task(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    category_id: String,
) -> ApiResult<Task> {
    let task = state
        .with_db(move |db| tasks::move_to_category(db, &id, &category_id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(task)
}

/// Permanent (FR-TASK-UI-3). The UI asks for confirmation first.
#[tauri::command]
#[specta::specta]
pub async fn delete_task(app: AppHandle, state: State<'_, AppState>, id: String) -> ApiResult<()> {
    state.with_db(move |db| tasks::delete(db, &id)).await?;
    emit_changed(&app, CHANGED);
    Ok(())
}

/// One change for many tasks (FR-TASK-UI-7). The UI asks for confirmation first.
#[tauri::command]
#[specta::specta]
pub async fn update_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    change: TaskBulkChange,
) -> ApiResult<BulkResult> {
    let out = state
        .with_db(move |db| tasks::update_many(db, &ids, change))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(out)
}

/// Permanent, as `delete_task` (FR-TASK-UI-7). The UI asks for confirmation first.
#[tauri::command]
#[specta::specta]
pub async fn delete_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> ApiResult<BulkResult> {
    let out = state
        .with_db(move |db| tasks::delete_many(db, &ids))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub async fn add_task_comment(
    app: AppHandle,
    state: State<'_, AppState>,
    task_id: String,
    text: String,
) -> ApiResult<TaskComment> {
    let comment = state
        .with_db(move |db| tasks::add_comment(db, &task_id, &text))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(comment)
}

#[tauri::command]
#[specta::specta]
pub async fn update_task_comment(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> ApiResult<TaskComment> {
    let comment = state
        .with_db(move |db| tasks::update_comment(db, &id, &text))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(comment)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_task_comment(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    state
        .with_db(move |db| tasks::delete_comment(db, &id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn add_task_link(
    app: AppHandle,
    state: State<'_, AppState>,
    link_type: TaskLinkType,
    from_task_id: String,
    to_task_id: String,
) -> ApiResult<TaskLink> {
    let link = state
        .with_db(move |db| tasks::add_link(db, link_type, &from_task_id, &to_task_id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(link)
}

#[tauri::command]
#[specta::specta]
pub async fn remove_task_link(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    state.with_db(move |db| tasks::remove_link(db, &id)).await?;
    emit_changed(&app, CHANGED);
    Ok(())
}

/// Debug builds only: inserts the prototype's sample tasks where missing.
#[tauri::command]
#[specta::specta]
pub async fn dev_seed_tasks(
    app: AppHandle,
    state: State<'_, AppState>,
    seed: TaskSeed,
) -> ApiResult<()> {
    if !cfg!(debug_assertions) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Demo data is only available in development builds.".into(),
        });
    }
    state
        .with_db(move |db| tasks::seed_if_missing(db, seed))
        .await?;
    emit_changed(&app, &[Entity::Tasks, Entity::Activity, Entity::Categories]);
    Ok(())
}
