//! Settings › Search: the search model added after installing (FR-RAG-EM-3, D-82). The work runs in
//! `crate::search_model`; these commands start it and return the state at once.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::error::ApiResult;
use crate::search_model::{SearchModel, SearchModelStatus};

#[tauri::command]
#[specta::specta]
pub async fn get_search_model_status(model: State<'_, Arc<SearchModel>>) -> ApiResult<SearchModelStatus> {
    Ok(model.status())
}

/// Starts the download in the background. `accept_terms` records the Gemma Terms of Use tick;
/// once accepted, it isn't asked again.
#[tauri::command]
#[specta::specta]
pub async fn download_search_model(
    model: State<'_, Arc<SearchModel>>,
    accept_terms: bool,
) -> ApiResult<SearchModelStatus> {
    model.download_now(accept_terms)?;
    Ok(model.status())
}

/// Adds the model from a folder or a `.zip` file, checked in the background.
#[tauri::command]
#[specta::specta]
pub async fn add_search_model(model: State<'_, Arc<SearchModel>>, path: String) -> ApiResult<SearchModelStatus> {
    model.import_now(PathBuf::from(path))?;
    Ok(model.status())
}

/// Stops a running download (forgetting its files) or import; clears a failed one.
#[tauri::command]
#[specta::specta]
pub async fn cancel_search_model_task(model: State<'_, Arc<SearchModel>>) -> ApiResult<SearchModelStatus> {
    model.cancel();
    Ok(model.status())
}

/// Deletes an added model; the stored vectors are kept, so adding it back needs no rebuild.
#[tauri::command]
#[specta::specta]
pub async fn remove_search_model(model: State<'_, Arc<SearchModel>>) -> ApiResult<SearchModelStatus> {
    model.remove().await?;
    Ok(model.status())
}

/// The one-time Home notice (a copy with data but no model) won't show again.
#[tauri::command]
#[specta::specta]
pub async fn dismiss_search_model_notice(model: State<'_, Arc<SearchModel>>) -> ApiResult<SearchModelStatus> {
    model.dismiss_notice()?;
    Ok(model.status())
}
