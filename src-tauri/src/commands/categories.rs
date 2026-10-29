//! Category commands (FR-PRJ-*).

use cairn_core::categories::{self, NewCategory, Category, CategoryChanges, CategorySeed};
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::state::AppState;

/// Categories, General first. Archived ones only when `include_archived` is true.
#[tauri::command]
#[specta::specta]
pub async fn list_categories(
    state: State<'_, AppState>,
    include_archived: bool,
) -> ApiResult<Vec<Category>> {
    state
        .with_db(move |db| categories::list(db.conn(), include_archived))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_category(state: State<'_, AppState>, id: String) -> ApiResult<Category> {
    state.with_db(move |db| categories::get(db.conn(), &id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_category(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewCategory,
) -> ApiResult<Category> {
    let category = state
        .with_db(move |db| categories::create(db.conn(), input))
        .await?;
    emit_changed(&app, &[Entity::Categories]);
    Ok(category)
}

/// Name, description and colour only; the task key never changes (FR-PRJ-6).
#[tauri::command]
#[specta::specta]
pub async fn update_category(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    changes: CategoryChanges,
) -> ApiResult<Category> {
    let category = state
        .with_db(move |db| categories::update(db.conn(), &id, changes))
        .await?;
    // External mode: a new name renamed the category's folder, so its pages' paths changed.
    if category.folder.is_some() {
        emit_changed(&app, &[Entity::Categories, Entity::Knowledge]);
    } else {
        emit_changed(&app, &[Entity::Categories]);
    }
    Ok(category)
}

#[tauri::command]
#[specta::specta]
pub async fn archive_category(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    archived: bool,
) -> ApiResult<Category> {
    let category = state
        .with_db(move |db| categories::set_archived(db.conn(), &id, archived))
        .await?;
    emit_changed(&app, &[Entity::Categories]);
    Ok(category)
}

/// A free task key for a new category's name (FR-PRJ-6).
#[tauri::command]
#[specta::specta]
pub async fn suggest_category_key(state: State<'_, AppState>, name: String) -> ApiResult<String> {
    state
        .with_db(move |db| categories::suggest_key(db.conn(), &name))
        .await
}

/// Development builds only: inserts the prototype's sample categories that the not-yet-migrated
/// mock screens refer to. Insert-if-missing; never changes existing categories.
#[tauri::command]
#[specta::specta]
pub async fn dev_seed_categories(
    app: AppHandle,
    state: State<'_, AppState>,
    seeds: Vec<CategorySeed>,
) -> ApiResult<u32> {
    if !cfg!(debug_assertions) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Demo data is only available in development builds.".into(),
        });
    }
    let inserted = state
        .with_db(move |db| categories::seed_if_missing(db.conn(), &seeds))
        .await?;
    if inserted > 0 {
        emit_changed(&app, &[Entity::Categories]);
    }
    Ok(inserted as u32)
}
