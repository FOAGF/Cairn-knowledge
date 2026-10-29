//! The glossary (spec §5.9, UI-KN-8). Changes re-index the pages and tasks that use the entry
//! (keyword and vector index), so they emit `glossary` and `search` and wake the indexer.
//!
//! Thin wrappers: the rules live in `cairn_core::glossary`.

use cairn_core::glossary::{
    self, GlossaryEntry, GlossaryPatch, GlossarySeed, GlossaryUsage, NewGlossaryEntry,
};
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::state::AppState;

/// An entry change: the glossary itself and the search results that read it.
const CHANGED: &[Entity] = &[Entity::Glossary, Entity::Search];

#[tauri::command]
#[specta::specta]
pub async fn list_glossary(state: State<'_, AppState>) -> ApiResult<Vec<GlossaryEntry>> {
    state.with_db(|db| glossary::list(db.conn())).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_glossary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewGlossaryEntry,
) -> ApiResult<GlossaryEntry> {
    let entry = state.with_db(move |db| glossary::create(db, input)).await?;
    emit_changed(&app, CHANGED);
    Ok(entry)
}

#[tauri::command]
#[specta::specta]
pub async fn update_glossary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: GlossaryPatch,
) -> ApiResult<GlossaryEntry> {
    let entry = state
        .with_db(move |db| glossary::update(db, &id, patch))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(entry)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_glossary_entry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    state.with_db(move |db| glossary::delete(db, &id)).await?;
    emit_changed(&app, CHANGED);
    Ok(())
}

/// "Used in" (UI-KN-8): `limit` items from `offset` (the UI asks for 3, then 10 more at a time).
#[tauri::command]
#[specta::specta]
pub async fn glossary_usage(
    state: State<'_, AppState>,
    id: String,
    offset: u32,
    limit: u32,
) -> ApiResult<GlossaryUsage> {
    state
        .with_db(move |db| glossary::usage(db.conn(), &id, offset, limit))
        .await
}

/// Development only: inserts the sample glossary where ids are missing.
#[tauri::command]
#[specta::specta]
pub async fn dev_seed_glossary(
    app: AppHandle,
    state: State<'_, AppState>,
    seed: Vec<GlossarySeed>,
) -> ApiResult<()> {
    if !cfg!(debug_assertions) {
        return Err(ApiError {
            kind: "invalid".into(),
            message: "Demo data is only available in development builds.".into(),
        });
    }
    state
        .with_db(move |db| glossary::dev_seed(db, seed))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(())
}
