//! LLM calls for the M5 flows (D-41): which model does the work, the call through the `ai`
//! sidecar (streamed text as `LlmDelta` events), the one JSON retry (prompts/README.md), and
//! *Stop* (`cancel_job`).
//!
//! Every creation command builds its messages with `cairn_core::prompts` and calls [`complete`]
//! or [`complete_json`] here; none talks to the sidecar's `llm.complete` directly.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use cairn_core::prompts::{LlmMessage, ModelRole, Role};
use cairn_core::providers::{self, CallTarget, ModelConfig};
use cairn_core::{CoreError, replies};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_specta::Event;
use tokio::task::AbortHandle;

use crate::error::{ApiError, ApiResult};
use crate::sidecar::{Sidecars, Worker};
use crate::state::AppState;

/// A piece of streamed text for the UI (FR-LLM-AS-7). `stream_id` is the id the command was given
/// (for chat drafts, the assistant message's id).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct LlmDelta {
    pub stream_id: String,
    pub text: String,
}

/// The kind of [`ApiError`] when no usable model is set for a step (FR-KN-SV-3: the step is
/// skipped and the page is saved "not checked"). Its message says what to set up.
pub const UNAVAILABLE: &str = "llm_unavailable";
/// The kind when *Stop* ended the call.
pub const CANCELLED: &str = "cancelled";

pub fn unavailable(message: impl Into<String>) -> ApiError {
    ApiError {
        kind: UNAVAILABLE.into(),
        message: message.into(),
    }
}

fn cancelled() -> ApiError {
    ApiError {
        kind: CANCELLED.into(),
        message: "Stopped.".into(),
    }
}

/// A page the model wrote (README "Pages"), for the commands that return one. `body` starts with
/// the `# title` line.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftText {
    pub title: String,
    pub body: String,
    /// Cut off at the token limit (`finish_reason: length`).
    pub incomplete: bool,
}

impl DraftText {
    pub fn from_reply(reply: &Reply, fallback_title: &str) -> Self {
        let page = replies::page(&reply.text, fallback_title);
        Self {
            title: page.title,
            body: page.body,
            incomplete: reply.cut_off(),
        }
    }
}

/// A model ready to call.
#[derive(Clone)]
pub struct Resolved {
    pub model: ModelConfig,
    pub target: CallTarget,
}

/// The model for a step (FR-KN-UP-9, FR-LLM-AS-6):
/// - `Conversation`: the model picked in the chat, else the default model;
/// - `Default`: Settings › default model;
/// - `Ocr`: the OCR model, else the default model if it can read images, else the chat's model if
///   it can.
///
/// Fails with [`UNAVAILABLE`] when none is set, or its provider is disabled or lacks its key.
pub async fn resolve(
    state: &AppState,
    role: ModelRole,
    conversation_model: Option<String>,
) -> ApiResult<Resolved> {
    let secrets = state.secrets.clone();
    let picked: Result<Resolved, String> = state
        .with_db(move |db| {
            let conn = db.conn();
            let settings = cairn_core::settings::get(conn)?;
            let get = |id: &Option<String>| -> cairn_core::Result<Option<ModelConfig>> {
                match id {
                    Some(id) => match providers::get_model(conn, id) {
                        Ok(m) => Ok(Some(m)),
                        Err(CoreError::NotFound { .. }) => Ok(None),
                        Err(e) => Err(e),
                    },
                    None => Ok(None),
                }
            };
            let default = get(&settings.default_model_id)?;
            let chosen = match role {
                ModelRole::Conversation => get(&conversation_model)?.or(default),
                ModelRole::Default => default,
                ModelRole::Ocr => get(&settings.ocr_model_id)?
                    .or_else(|| default.filter(|m| m.vision))
                    .or(get(&conversation_model)?.filter(|m| m.vision)),
            };
            let Some(model) = chosen else {
                return Ok(Err(match role {
                    ModelRole::Ocr => "No model that can read images is set. Choose an OCR model in Settings › Models & defaults.",
                    _ => "No default model is set. Choose one in Settings › Models & defaults.",
                }
                .to_string()));
            };
            match providers::call_target(conn, secrets.as_ref(), &model.id) {
                Ok(target) => Ok(Ok(Resolved { model, target })),
                // A disabled provider or a missing key: the message says which.
                Err(CoreError::Invalid(message)) => Ok(Err(message)),
                Err(e) => Err(e),
            }
        })
        .await?;
    picked.map_err(unavailable)
}

/// How to make one call.
#[derive(Debug, Clone, Default)]
pub struct CallOptions {
    /// Stream the text as `LlmDelta` events with this id.
    pub stream_id: Option<String>,
    /// Seconds per attempt (PROTOCOL.md `timeout_s`): about 60 for checks, 120 for OCR, 300 for
    /// streamed drafts. `None` = the sidecar's 90.
    pub timeout_s: Option<u32>,
    pub max_tokens: Option<u32>,
    /// The effort level picked in the chat (FR-LLM-ST-2); `None` = the model's default level, or
    /// none (FR-LLM-EF-1). M6: the assistant's calls pass the message's effort.
    pub effort: Option<String>,
    /// FR-LLM-AS-12: turn on the provider's web search, with at most this many searches
    /// (PROTOCOL.md `web_search`). `None` = no web search.
    pub web_search: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// D-65: tokens spent on reasoning, when the provider reports them (part of `output_tokens`).
    #[serde(default)]
    pub reasoning_tokens: Option<u64>,
}

/// A page the provider's web search cited (PROTOCOL.md `llm.complete` result `web`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebSource {
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// The quoted text the answer rests on; may be empty.
    #[serde(default)]
    pub cited_text: String,
}

/// What the web search did; present whenever web search was asked for.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WebReply {
    /// Searches made; `0` = no search was made (the model chose not to, or the provider ignored
    /// the request).
    pub searches: u32,
    pub sources: Vec<WebSource>,
}

/// The sidecar's answer (PROTOCOL.md `llm.complete`).
#[derive(Debug, Clone, Deserialize)]
pub struct Reply {
    pub text: String,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    /// M6: set when `CallOptions::web_search` was.
    #[serde(default)]
    pub web: Option<WebReply>,
}

impl Reply {
    /// Cut off at the token limit: a page made from it is marked incomplete.
    pub fn cut_off(&self) -> bool {
        self.finish_reason.as_deref() == Some("length")
    }
}

fn bad_answer(e: impl std::fmt::Display) -> ApiError {
    tracing::error!(error = %e, "unexpected llm.complete answer");
    ApiError::internal("The AI worker sent an answer Cairn doesn't understand.")
}

/// One `llm.complete` call. A sidecar error becomes an `ai_<kind>` [`ApiError`] (`ai_too_long`
/// tells the caller to send less, FR-KN-UP-4a).
pub async fn complete(
    app: &AppHandle,
    sidecars: &Sidecars,
    target: &CallTarget,
    messages: &[LlmMessage],
    opts: &CallOptions,
) -> ApiResult<Reply> {
    let on_delta: Option<Box<dyn Fn(String) + Send + Sync>> =
        opts.stream_id.clone().map(|stream_id| {
            let app = app.clone();
            Box::new(move |text| {
                let delta = LlmDelta {
                    stream_id: stream_id.clone(),
                    text,
                };
                if let Err(e) = delta.emit(&app) {
                    tracing::warn!(error = %e, "failed to emit LlmDelta");
                }
            }) as Box<dyn Fn(String) + Send + Sync>
        });
    complete_with(sidecars, target, messages, opts, on_delta).await
}

/// [`complete`] with the streamed text going to `on_delta` (streams iff it is `Some`) instead of
/// `LlmDelta` events, for callers without an app handle (tests).
pub async fn complete_with(
    sidecars: &Sidecars,
    target: &CallTarget,
    messages: &[LlmMessage],
    opts: &CallOptions,
    on_delta: Option<Box<dyn Fn(String) + Send + Sync>>,
) -> ApiResult<Reply> {
    let params = json!({
        "target": serde_json::to_value(target).map_err(bad_answer)?,
        "messages": messages,
        "stream": on_delta.is_some(),
        "effort": opts.effort,
        "max_tokens": opts.max_tokens,
        "timeout_s": opts.timeout_s,
        "web_search": opts.web_search.map(|n| json!({ "max_uses": n })),
    });
    let started = std::time::Instant::now();
    let answer = match on_delta {
        Some(on_delta) => {
            sidecars
                .call_streaming(Worker::Ai, "llm.complete", params, on_delta)
                .await
        }
        None => sidecars.call(Worker::Ai, "llm.complete", params).await,
    };
    let reply = match answer {
        Ok(a) => serde_json::from_value::<Reply>(a).map_err(bad_answer),
        Err(e) => Err(e.into()),
    };
    log_call(target, messages, started.elapsed(), &reply);
    reply
}

/// NFR-SEC-3, D-47: one line per LLM call: model, prompt template, tokens in and out, duration and
/// outcome. Never text.
fn log_call(target: &CallTarget, messages: &[LlmMessage], took: std::time::Duration, reply: &ApiResult<Reply>) {
    let template = messages.iter().find_map(|m| m.template.0).unwrap_or("-");
    let count = |n: Option<u64>| n.map_or("?".to_string(), |n| n.to_string());
    let (tokens_in, tokens_out, tokens_reasoning, outcome) = match reply {
        Ok(r) => (
            count(r.usage.as_ref().map(|u| u.input_tokens)),
            count(r.usage.as_ref().map(|u| u.output_tokens)),
            count(r.usage.as_ref().and_then(|u| u.reasoning_tokens)),
            if r.cut_off() { "cut_off".to_string() } else { "ok".to_string() },
        ),
        Err(e) => ("?".into(), "?".into(), "?".into(), e.kind.clone()),
    };
    tracing::info!(
        target: "llm",
        model = %target.model,
        %template,
        %tokens_in,
        %tokens_out,
        %tokens_reasoning,
        ms = took.as_millis() as u64,
        %outcome,
        "llm call"
    );
}

/// A call whose reply is a JSON object (`check`, `suggest-groups`): when the reply can't be read,
/// the bad reply and `check-retry` are appended and the call is made once more (README "JSON
/// shapes"). Fails with `ai_unreadable` when the second reply can't be read either.
pub async fn complete_json(
    app: &AppHandle,
    sidecars: &Sidecars,
    target: &CallTarget,
    messages: &[LlmMessage],
    opts: &CallOptions,
) -> ApiResult<(Map<String, Value>, Reply)> {
    let first = complete(app, sidecars, target, messages, opts).await?;
    if let Some(v) = replies::json_object(&first.text) {
        return Ok((v, first));
    }
    tracing::debug!("JSON reply unreadable, retrying once");
    let mut again = messages.to_vec();
    again.push(LlmMessage::new(Role::Assistant, first.text.clone()));
    again.push(cairn_core::prompts::check_retry()?);
    let second = complete(app, sidecars, target, &again, opts).await?;
    match replies::json_object(&second.text) {
        Some(v) => Ok((v, second)),
        None => Err(ApiError {
            kind: "ai_unreadable".into(),
            message: "The model's answer couldn't be read, even after asking again.".into(),
        }),
    }
}

/// How many tokens the model accepts (FR-KN-UP-4a): its *Context size* setting, else LiteLLM's
/// known size (`llm.model_info`), else `None` (send the whole text and fall back on `too_long`).
pub async fn context_size(sidecars: &Sidecars, resolved: &Resolved) -> Option<u32> {
    if let Some(n) = resolved.model.context_size {
        return Some(n);
    }
    let target = serde_json::to_value(&resolved.target).ok()?;
    match sidecars
        .call(Worker::Ai, "llm.model_info", json!({ "target": target }))
        .await
    {
        Ok(v) => v
            .get("max_input_tokens")
            .and_then(Value::as_u64)
            .map(|n| n.min(u64::from(u32::MAX)) as u32),
        Err(e) => {
            tracing::debug!(kind = %e.kind, "llm.model_info failed");
            None
        }
    }
}

// ---------------------------------------------------------------- *Stop*

/// Running work that *Stop* can end, by id: a stream id, or the assistant message of a chat job.
#[derive(Default)]
pub struct Jobs {
    running: Mutex<HashMap<String, AbortHandle>>,
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, AbortHandle>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Ends the job; `false` when nothing with that id is running.
    pub fn cancel(&self, id: &str) -> bool {
        match self.lock().remove(id) {
            Some(handle) => {
                handle.abort();
                true
            }
            None => false,
        }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    /// Starts `work` in the background under `id` (a job with the same id is stopped first) and
    /// forgets it when it ends. Aborting drops the future, which also cancels a running stream.
    pub fn spawn<F>(self: &Arc<Self>, id: String, work: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.cancel(&id);
        let jobs = Arc::clone(self);
        let key = id.clone();
        let task = tokio::spawn(async move {
            work.await;
            jobs.lock().remove(&key);
        });
        self.lock().insert(id, task.abort_handle());
    }

    /// Runs `work` and waits for it; with an id, *Stop* (`cancel_job(id)`) ends it and the result is
    /// a [`CANCELLED`] error. For commands the UI awaits (checks, task drafts, merges).
    pub async fn run<T, F>(self: &Arc<Self>, id: Option<String>, work: F) -> ApiResult<T>
    where
        T: Send + 'static,
        F: Future<Output = ApiResult<T>> + Send + 'static,
    {
        let Some(id) = id else {
            return work.await;
        };
        self.cancel(&id);
        let task = tokio::spawn(work);
        self.lock().insert(id.clone(), task.abort_handle());
        let result = task.await;
        self.lock().remove(&id);
        match result {
            Ok(r) => r,
            Err(e) if e.is_cancelled() => Err(cancelled()),
            Err(e) => Err(ApiError::internal(e.to_string())),
        }
    }
}

/// *Stop* (FR-KN-UP-4b, FR-LLM-AS-7): ends the work started under `id`. `false` when it had
/// already finished.
#[tauri::command]
#[specta::specta]
pub async fn cancel_job(jobs: State<'_, Arc<Jobs>>, id: String) -> ApiResult<bool> {
    Ok(jobs.cancel(&id))
}
