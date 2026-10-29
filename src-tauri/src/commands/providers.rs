//! AI providers and models (spec §6.1, UI-SET-2, UI-SET-3). Keys go to the OS secure store
//! (`AppState::secrets`, NFR-SEC-1); calls to the provider go through the `ai` sidecar.

use cairn_core::providers::{
    self, ModelConfig, ModelPatch, NewModel, NewProvider, Provider, ProviderPatch,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tauri::{AppHandle, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::sidecar::{Sidecars, Worker};
use crate::state::AppState;

/// One line of *Test connection* (PROTOCOL.md `llm.test`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestCheck {
    /// `basic`, `stream`, `effort:<level>` or `image`.
    pub name: String,
    /// `None` = skipped.
    pub ok: Option<bool>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TestReport {
    pub model_id: String,
    pub checks: Vec<TestCheck>,
}

/// A model offered by the provider's API (*Fetch models*).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteModel {
    pub id: String,
    pub label: Option<String>,
}

/// *Check sign-in* for Entra ID (FR-LLM-PV-1a).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AzureSignin {
    pub signed_in: bool,
    pub account: Option<String>,
    pub detail: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_providers(state: State<'_, AppState>) -> ApiResult<Vec<Provider>> {
    let secrets = state.secrets.clone();
    state
        .with_db(move |db| providers::list_providers(db.conn(), secrets.as_ref()))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn create_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewProvider,
) -> ApiResult<Provider> {
    let secrets = state.secrets.clone();
    let provider = state
        .with_db(move |db| providers::create_provider(db, secrets.as_ref(), input))
        .await?;
    emit_changed(&app, &[Entity::Providers]);
    Ok(provider)
}

#[tauri::command]
#[specta::specta]
pub async fn update_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: ProviderPatch,
) -> ApiResult<Provider> {
    let secrets = state.secrets.clone();
    let provider = state
        .with_db(move |db| providers::update_provider(db, secrets.as_ref(), &id, patch))
        .await?;
    emit_changed(&app, &[Entity::Providers]);
    Ok(provider)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    let secrets = state.secrets.clone();
    let cleared = state
        .with_db(move |db| providers::delete_provider(db, secrets.as_ref(), &id))
        .await?;
    emit_changed(&app, &changed_entities(cleared));
    Ok(())
}

/// Providers changed; settings too when a default / OCR model choice was cleared (UI-SET-3).
fn changed_entities(settings_cleared: usize) -> Vec<Entity> {
    if settings_cleared > 0 {
        vec![Entity::Providers, Entity::Settings]
    } else {
        vec![Entity::Providers]
    }
}

/// Saves the key in the OS secure store. The key never comes back to the UI.
#[tauri::command]
#[specta::specta]
pub async fn set_provider_key(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    key: String,
) -> ApiResult<()> {
    let secrets = state.secrets.clone();
    state
        .with_db(move |db| providers::set_key(db.conn(), secrets.as_ref(), &id, &key))
        .await?;
    emit_changed(&app, &[Entity::Providers]);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn remove_provider_key(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<()> {
    let secrets = state.secrets.clone();
    state
        .with_db(move |db| providers::remove_key(db.conn(), secrets.as_ref(), &id))
        .await?;
    emit_changed(&app, &[Entity::Providers]);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn list_models(state: State<'_, AppState>) -> ApiResult<Vec<ModelConfig>> {
    state.with_db(|db| providers::list_models(db.conn())).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_model(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewModel,
) -> ApiResult<ModelConfig> {
    let model = state
        .with_db(move |db| providers::create_model(db, input))
        .await?;
    emit_changed(&app, &[Entity::Providers]);
    Ok(model)
}

#[tauri::command]
#[specta::specta]
pub async fn update_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: ModelPatch,
) -> ApiResult<ModelConfig> {
    let model = state
        .with_db(move |db| providers::update_model(db, &id, patch))
        .await?;
    emit_changed(&app, &[Entity::Providers]);
    Ok(model)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_model(app: AppHandle, state: State<'_, AppState>, id: String) -> ApiResult<()> {
    let cleared = state
        .with_db(move |db| providers::delete_model(db, &id))
        .await?;
    emit_changed(&app, &changed_entities(cleared));
    Ok(())
}

/// The target as JSON for the sidecar (snake_case, includes the key; never logged).
fn target_json(target: &providers::CallTarget) -> ApiResult<Value> {
    serde_json::to_value(target).map_err(|e| ApiError::internal(e.to_string()))
}

/// A sidecar answer that doesn't have the promised shape.
fn bad_answer(method: &str, e: serde_json::Error) -> ApiError {
    tracing::error!(error = %e, method, "unexpected answer from the AI worker");
    ApiError::internal("The AI worker sent an answer Cairn doesn't understand.")
}

/// *Test connection* for one model (UI-SET-2, FR-LLM-EF-4): basic call, streaming, each effort
/// level, image input.
#[tauri::command]
#[specta::specta]
pub async fn test_model(
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    model_id: String,
) -> ApiResult<TestReport> {
    let secrets = state.secrets.clone();
    let id = model_id.clone();
    let target = state
        .with_db(move |db| providers::call_target(db.conn(), secrets.as_ref(), &id))
        .await?;
    let answer = sidecars
        .call(
            Worker::Ai,
            "llm.test",
            json!({ "target": target_json(&target)? }),
        )
        .await?;
    #[derive(Deserialize)]
    struct Answer {
        checks: Vec<TestCheck>,
    }
    let answer: Answer = serde_json::from_value(answer).map_err(|e| bad_answer("llm.test", e))?;
    Ok(TestReport {
        model_id,
        checks: answer.checks,
    })
}

/// *Fetch models* (UI-SET-2). Azure providers answer with an `unsupported` error from the sidecar.
#[tauri::command]
#[specta::specta]
pub async fn fetch_models(
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    provider_id: String,
) -> ApiResult<Vec<RemoteModel>> {
    let secrets = state.secrets.clone();
    let target = state
        .with_db(move |db| providers::provider_target(db.conn(), secrets.as_ref(), &provider_id))
        .await?;
    let answer = sidecars
        .call(
            Worker::Ai,
            "llm.list_models",
            json!({ "target": target_json(&target)? }),
        )
        .await?;
    #[derive(Deserialize)]
    struct Answer {
        models: Vec<RemoteModel>,
    }
    let answer: Answer =
        serde_json::from_value(answer).map_err(|e| bad_answer("llm.list_models", e))?;
    Ok(answer.models)
}

#[tauri::command]
#[specta::specta]
pub async fn check_azure_signin(sidecars: State<'_, Sidecars>) -> ApiResult<AzureSignin> {
    let answer = sidecars
        .call(Worker::Ai, "azure.check_signin", json!({}))
        .await?;
    // The sidecar answers in snake_case (`signed_in`); the UI gets camelCase.
    #[derive(Deserialize)]
    struct Answer {
        signed_in: bool,
        account: Option<String>,
        detail: String,
    }
    let a: Answer =
        serde_json::from_value(answer).map_err(|e| bad_answer("azure.check_signin", e))?;
    Ok(AzureSignin {
        signed_in: a.signed_in,
        account: a.account,
        detail: a.detail,
    })
}
