//! The Assistant (M6, D-44, spec §6.4): a message in a conversation runs as a background job
//! named after the assistant message (*Stop* = `cancel_job(<message id>)`), on the same
//! [`Ctx`] / [`Progress`] plumbing as the file drafts (`commands::files`).
//!
//! The job (`cairn_core::assistant` describes the flow):
//! 1. 🌐 on: `ask-web` only, with `CallOptions::web_search`; the reply's `web` evidence decides the
//!    label (*From the web* / *No web search was made*), and a reply without a search on Chat
//!    Completions or a local model removes the model's tick (`providers::set_web_search`, emit
//!    `Providers`) with the warning on the answer (`AnswerPart::web_unticked`).
//! 2. `ask-plan` on the conversation's model (`complete_json`-style, one `check-retry`; then
//!    `plan::fallback`).
//! 3. By kind: retrieve + `ask-answer` streamed (stream id = the message id); `find`; create /
//!    change → `commands::actions::run`; file draft → the M5 file flow; `doc-terms` once per
//!    attached document (default model) after the first answer about it.
//!
//! Steps on the message, e.g. "Reading your question", "Searching your notes", "Found 6 items",
//! "Writing the answer", "Searching the web".
//!
//! What *Retry* and *Search all categories* need is on the messages: the user message holds the
//! text and the attachment, the assistant message its model, its effort and in `extra`
//! `{"ask": true, "web": …, "allCategories": …}`.
//!
//! The first step is on the message before any model call, and one step stays active until the
//! answer, the find list or the proposal is stored on it, so the UI never sees a finished-looking
//! message in between.
//!
//! Owned by the M6 answers agent (the action branch by the actions agent).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use specta::Type;
use tauri::{AppHandle, State};

use cairn_core::assistant::find::{self, FindQuery};
use cairn_core::assistant::formats::{
    self, ItemInfo, PiecePart, CategoryLine, SourcePiece, Turn,
};
use cairn_core::assistant::history::{self, HistoryUsage};
use cairn_core::assistant::items::{self, ItemRef};
use cairn_core::assistant::markers;
use cairn_core::assistant::parts::{AnswerBasis, AnswerPart, FindPart, SourceKind, SourceRef};
use cairn_core::assistant::plan::{self, Plan, PlanFilters, PlanCategory, RequestKind};
use cairn_core::assistant::references;
use cairn_core::assistant::retrieve::{self, RelevanceRule, Retrieval, Retrieved};
use cairn_core::checks::{self, RawTerm};
use cairn_core::chunking::estimate_tokens;
use cairn_core::conversations::{
    self, ChatAttachment, ChatMessage, ChatRole, Conversation, ConversationPatch, FileKind,
    MessagePatch, NewMessage,
};
use cairn_core::prompts::{self, LlmMessage, ModelRole, Role};
use cairn_core::providers::{self, CallTarget, ProviderKind, RequestFormat};
use cairn_core::settings::{self, RELEVANCE_MARGIN, Settings};
use cairn_core::{glossary, hybrid, categories, replies, sources};

use super::files::{
    self, ChatJobStarted, Ctx, Done, FileJob, Job, JobError, Progress, Uploads, Upload, plain,
    score_by_meaning, score_by_words, spawn_job,
};
use crate::error::ApiResult;
use crate::events::{Entity, emit_changed};
use crate::llm::{self, CallOptions, Jobs, Resolved};
use crate::sidecar::Sidecars;
use crate::state::AppState;

/// Tokens kept for the system prompt, the glossary, the question and the reply when the model's
/// context size limits the sources and the history.
const REPLY_RESERVE_TOKENS: u32 = 3000;
const PLAN_TIMEOUT_S: u32 = 90;
const ANSWER_TIMEOUT_S: u32 = 300;
/// What a find with topic words takes from the relevance search: no budget in practice.
const UNLIMITED_TOKENS: usize = usize::MAX / 4;
/// Most earlier items sent again as pinned sources.
const MAX_PINNED: usize = 8;
/// The question put to a file sent without a message.
const FILE_ONLY_QUESTION: &str = "Summarize the attached file.";
const NEW_CONVERSATION: &str = "New conversation";

/// A message typed in a normal conversation (UI-AS-3). In a *Create knowledge from a file*
/// conversation, and while a draft card is active, the UI keeps using `start_file_draft` and
/// `revise_draft` (M5).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SendInput {
    pub conversation_id: String,
    /// As typed; may be empty when a file is attached.
    pub text: String,
    /// The model picked in the chat; `None` = the default model.
    pub model_id: Option<String>,
    /// The effort picked in the chat (FR-LLM-ST-2); `None` = the model's default.
    pub effort: Option<String>,
    /// A finished upload (`upload_begin` / `upload_chunk`) sent with the message (FR-LLM-AS-11).
    pub upload_id: Option<String>,
    /// 🌐 for this message only (FR-LLM-AS-12): only `text` is sent, with web search on.
    pub web: bool,
}

/// What an assistant job needs; also rebuilt from the stored messages by *Retry* and *Search all
/// categories*.
pub(crate) struct AskJob {
    pub(crate) conversation_id: String,
    pub(crate) user_message_id: String,
    pub(crate) assistant_id: String,
    /// The message as the model reads it (page links as `[[Title]]`, [`references::for_model`]).
    pub(crate) text: String,
    /// The tasks and pages the message refers to (FR-LLM-AS-14, D-81): always sources, pinned
    /// first, and candidates for a change.
    pub(crate) refs: Vec<ItemRef>,
    pub(crate) model_id: Option<String>,
    pub(crate) effort: Option<String>,
    pub(crate) web: bool,
    /// *Search all categories*: ignore the conversation's scope for this message.
    pub(crate) all_categories: bool,
    /// The attached file's upload, until it is read (a first run only).
    pub(crate) upload: Option<Upload>,
    /// The attached file's `chat_files` row, once it is read (*Retry*).
    pub(crate) chat_file_id: Option<String>,
}

// ---------------------------------------------------------------- commands

/// Adds the user message (with the file, if any) and the assistant message, then runs the job.
/// The first message of a conversation still called "New conversation" also titles it (its
/// first 48 characters, "…" when cut; the file name when there is no text).
#[tauri::command]
#[specta::specta]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    uploads: State<'_, Uploads>,
    input: SendInput,
) -> ApiResult<ChatJobStarted> {
    let _ = &sidecars;
    let (job, started) = prepare_send(&state, &uploads, input).await?;
    emit_changed(&app, &[Entity::Conversations]);
    spawn_job(
        &app,
        &jobs,
        started.assistant_message_id.clone(),
        Job::Ask(job),
    );
    Ok(started)
}

/// The two messages of a question, and the job that fills the second.
pub(crate) async fn prepare_send(
    state: &AppState,
    uploads: &Uploads,
    input: SendInput,
) -> ApiResult<(AskJob, ChatJobStarted)> {
    let text = input.text.trim().to_string();
    if text.is_empty() && input.upload_id.is_none() {
        return Err(files::invalid("Write a message or attach a file."));
    }
    if input.web && input.upload_id.is_some() {
        return Err(files::invalid(
            "A message with the web switch on can't carry a file: only what you type is sent.",
        ));
    }
    if input.web && text.is_empty() {
        return Err(files::invalid("Write what to look up on the web."));
    }
    let upload = match &input.upload_id {
        Some(id) => {
            let upload = uploads.take(id)?;
            if upload.written != u64::from(upload.size) {
                files::remove_path(&upload.path);
                return Err(files::invalid(
                    "The upload did not finish. Attach the file again.",
                ));
            }
            Some(upload)
        }
        None => None,
    };
    let (model_id, model_label) = files::chat_model(state, input.model_id.clone()).await;
    let attachment = upload.as_ref().map(|u| ChatAttachment {
        name: u.name.clone(),
        size: u.size,
        kind: u.kind,
        chat_file_id: None,
    });
    let (conversation_id, effort, web) = (
        input.conversation_id.clone(),
        input.effort.clone(),
        input.web,
    );
    let (typed, label_model) = (text.clone(), (model_id.clone(), model_label));
    let file_name = upload.as_ref().map(|u| u.name.clone());
    let (user, assistant, read) = state
        .with_db(move |db| {
            let read = job_text(db, &typed)?;
            let conversation = conversations::get(db.conn(), &conversation_id)?;
            let first = conversations::messages(db.conn(), &conversation_id)?.is_empty();
            if first && conversation.title == NEW_CONVERSATION {
                let title = title_of(&read.0, file_name.as_deref());
                conversations::update(
                    db,
                    &conversation_id,
                    ConversationPatch {
                        title: Some(title),
                        ..ConversationPatch::default()
                    },
                )?;
            }
            let user = conversations::add_message(
                db,
                &conversation_id,
                NewMessage {
                    role: ChatRole::User,
                    text: typed,
                    model_id: None,
                    model_label: None,
                    effort: None,
                    attachment,
                    steps: None,
                    draft: None,
                    answer: None,
                    find: None,
                    proposal: None,
                    extra: web.then(|| json!({ "web": true }).to_string()),
                },
            )?;
            let assistant = conversations::add_message(
                db,
                &conversation_id,
                NewMessage {
                    role: ChatRole::Assistant,
                    text: String::new(),
                    model_id: label_model.0,
                    model_label: label_model.1,
                    effort,
                    attachment: None,
                    steps: Some(vec![]),
                    draft: None,
                    answer: None,
                    find: None,
                    proposal: None,
                    extra: Some(ask_extra(web, false)),
                },
            )?;
            Ok((user, assistant, read))
        })
        .await?;
    let (text, refs) = read;
    let job = AskJob {
        conversation_id: input.conversation_id,
        user_message_id: user.id.clone(),
        assistant_id: assistant.id.clone(),
        text,
        refs,
        model_id: assistant.model_id.clone(),
        effort: input.effort,
        web: input.web,
        all_categories: false,
        upload,
        chat_file_id: None,
    };
    Ok((
        job,
        ChatJobStarted {
            user_message_id: user.id,
            assistant_message_id: assistant.id,
        },
    ))
}

/// The conversation's title from its first message.
fn title_of(text: &str, file_name: Option<&str>) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        return file_name.unwrap_or(NEW_CONVERSATION).to_string();
    }
    let cut: String = first.chars().take(48).collect();
    if first.chars().count() > 48 {
        format!("{}…", cut.trim_end())
    } else {
        cut
    }
}

/// What a job reads of the user's message (FR-LLM-AS-14, D-81): the text as the model reads it,
/// and the items it refers to.
fn job_text(db: &cairn_core::Db, typed: &str) -> cairn_core::Result<(String, Vec<ItemRef>)> {
    Ok((references::for_model(db.conn(), typed), references::items(db.conn(), typed)?))
}

fn ask_extra(web: bool, all_categories: bool) -> String {
    json!({ "ask": true, "web": web, "allCategories": all_categories }).to_string()
}

/// The job of an assistant message again, from the stored messages: `None` when the message is
/// not one of the assistant's (an M5 file draft). Used by *Retry* (`files::plan_retry`) and
/// *Search all categories*.
pub(crate) fn retry_job(
    db: &cairn_core::Db,
    message: &ChatMessage,
    user: &ChatMessage,
    model_id: Option<String>,
) -> cairn_core::Result<Option<AskJob>> {
    let Some(extra) = message
        .extra
        .as_deref()
        .and_then(|e| serde_json::from_str::<Value>(e).ok())
        .filter(|v| v.get("ask").and_then(Value::as_bool) == Some(true))
    else {
        return Ok(None);
    };
    let chat_file_id = match &user.attachment {
        Some(a) => Some(a.chat_file_id.clone().ok_or_else(|| {
            cairn_core::CoreError::Invalid(
                "The file's text was not read yet. Attach the file again.".into(),
            )
        })?),
        None => None,
    };
    let flag = |key: &str| extra.get(key).and_then(Value::as_bool).unwrap_or(false);
    let (text, refs) = job_text(db, user.text.trim())?;
    Ok(Some(AskJob {
        conversation_id: message.conversation_id.clone(),
        user_message_id: user.id.clone(),
        assistant_id: message.id.clone(),
        text,
        refs,
        model_id,
        effort: message.effort.clone(),
        web: flag("web"),
        all_categories: flag("allCategories"),
        upload: None,
        chat_file_id,
    }))
}

/// *Search all categories* (FR-LLM-AS-4): answers the same message again over every category,
/// replacing the answer. Returns the assistant message id (the new job's id).
#[tauri::command]
#[specta::specta]
pub async fn search_all_categories(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, Arc<Jobs>>,
    message_id: String,
) -> ApiResult<String> {
    let id = message_id.clone();
    let job = state.with_db(move |db| widen_search(db, &id)).await?;
    emit_changed(&app, &[Entity::Conversations]);
    spawn_job(&app, &jobs, message_id.clone(), Job::Ask(job));
    Ok(message_id)
}

pub(crate) fn widen_search(db: &mut cairn_core::Db, message_id: &str) -> cairn_core::Result<AskJob> {
    let message = conversations::get_message(db.conn(), message_id)?;
    let all = conversations::messages(db.conn(), &message.conversation_id)?;
    let user = files::previous_user_message(&all, message_id)
        .filter(|_| message.role == ChatRole::Assistant)
        .ok_or_else(|| cairn_core::CoreError::Invalid("This message can't be searched again.".into()))?;
    conversations::update_message(
        db,
        message_id,
        MessagePatch {
            extra: Some(ask_extra(false, true)),
            ..MessagePatch::default()
        },
    )?;
    let message = conversations::reset_message(db, message_id)?;
    let mut message = message;
    message.extra = Some(ask_extra(false, true));
    retry_job(db, &message, &user, message.model_id.clone())?
        .ok_or_else(|| cairn_core::CoreError::Invalid("This message can't be searched again.".into()))
}

/// The sizes the composer and the job work with.
struct Budgets {
    /// Tokens of sources.
    source: usize,
    /// Tokens of earlier turns in force.
    history: u32,
    /// What the model takes for history; `None` = unknown.
    history_limit: Option<u32>,
    /// The conversation's own budget or the setting, before the model's cap.
    configured: u32,
}

fn budgets(settings: &Settings, conversation: &Conversation, context_size: Option<u32>) -> Budgets {
    let floor = settings::MIN_BUDGET_TOKENS;
    let room = context_size.map(|c| c.saturating_sub(REPLY_RESERVE_TOKENS));
    let source = match room {
        Some(r) => settings.assistant_source_budget.min((r / 2).max(floor)),
        None => settings.assistant_source_budget,
    };
    let history_limit = room.map(|r| r.saturating_sub(source).max(floor));
    let configured = conversation
        .history_budget
        .unwrap_or(settings.assistant_history_budget);
    Budgets {
        source: source as usize,
        history: history_limit.map_or(configured, |l| configured.min(l)),
        history_limit,
        configured,
    }
}

/// The numbers behind the composer's indicator: tokens used by the conversation so far, and the
/// budgets for the model.
async fn history_numbers(
    state: &AppState,
    sidecars: &Sidecars,
    conversation_id: &str,
    model_id: Option<String>,
) -> ApiResult<(u32, Budgets, Conversation)> {
    let id = conversation_id.to_string();
    let (conversation, settings, used) = state
        .with_db(move |db| {
            let conversation = conversations::get(db.conn(), &id)?;
            let all = conversations::messages(db.conn(), &id)?;
            let used: usize = history::turns(db.conn(), &all, "")
                .iter()
                .map(|t| estimate_tokens(&t.text))
                .sum();
            Ok((conversation, settings::get(db.conn())?, used as u32))
        })
        .await?;
    // No usable model: the setting is all there is.
    let size = match llm::resolve(state, ModelRole::Conversation, model_id).await {
        Ok(resolved) => llm::context_size(sidecars, &resolved).await,
        Err(_) => None,
    };
    Ok((used, budgets(&settings, &conversation, size), conversation))
}

/// What the composer shows for the conversation with this model (FR-LLM-AS-10).
#[tauri::command]
#[specta::specta]
pub async fn history_usage(
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    conversation_id: String,
    model_id: Option<String>,
) -> ApiResult<HistoryUsage> {
    let (used, b, _) = history_numbers(&state, &sidecars, &conversation_id, model_id).await?;
    Ok(history::usage(used, b.history, b.history_limit))
}

/// *Widen for this conversation*: doubles its history budget, up to the model's limit.
#[tauri::command]
#[specta::specta]
pub async fn widen_history(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    conversation_id: String,
    model_id: Option<String>,
) -> ApiResult<HistoryUsage> {
    let (used, b, _) = history_numbers(&state, &sidecars, &conversation_id, model_id.clone()).await?;
    let wider = history::widened(b.history, b.history_limit);
    if wider != b.configured {
        let id = conversation_id.clone();
        state
            .with_db(move |db| {
                conversations::update(
                    db,
                    &id,
                    ConversationPatch {
                        history_budget: Some(wider),
                        ..ConversationPatch::default()
                    },
                )
            })
            .await?;
        emit_changed(&app, &[Entity::Conversations]);
    }
    let (used_now, b, _) = history_numbers(&state, &sidecars, &conversation_id, model_id).await?;
    let _ = used;
    Ok(history::usage(used_now, b.history, b.history_limit))
}

// ---------------------------------------------------------------- the job

/// Everything read from the database once at the start of a job.
struct Base {
    conversation: Conversation,
    settings: Settings,
    all: Vec<ChatMessage>,
    categories: Vec<cairn_core::categories::Category>,
    /// Archived ones too (for the action branch, which may name any category).
    categories_all: Vec<cairn_core::categories::Category>,
    assistant: ChatMessage,
}

async fn load_base(ctx: &Ctx<'_>, job: &AskJob) -> ApiResult<Base> {
    let (conversation_id, assistant_id) = (job.conversation_id.clone(), job.assistant_id.clone());
    ctx.state
        .with_db(move |db| {
            let conn = db.conn();
            Ok(Base {
                conversation: conversations::get(conn, &conversation_id)?,
                settings: settings::get(conn)?,
                all: conversations::messages(conn, &conversation_id)?,
                categories: categories::list(conn, false)?,
                categories_all: categories::list(conn, true)?,
                assistant: conversations::get_message(conn, &assistant_id)?,
            })
        })
        .await
}

/// A model call that returns a JSON object: the reply is read, and an unreadable one is asked
/// again once with `check-retry` (README "JSON shapes"). `None` when the second reply is
/// unreadable too.
async fn json_reply(
    ctx: &Ctx<'_>,
    target: &CallTarget,
    messages: Vec<LlmMessage>,
    opts: &CallOptions,
) -> ApiResult<Option<Map<String, Value>>> {
    let first = ctx.complete(target, &messages, opts).await?;
    if let Some(v) = replies::json_object(&first.text) {
        return Ok(Some(v));
    }
    tracing::debug!("JSON reply unreadable, retrying once");
    let mut again = messages;
    again.push(LlmMessage::new(Role::Assistant, first.text));
    again.push(prompts::check_retry()?);
    let second = ctx.complete(target, &again, opts).await?;
    Ok(replies::json_object(&second.text))
}

fn options(stream: Option<&str>, effort: Option<String>, timeout_s: u32) -> CallOptions {
    CallOptions {
        stream_id: stream.map(str::to_string),
        timeout_s: Some(timeout_s),
        max_tokens: None,
        effort,
        web_search: None,
    }
}

fn item_ref(info: &ItemInfo) -> ItemRef {
    match info {
        ItemInfo::Task(t) => ItemRef::task(&t.id),
        ItemInfo::Page(p) => ItemRef::page(&p.id),
    }
}

fn web_source(w: &llm::WebSource) -> SourceRef {
    SourceRef {
        kind: SourceKind::Web,
        id: None,
        title: if w.title.is_empty() { w.url.clone() } else { w.title.clone() },
        key: None,
        category_id: None,
        part: None,
        snippet: w.cited_text.clone(),
        url: Some(w.url.clone()),
    }
}

/// Whether a reply without a search means the model's *Web search* tick was wrong: on Chat
/// Completions and local servers Cairn has no built-in search tool to offer.
fn ignores_web_search(target: &CallTarget) -> bool {
    // D-69: DeepSeek's searches are Cairn's own; a reply without one means every search failed.
    if target.provider_kind == ProviderKind::DeepSeek {
        return false;
    }
    target.format == RequestFormat::ChatCompletions || target.provider_kind == ProviderKind::Local
}

/// The job (module docs). Stores the answer, find list or proposal on the message as it goes;
/// returns the final text.
pub(crate) async fn ask_job(
    ctx: &Ctx<'_>,
    job: &AskJob,
    progress: &mut Progress,
) -> Result<Done, JobError> {
    // The first step is recorded before anything else (the UI marks a web message by it).
    let base = load_base(ctx, job).await.map_err(plain)?;
    let label = base
        .assistant
        .model_label
        .clone()
        .unwrap_or_else(|| "the model".to_string());
    progress
        .begin(
            ctx,
            if job.web {
                format!("Searching the web with {label}")
            } else {
                "Reading your question".to_string()
            },
        )
        .await;
    let resolved = llm::resolve(ctx.state, ModelRole::Conversation, job.model_id.clone())
        .await
        .map_err(plain)?;
    let label = resolved.model.label.clone();
    if job.web {
        return web_job(ctx, job, &resolved, &base, progress).await;
    }
    let size = llm::context_size(ctx.sidecars, &resolved).await;
    let sizes = budgets(&base.settings, &base.conversation, size);
    let run = Run::new(ctx, job, &base, &resolved, label, sizes);

    // The attached file is read first: the first step decides with it in view.
    let file = run.attached_file(progress).await?;
    let planned = run.plan(file.as_ref()).await?;
    let file_ref = file.as_ref();
    match planned.plan.kind {
        RequestKind::FileDraft => {
            let Some(file) = file_ref else {
                return Err(plain(files::invalid("There is no file to draft a page from.")));
            };
            run.draft(file, progress).await
        }
        RequestKind::Find => run.find(&planned, progress).await,
        RequestKind::Create | RequestKind::Change => {
            let gathered = run.gather(&planned, file_ref, progress).await?;
            let input = run.action_input(&planned, &gathered);
            super::actions::run(ctx, job, &planned.plan, input, progress).await
        }
        RequestKind::Question | RequestKind::FileQuestion | RequestKind::General => {
            run.answer(&planned, file_ref, progress).await
        }
    }
}

// ---------------------------------------------------------------- 🌐 messages

async fn web_job(
    ctx: &Ctx<'_>,
    job: &AskJob,
    resolved: &Resolved,
    base: &Base,
    progress: &mut Progress,
) -> Result<Done, JobError> {
    let today = formats::today_line(find::local_today());
    let messages = prompts::ask_web(&today, &job.text, true).map_err(|e| plain(e.into()))?;
    let searches = base.settings.web_searches_per_message;
    let mut opts = options(Some(&job.assistant_id), job.effort.clone(), ANSWER_TIMEOUT_S);
    opts.web_search = Some(searches);
    let mut warning: Option<String> = None;
    let reply = match ctx.complete(&resolved.target, &messages, &opts).await {
        Ok(r) => Some(r),
        Err(e) if e.kind == "ai_web_search_unsupported" => {
            // The provider refused web search: the tick goes, and the same message is answered
            // again without the web.
            untick(ctx, &resolved.model.id).await;
            warning = Some(e.message);
            None
        }
        Err(e) => return Err(plain(e)),
    };
    let reply = match reply {
        Some(r) => r,
        None => {
            progress
                .begin(ctx, format!("Answering with {}, without the web", resolved.model.label))
                .await;
            let again = options(Some(&job.assistant_id), job.effort.clone(), ANSWER_TIMEOUT_S);
            let messages = prompts::ask_web(&today, &job.text, false).map_err(|e| plain(e.into()))?;
            ctx.complete(&resolved.target, &messages, &again)
                .await
                .map_err(plain)?
        }
    };
    let searched = reply.web.as_ref().is_some_and(|w| w.searches > 0);
    let mut part = AnswerPart::default();
    if searched {
        let web = reply.web.as_ref();
        part.basis = AnswerBasis::Web;
        part.sources = web
            .map(|w| w.sources.iter().map(web_source).collect())
            .unwrap_or_default();
        part.cited = (1..=part.sources.len() as u32).collect();
    } else {
        part.basis = AnswerBasis::NoWebSearch;
        if warning.is_none() && ignores_web_search(&resolved.target) {
            untick(ctx, &resolved.model.id).await;
            warning = Some(format!(
                "{} ignored the web search request, so its Web search tick was removed.",
                resolved.model.label
            ));
        }
    }
    part.web_unticked = warning;
    let text = reply.text.trim().to_string();
    ctx.patch(
        &job.assistant_id,
        MessagePatch {
            text: Some(text.clone()),
            answer: Some(part),
            ..MessagePatch::default()
        },
    )
    .await;
    Ok(Done { text, card: None })
}

/// Removes the model's *Web search* tick (FR-LLM-AS-12).
async fn untick(ctx: &Ctx<'_>, model_id: &str) {
    let id = model_id.to_string();
    let done = ctx
        .state
        .with_db(move |db| providers::set_web_search(db, &id, false).map(|_| ()))
        .await;
    match done {
        Ok(()) => {
            if let Some(app) = ctx.app {
                emit_changed(app, &[Entity::Providers]);
            }
        }
        Err(e) => tracing::warn!(error = %e.message, "could not remove the web search tick"),
    }
}

// ---------------------------------------------------------------- the notes

/// The file attached to the message, read.
struct AttachedFile {
    id: String,
    name: String,
    text: String,
}

/// One turn of the job: what the steps share.
struct Run<'a> {
    ctx: &'a Ctx<'a>,
    job: &'a AskJob,
    base: &'a Base,
    resolved: &'a Resolved,
    label: String,
    today: String,
    /// Every active category, with the scope of this message.
    categories: Vec<PlanCategory>,
    /// The conversation's scope as ids; empty = every category.
    scope_ids: Vec<String>,
    budgets: Budgets,
}

/// What the notes and the conversation give one request.
struct Gathered {
    retrieved: Retrieved,
    /// The attached file's parts first, then the notes' pieces.
    pieces: Vec<SourcePiece>,
    sources: Vec<SourceRef>,
    glossary: String,
    history: Vec<LlmMessage>,
    refs_items: Vec<ItemInfo>,
}

impl<'a> Run<'a> {
    fn new(
        ctx: &'a Ctx<'a>,
        job: &'a AskJob,
        base: &'a Base,
        resolved: &'a Resolved,
        label: String,
        budgets: Budgets,
    ) -> Self {
        let scope_ids = if job.all_categories {
            Vec::new()
        } else {
            base.conversation.scope_category_ids.clone()
        };
        let categories = base
            .categories
            .iter()
            .map(|p| PlanCategory {
                id: p.id.clone(),
                name: p.name.clone(),
                in_scope: scope_ids.is_empty() || scope_ids.contains(&p.id),
            })
            .collect();
        Run {
            ctx,
            job,
            base,
            resolved,
            label,
            today: formats::today_line(find::local_today()),
            categories,
            scope_ids,
            budgets,
        }
    }

    fn lines(&self) -> Vec<CategoryLine> {
        self.base
            .categories
            .iter()
            .zip(&self.categories)
            .map(|(p, pp)| CategoryLine {
                name: p.name.clone(),
                key: p.key.clone(),
                in_scope: pp.in_scope,
            })
            .collect()
    }

    /// The text of the attached file (reading it first when this is its first run).
    async fn attached_file(&self, progress: &mut Progress) -> Result<Option<AttachedFile>, JobError> {
        let job = self.job;
        if let Some(upload) = &job.upload {
            let file_job = FileJob {
                conversation_id: job.conversation_id.clone(),
                user_message_id: job.user_message_id.clone(),
                assistant_id: job.assistant_id.clone(),
                upload: None,
                name: upload.name.clone(),
                kind: upload.kind,
                instruction: job.text.clone(),
                model_id: job.model_id.clone(),
                effort: job.effort.clone(),
                category_id: String::new(),
                chat_file_id: None,
            };
            let (id, text) = files::read_file(self.ctx, &file_job, upload, progress).await?;
            return Ok(Some(AttachedFile {
                id,
                name: upload.name.clone(),
                text,
            }));
        }
        let Some(id) = &job.chat_file_id else {
            return Ok(None);
        };
        let id = id.clone();
        let file = self
            .ctx
            .state
            .with_db(move |db| conversations::chat_file(db.conn(), &id))
            .await
            .map_err(plain)?;
        let text = file.text.ok_or_else(|| {
            plain(files::invalid(
                "The file's text is no longer kept. Attach the file again.",
            ))
        })?;
        progress.text_ready = true;
        Ok(Some(AttachedFile {
            id: file.id,
            name: file.file_name,
            text,
        }))
    }

    /// Step 1 (`ask-plan`) on the conversation's model; also loads what the plan is read against
    /// (the earlier turns and the items shown with the previous answer).
    async fn plan(&self, file: Option<&AttachedFile>) -> Result<Planned, JobError> {
        let before = self.job.user_message_id.clone();
        let all = self.base.all.clone();
        let (turns, shown_refs, shown_is_find) = self
            .ctx
            .state
            .with_db(move |db| {
                let turns = history::turns(db.conn(), &all, &before);
                let earlier: Vec<ChatMessage> =
                    all.iter().take_while(|m| m.id != before).cloned().collect();
                let is_find = earlier
                    .iter()
                    .rev()
                    .find(|m| m.role == ChatRole::Assistant && m.error.is_none())
                    .is_some_and(|m| m.find.is_some());
                Ok((turns, history::shown_items(&earlier), is_find))
            })
            .await
            .map_err(plain)?;
        let shown = self
            .ctx
            .state
            .with_db(move |db| items::load(db.conn(), &shown_refs))
            .await
            .map_err(plain)?;
        let lines = self.lines();
        let history_text = formats::history_text(&turns);
        let cited = formats::cited_lines(&shown);
        let categories = formats::categories_lines(&lines);
        let scope = formats::scope_sentence(&lines);
        let messages = prompts::ask_plan(&prompts::AskPlan {
            today: &self.today,
            categories: &categories,
            scope: &scope,
            history: &history_text,
            cited: &cited,
            attachment: file.map(|f| f.name.as_str()),
            message: &self.job.text,
        })
        .map_err(|e| plain(e.into()))?;
        let opts = options(None, self.job.effort.clone(), PLAN_TIMEOUT_S);
        let reply = json_reply(self.ctx, &self.resolved.target, messages, &opts)
            .await
            .map_err(plain)?;
        let plan = match reply {
            Some(map) => {
                let (plan, notes) = plan::read(&map, &self.categories, file.is_some(), shown.len());
                // NFR-SEC-3: the notes quote the model's words; only their number is logged.
                if !notes.is_empty() {
                    tracing::debug!(count = notes.len(), "ask-plan repaired");
                }
                plan
            }
            None => plan::fallback(&self.job.text, file.is_some()),
        };
        Ok(Planned {
            plan,
            shown,
            shown_is_find,
            turns,
        })
    }

    /// The query vector for the search words; `None` (with a step note) when the search model
    /// is not ready, and only keyword results are used.
    async fn embed(&self, words: &str, progress: &mut Progress) -> Option<Vec<f32>> {
        match self.ctx.sidecars.embed_query(words).await {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::debug!(kind = %e.kind, "search by meaning unavailable for the assistant");
                // Without an installed model (D-82) words are the normal way: no note.
                if e.kind != crate::sidecar::NOT_INSTALLED {
                    progress
                        .note(
                            self.ctx,
                            "Searching by words only: the search model is not ready",
                        )
                        .await;
                }
                None
            }
        }
    }

    async fn retrieve(
        &self,
        words: &str,
        filters: &PlanFilters,
        vector: Option<Vec<f32>>,
        budget: usize,
        pinned: Vec<ItemRef>,
    ) -> Result<Retrieved, JobError> {
        let (words, filters, scope) = (words.to_string(), filters.clone(), self.scope_ids.clone());
        let rule = RelevanceRule {
            floor: self.base.settings.assistant_min_relevance,
            margin: RELEVANCE_MARGIN,
        };
        self.ctx
            .state
            .with_db(move |db| {
                retrieve::retrieve(
                    db.conn(),
                    &Retrieval {
                        words: &words,
                        query_vector: vector.as_deref(),
                        scope_category_ids: &scope,
                        filters: &filters,
                        rule,
                        budget_tokens: budget,
                        pinned: &pinned,
                    },
                )
            })
            .await
            .map_err(plain)
    }

    /// The items sent with this turn whatever the search finds: the message's references
    /// (FR-LLM-AS-14), the plan's refs, then the items cited in the previous answer (not the rows
    /// of a find list).
    fn pinned(&self, p: &Planned) -> Vec<ItemRef> {
        let mut out: Vec<ItemRef> = self.job.refs.clone();
        let refs = p.plan.refs.iter().filter_map(|i| p.shown.get(*i));
        let others = p.shown.iter().filter(|_| !p.shown_is_find);
        for info in refs.chain(others) {
            let r = item_ref(info);
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out.truncate(MAX_PINNED);
        out
    }

    /// The glossary lines for the question and the sources (FR-LLM-AS-8): the entries of the
    /// sources' categories (the scope's when there are no sources), plus those the search found.
    async fn glossary(&self, g: &Retrieved, texts: Vec<String>) -> String {
        let mut category_ids: Vec<String> = Vec::new();
        for s in &g.sources {
            if let Some(p) = &s.category_id
                && !category_ids.contains(p)
            {
                category_ids.push(p.clone());
            }
        }
        if category_ids.is_empty() {
            category_ids = if self.scope_ids.is_empty() {
                self.base.categories.iter().map(|p| p.id.clone()).collect()
            } else {
                self.scope_ids.clone()
            };
        }
        let term_ids = g.term_ids.clone();
        let found = self
            .ctx
            .state
            .with_db(move |db| {
                let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                glossary::prompt_entries_in(db.conn(), &category_ids, &refs, &term_ids)
            })
            .await;
        match found {
            Ok(entries) => prompts::glossary_lines(&entries),
            Err(e) => {
                tracing::debug!(error = %e.message, "glossary lines for the prompt failed");
                String::new()
            }
        }
    }

    /// The attached file as sources (FR-LLM-AS-11): all its parts when they fit `budget`, else
    /// the parts closest in meaning to `query`, in file order. Returns the tokens used.
    async fn file_parts(
        &self,
        file: &AttachedFile,
        query: &str,
        budget: usize,
    ) -> (Vec<SourcePiece>, Vec<SourceRef>, usize) {
        let pieces = sources::pieces(&file.text);
        let n = pieces.len();
        let cost = |t: &str| estimate_tokens(t) + 20;
        let total: usize = pieces.iter().map(|p| cost(p)).sum();
        let mut used = total;
        let chosen: Vec<usize> = if total <= budget {
            (0..n).collect()
        } else {
            let scores = match score_by_meaning(self.ctx, &pieces, query).await {
                Ok(s) => s,
                Err(_) => score_by_words(&pieces, query),
            };
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]));
            used = 0;
            let mut keep = Vec::new();
            for i in order {
                let c = cost(&pieces[i]);
                if used + c <= budget {
                    used += c;
                    keep.push(i);
                }
            }
            keep.sort_unstable();
            keep
        };
        let mut out = Vec::new();
        let mut refs = Vec::new();
        for i in chosen {
            let label = format!("Part {} of {n}", i + 1);
            refs.push(SourceRef {
                kind: SourceKind::File,
                id: None,
                title: file.name.clone(),
                key: None,
                category_id: None,
                part: Some(label),
                snippet: hybrid::excerpt(&pieces[i]),
                url: None,
            });
            out.push(SourcePiece {
                item: None,
                part: PiecePart::File {
                    name: file.name.clone(),
                    index: (i + 1) as u32,
                    count: n as u32,
                },
                text: pieces[i].clone(),
            });
        }
        (out, refs, used)
    }

    /// The sources, the glossary and the earlier turns for a question or a request to change
    /// something.
    async fn gather(
        &self,
        p: &Planned,
        file: Option<&AttachedFile>,
        progress: &mut Progress,
    ) -> Result<Gathered, JobError> {
        let plan = &p.plan;
        let b = &self.budgets;
        // A general question that refers to items reads them (FR-LLM-AS-14): only those.
        let general = plan.kind == RequestKind::General;
        let pinned = if general {
            self.job.refs.clone()
        } else {
            self.pinned(p)
        };
        // An empty search on a plain question means: the message itself.
        let words = if general {
            String::new()
        } else if plan.search.is_empty()
            && plan.kind == RequestKind::Question
            && pinned.is_empty()
        {
            self.job.text.clone()
        } else {
            plan.search.clone()
        };
        let notes_wanted = if general { !pinned.is_empty() } else { !words.is_empty() || !pinned.is_empty() };
        let (mut pieces, mut sources) = (Vec::new(), Vec::new());
        let mut used = 0;
        if let Some(f) = file {
            let query = if words.is_empty() { &self.job.text } else { &words };
            let share = if notes_wanted { b.source / 2 } else { b.source };
            (pieces, sources, used) = self.file_parts(f, query, share).await;
        }
        let mut retrieved = Retrieved::default();
        if notes_wanted {
            let step = progress.begin(self.ctx, "Searching your notes").await;
            let vector = if words.is_empty() {
                None
            } else {
                self.embed(&words, progress).await
            };
            retrieved = self
                .retrieve(&words, &plan.filters, vector, b.source.saturating_sub(used), pinned)
                .await?;
            let n = retrieved.items.len();
            progress
                .relabel(
                    self.ctx,
                    step,
                    format!("Found {n} {}", if n == 1 { "item" } else { "items" }),
                )
                .await;
        }
        pieces.extend(retrieved.pieces.iter().cloned());
        sources.extend(retrieved.sources.iter().cloned());

        let fitted = history::fit(p.turns.clone(), b.history);
        let history = formats::history_messages(&fitted.turns);
        let mut texts = vec![self.job.text.clone()];
        texts.extend(pieces.iter().map(|x| x.text.clone()));
        let glossary = self.glossary(&retrieved, texts).await;
        let refs_items = plan
            .refs
            .iter()
            .filter_map(|i| p.shown.get(*i).cloned())
            .collect();
        Ok(Gathered {
            retrieved,
            pieces,
            sources,
            glossary,
            history,
            refs_items,
        })
    }

    /// `ask-answer` and what follows (FR-LLM-AS-1, -4, -9, -11).
    async fn answer(
        &self,
        p: &Planned,
        file: Option<&AttachedFile>,
        progress: &mut Progress,
    ) -> Result<Done, JobError> {
        let plan = &p.plan;
        let g = self.gather(p, file, progress).await?;
        progress
            .begin(self.ctx, format!("Writing the answer with {}", self.label))
            .await;
        let lines = self.lines();
        let scope = formats::scope_sentence(&lines);
        let out_of_scope = formats::out_of_scope_note(&plan.outside_scope);
        // The earlier items the message refers to, with their number among this turn's sources.
        let mut refs: Vec<(usize, &ItemInfo)> = Vec::new();
        for info in &g.refs_items {
            let at = g.pieces.iter().position(|x| x.item.as_ref().is_some_and(|i| i.id() == info.id()));
            if let Some(at) = at {
                refs.push((at + 1, info));
            }
        }
        let refs_note = formats::refs_note(&refs);
        let sources_text = formats::sources_block(&g.pieces);
        let question = if self.job.text.is_empty() {
            FILE_ONLY_QUESTION
        } else {
            &self.job.text
        };
        let messages = prompts::ask_answer(
            &prompts::AskAnswer {
                today: &self.today,
                scope: &scope,
                out_of_scope_note: &out_of_scope,
                refs_note: &refs_note,
                sources: &sources_text,
                glossary: &g.glossary,
                question,
            },
            &g.history,
        )
        .map_err(|e| plain(e.into()))?;
        let opts = options(
            Some(&self.job.assistant_id),
            self.job.effort.clone(),
            ANSWER_TIMEOUT_S,
        );
        let reply = self
            .ctx
            .complete(&self.resolved.target, &messages, &opts)
            .await
            .map_err(plain)?;
        let (text, cited) = markers::read(reply.text.trim(), g.sources.len());
        let basis = if plan.kind == RequestKind::General && self.job.refs.is_empty() {
            AnswerBasis::General
        } else if cited.is_empty() {
            AnswerBasis::NotInNotes
        } else {
            AnswerBasis::Notes
        };
        let mut part = AnswerPart {
            basis,
            sources: g.sources.clone(),
            cited,
            more: g.retrieved.more.clone(),
            outside_scope: plan.outside_scope.clone(),
            ..AnswerPart::default()
        };
        // The answer is stored while the last step is still running (the message is not
        // finished before it has its result).
        self.ctx
            .patch(
                &self.job.assistant_id,
                MessagePatch {
                    text: Some(text.clone()),
                    answer: Some(part.clone()),
                    ..MessagePatch::default()
                },
            )
            .await;
        if let Some(f) = file {
            self.doc_terms(f, &mut part, progress).await;
            self.ctx
                .patch(
                    &self.job.assistant_id,
                    MessagePatch {
                        answer: Some(part),
                        ..MessagePatch::default()
                    },
                )
                .await;
        }
        Ok(Done { text, card: None })
    }

    /// `doc-terms` on the default model, once per attached document (FR-KN-GL-7). Skipped quietly
    /// without a default model or when anything fails.
    async fn doc_terms(&self, file: &AttachedFile, part: &mut AnswerPart, progress: &mut Progress) {
        let Ok(default) = llm::resolve(self.ctx.state, ModelRole::Default, None).await else {
            return;
        };
        progress
            .begin(
                self.ctx,
                format!(
                    "Looking for glossary terms in {} with {}",
                    file.name, default.model.label
                ),
            )
            .await;
        let budget = self.budgets.source;
        let (source, excerpts) = if estimate_tokens(&file.text) <= budget {
            (file.text.clone(), false)
        } else {
            let picked = files::pick_excerpts(
                self.ctx,
                &file.text,
                "abbreviations, codes and names defined or used in this document",
                budget,
            )
            .await;
            (picked, true)
        };
        // The terms are suggested for the first in-scope category.
        let category_id = match self.scope_ids.first() {
            Some(p) => p.clone(),
            None => match self
                .ctx
                .state
                .with_db(|db| files::general_category(db))
                .await
            {
                Ok(p) => p,
                Err(_) => return,
            },
        };
        let glossary_lines = {
            let (pid, texts) = (category_id.clone(), vec![source.clone()]);
            self.ctx.glossary(&pid, texts).await
        };
        let Ok(messages) = prompts::doc_terms(&file.name, &source, excerpts, &glossary_lines) else {
            return;
        };
        let opts = options(None, None, PLAN_TIMEOUT_S);
        let reply = match json_reply(self.ctx, &default.target, messages, &opts).await {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(e) => {
                tracing::debug!(kind = %e.kind, "doc-terms failed; no suggestions");
                return;
            }
        };
        let raw: Vec<RawTerm> = reply
            .get("terms")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter_map(|o| {
                let text = |key: &str| o.get(key).and_then(Value::as_str).map(str::to_string);
                Some(RawTerm {
                    term: text("term")?,
                    meaning: text("meaning").unwrap_or_default(),
                    confidence: text("confidence").unwrap_or_default(),
                })
            })
            .collect();
        let (pid, doc) = (category_id.clone(), source);
        let filtered = self
            .ctx
            .state
            .with_db(move |db| checks::filter_terms(db.conn(), &pid, &doc, raw, &[]))
            .await;
        match filtered {
            Ok(f) => {
                // NFR-SEC-3: the reason only, never the term (text of the document).
                for (_, why) in &f.dropped {
                    tracing::debug!(why, "doc-terms dropped");
                }
                part.shown_terms = f.kept.iter().map(|t| t.term.clone()).collect();
                part.terms = f.kept;
                part.terms_category_id = Some(category_id);
            }
            Err(e) => tracing::debug!(error = %e.message, "doc-terms filter failed"),
        }
    }

    /// A find request (FR-LLM-AS-2): Rust lists the matches under the plan's intro.
    async fn find(&self, p: &Planned, progress: &mut Progress) -> Result<Done, JobError> {
        let plan = &p.plan;
        let step = progress.begin(self.ctx, "Searching your notes").await;
        let matching: Option<Vec<ItemRef>> = if plan.search.is_empty() {
            None
        } else {
            let vector = self.embed(&plan.search, progress).await;
            let found = self
                .retrieve(&plan.search, &plan.filters, vector, UNLIMITED_TOKENS, Vec::new())
                .await?;
            Some(found.items)
        };
        let (scope, filters) = (self.scope_ids.clone(), plan.filters.clone());
        let today = find::local_today();
        let items = self
            .ctx
            .state
            .with_db(move |db| {
                find::find(
                    db.conn(),
                    &FindQuery {
                        scope_category_ids: &scope,
                        filters: &filters,
                        matching: matching.as_deref(),
                        today,
                    },
                )
            })
            .await
            .map_err(plain)?;
        let n = items.len();
        progress
            .relabel(
                self.ctx,
                step,
                format!("Found {n} {}", if n == 1 { "item" } else { "items" }),
            )
            .await;
        let intro = if plan.intro.is_empty() {
            "Matching items:".to_string()
        } else {
            plan.intro.clone()
        };
        self.ctx
            .patch(
                &self.job.assistant_id,
                MessagePatch {
                    text: Some(intro.clone()),
                    find: Some(FindPart {
                        intro: intro.clone(),
                        items,
                        outside_scope: plan.outside_scope.clone(),
                    }),
                    ..MessagePatch::default()
                },
            )
            .await;
        Ok(Done {
            text: intro,
            card: None,
        })
    }

    /// The attached file becomes a draft page: the M5 file flow on this message.
    async fn draft(&self, file: &AttachedFile, progress: &mut Progress) -> Result<Done, JobError> {
        let category_id = match self.scope_ids.first() {
            Some(p) => p.clone(),
            None => self
                .ctx
                .state
                .with_db(|db| files::general_category(db))
                .await
                .map_err(plain)?,
        };
        let kind = self
            .base
            .all
            .iter()
            .find(|m| m.id == self.job.user_message_id)
            .and_then(|m| m.attachment.as_ref().map(|a| a.kind))
            .unwrap_or(FileKind::Text);
        let file_job = FileJob {
            conversation_id: self.job.conversation_id.clone(),
            user_message_id: self.job.user_message_id.clone(),
            assistant_id: self.job.assistant_id.clone(),
            upload: None,
            name: file.name.clone(),
            kind,
            instruction: self.job.text.clone(),
            model_id: self.job.model_id.clone(),
            effort: self.job.effort.clone(),
            category_id,
            chat_file_id: Some(file.id.clone()),
        };
        files::file_job(self.ctx, &file_job, progress).await
    }

    /// What the action branch gets (`commands::actions`): every category (archived too), the
    /// conversation's in-scope ones first; the candidates in the order they are labelled.
    fn action_input(&self, p: &Planned, g: &Gathered) -> super::actions::ActionInput {
        let mut candidates: Vec<ItemRef> = g.retrieved.items.clone();
        let refs = p.plan.refs.iter().filter_map(|i| p.shown.get(*i));
        for info in refs.chain(p.shown.iter()) {
            let r = item_ref(info);
            if !candidates.contains(&r) {
                candidates.push(r);
            }
        }
        let in_scope = |id: &str| self.scope_ids.is_empty() || self.scope_ids.iter().any(|s| s == id);
        let mut categories: Vec<PlanCategory> = self
            .base
            .categories_all
            .iter()
            .map(|pr| PlanCategory {
                id: pr.id.clone(),
                name: pr.name.clone(),
                in_scope: in_scope(&pr.id),
            })
            .collect();
        // Stable: the in-scope ones first, each group in the categories' own order.
        categories.sort_by_key(|pr| !pr.in_scope);
        super::actions::ActionInput {
            candidates,
            sources: g.pieces.clone(),
            history: g.history.clone(),
            categories,
            today: self.today.clone(),
            glossary: g.glossary.clone(),
        }
    }
}

/// What the plan step found out besides the plan.
struct Planned {
    plan: Plan,
    /// The items shown with the previous answer, in display order (`c1` is the first).
    shown: Vec<ItemInfo>,
    /// The previous answer was a find list (its rows are not sent again).
    shown_is_find: bool,
    turns: Vec<Turn>,
}


#[cfg(test)]
mod tests;
