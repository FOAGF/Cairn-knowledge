//! Creating a page from a file in the Assistant chat (FR-KN-UP-1..10, FR-KN-UP-7, D-41), and
//! revising the draft by chatting.
//!
//! The upload arrives in base64 chunks (no size limit, FR-KN-UP-2) and is written to
//! `DataPaths::uploads_dir`. `start_file_draft` then runs in the background as a job named after
//! the assistant message (*Stop* = `cancel_job(<message id>)`):
//! 1. `file.extract` (sidecar); 2. OCR of scanned pages and images with the OCR model, a few pages
//!    per call (`prompts::ocr`), progress in the message's steps; 3. the upload and page images are
//!    deleted and the text stored (`conversations::add_chat_file`); 4. `.md`/`.txt` without an
//!    instruction become the draft as they are (FR-KN-UP-8); otherwise the conversation's model
//!    drafts (`draft-file`, or excerpts / parts per `cairn_core::sources`, FR-KN-UP-4a), streamed
//!    as `LlmDelta` with the message id as stream id; 5. the `check` call (default model) fills the
//!    card's group, tags, topics and glossary suggestions; 6. the message gets its card.
//! Every step updates the stored message and emits `DataChanged { conversations }`. A failure puts
//! a `MessageError` on the message (with `can_use_as_is` when the text was read).
//!
//! The job logic runs on a [`Ctx`] (state, sidecars, and the app handle when there is one), so the
//! tests can run it against the fake sidecar without a window. *Stop* aborts the job's future;
//! [`StopGuard`] then writes the "Stopped." error from its `Drop`.
//!
//! Where the job's inputs are kept, so *Retry* can run it again:
//! - the file's text: `chat_files` (the user message's attachment names the row);
//! - the instruction: the user message's text; the category: `extra` of the assistant message
//!   (`{"categoryId": …}`); the model: the assistant message's `model_id`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cairn_core::assistant::references;
use cairn_core::chunking::estimate_tokens;
use cairn_core::conversations::{
    self, ChatAttachment, ChatMessage, ChatRole, ChatStep, DraftCard, DraftState, FileKind,
    MessageError, MessagePatch, NewMessage, StepState,
};
use cairn_core::prompts::{self, LlmMessage, ModelRole, Role};
use cairn_core::sources::{self, SourcePlan};
use cairn_core::{DataPaths, checks::GroupPick, glossary, replies};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::commands::checks::{CheckInput, run_check};
use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::llm::{self, CallOptions, DraftText, Jobs};
use crate::sidecar::{Sidecars, Worker};
use crate::state::AppState;

/// Scanned pages read at the same time (FR-KN-UP-4b).
const OCR_PARALLEL: usize = 4;
/// Seconds per attempt (PROTOCOL.md `timeout_s`).
const OCR_TIMEOUT_S: u32 = 120;
const DRAFT_TIMEOUT_S: u32 = 300;
/// The part drafts are combined in groups that fit; a text that still doesn't fit gives up here.
const MAX_FIT_TRIES: usize = 8;
/// Below this many tokens a refused source is not cut again.
const MIN_FIT_TOKENS: usize = 500;

const FILE_TYPES: &str = ".md, .txt, .pdf, .docx, .png, .jpg and .jpeg";

// ---------------------------------------------------------------- uploads

pub(crate) struct Upload {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) kind: FileKind,
    pub(crate) size: u32,
    pub(crate) written: u64,
}

/// Uploads in progress: upload id → the file being written.
#[derive(Default)]
pub struct Uploads {
    files: Mutex<HashMap<String, Upload>>,
    /// Leftovers from an earlier run are deleted once, at the first upload (FR-KN-UP-6).
    swept: AtomicBool,
}

impl Uploads {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Upload>> {
        self.files.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn take(&self, id: &str) -> ApiResult<Upload> {
        self.lock().remove(id).ok_or_else(|| ApiError {
            kind: "not_found".into(),
            message: "This upload is no longer there. Attach the file again.".into(),
        })
    }
}

pub(crate) fn invalid(message: impl Into<String>) -> ApiError {
    ApiError {
        kind: "invalid".into(),
        message: message.into(),
    }
}

/// FR-KN-UP-1: the file type from the extension.
fn kind_of(file_name: &str) -> Option<(FileKind, String)> {
    let ext = file_name.rsplit_once('.')?.1.to_ascii_lowercase();
    let kind = match ext.as_str() {
        "md" | "txt" => FileKind::Text,
        "pdf" => FileKind::Pdf,
        "docx" => FileKind::Docx,
        "png" | "jpg" | "jpeg" => FileKind::Image,
        _ => return None,
    };
    Some((kind, ext))
}

pub(crate) fn remove_path(path: &std::path::Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Removes everything in the uploads folder (the files of an earlier run).
fn sweep_uploads(paths: &DataPaths) {
    if let Ok(entries) = std::fs::read_dir(paths.uploads_dir()) {
        for entry in entries.flatten() {
            remove_path(&entry.path());
        }
    }
}

/// Starts an upload. Refuses types other than FR-KN-UP-1's with a clear message. Returns the id.
#[tauri::command]
#[specta::specta]
pub async fn upload_begin(
    state: State<'_, AppState>,
    uploads: State<'_, Uploads>,
    file_name: String,
    size: u32,
) -> ApiResult<String> {
    begin_upload(&state.paths, &uploads, &file_name, size)
}

pub(crate) fn begin_upload(
    paths: &DataPaths,
    uploads: &Uploads,
    file_name: &str,
    size: u32,
) -> ApiResult<String> {
    let (kind, ext) = kind_of(file_name).ok_or_else(|| {
        invalid(format!(
            "Cairn can read {FILE_TYPES} files. \"{file_name}\" is not one of them."
        ))
    })?;
    let dir = paths.uploads_dir();
    std::fs::create_dir_all(&dir).map_err(cairn_core::CoreError::from)?;
    if !uploads.swept.swap(true, Ordering::SeqCst) {
        sweep_uploads(paths);
    }
    let id = cairn_core::util::new_id();
    let path = dir.join(format!("{id}.{ext}"));
    std::fs::write(&path, b"").map_err(cairn_core::CoreError::from)?;
    uploads.lock().insert(
        id.clone(),
        Upload {
            path,
            name: file_name.to_string(),
            kind,
            size,
            written: 0,
        },
    );
    Ok(id)
}

/// Appends a chunk (base64, at most a few MB) to the upload.
#[tauri::command]
#[specta::specta]
pub async fn upload_chunk(
    uploads: State<'_, Uploads>,
    upload_id: String,
    data_base64: String,
) -> ApiResult<()> {
    append_chunk(&uploads, &upload_id, &data_base64)
}

pub(crate) fn append_chunk(uploads: &Uploads, upload_id: &str, data_base64: &str) -> ApiResult<()> {
    use std::io::Write;
    let bytes = STANDARD
        .decode(data_base64)
        .map_err(|_| invalid("A part of the file could not be read. Attach the file again."))?;
    let mut files = uploads.lock();
    let up = files.get_mut(upload_id).ok_or_else(|| ApiError {
        kind: "not_found".into(),
        message: "This upload is no longer there. Attach the file again.".into(),
    })?;
    if up.written + bytes.len() as u64 > u64::from(up.size) {
        return Err(invalid(
            "The file is larger than announced. Attach it again.",
        ));
    }
    std::fs::OpenOptions::new()
        .append(true)
        .open(&up.path)
        .and_then(|mut f| f.write_all(&bytes))
        .map_err(cairn_core::CoreError::from)?;
    up.written += bytes.len() as u64;
    Ok(())
}

/// Drops an unfinished upload and its file.
#[tauri::command]
#[specta::specta]
pub async fn upload_cancel(uploads: State<'_, Uploads>, upload_id: String) -> ApiResult<()> {
    if let Some(up) = uploads.lock().remove(&upload_id) {
        remove_path(&up.path);
    }
    Ok(())
}

/// A file that was dropped as a link only.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DroppedFile {
    pub name: String,
    pub data_base64: String,
}

/// Reads a file dropped as a `file://` link. Linux file managers often send only the link
/// (`text/uri-list`), with no file data in the drop. Only the types the chat and the editor take
/// (FR-KN-UP-1) are read.
#[tauri::command]
#[specta::specta]
pub async fn read_dropped_file(uri: String) -> ApiResult<DroppedFile> {
    tauri::async_runtime::spawn_blocking(move || read_dropped(&uri))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
}

fn read_dropped(uri: &str) -> ApiResult<DroppedFile> {
    let path = url::Url::parse(uri.trim())
        .ok()
        .filter(|u| u.scheme() == "file")
        .and_then(|u| u.to_file_path().ok())
        .ok_or_else(|| invalid("Cairn couldn’t read the dropped item. Use the attach button instead."))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if kind_of(&name).is_none() {
        return Err(invalid(format!(
            "Cairn can read {FILE_TYPES} files. \"{name}\" is not one of them."
        )));
    }
    if !path.is_file() {
        return Err(invalid(format!("\"{name}\" is not a file Cairn can open.")));
    }
    let bytes = std::fs::read(&path).map_err(cairn_core::CoreError::from)?;
    Ok(DroppedFile {
        name,
        data_base64: STANDARD.encode(bytes),
    })
}

// ---------------------------------------------------------------- commands

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileDraftInput {
    pub conversation_id: String,
    pub upload_id: String,
    /// The message typed with the file (FR-KN-UP-3); may be empty.
    pub instruction: String,
    /// The model picked in the chat (FR-KN-UP-9); `None` = the default model.
    pub model_id: Option<String>,
    /// The category the draft goes to (the conversation's scope, else General); changeable later.
    pub category_id: String,
    /// D-65: the effort level picked in the chat (`None` = the model's default, or none).
    #[specta(optional)]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReviseInput {
    pub conversation_id: String,
    pub text: String,
    pub model_id: Option<String>,
    /// D-65: the effort level picked in the chat (`None` = the model's default, or none).
    #[specta(optional)]
    pub effort: Option<String>,
}

/// The messages a chat job added; the assistant message's id is also the job and stream id.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatJobStarted {
    pub user_message_id: String,
    pub assistant_message_id: String,
}

/// Adds the user message (with the file) and the assistant message, then runs the job above.
#[tauri::command]
#[specta::specta]
pub async fn start_file_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    uploads: State<'_, Uploads>,
    input: FileDraftInput,
) -> ApiResult<ChatJobStarted> {
    let _ = &sidecars;
    let (job, started) = prepare_file_draft(&state, &uploads, input).await?;
    emit_changed(&app, &[Entity::Conversations]);
    spawn_job(
        &app,
        &jobs,
        started.assistant_message_id.clone(),
        Job::File(job),
    );
    Ok(started)
}

/// The two messages of a file draft, and the job that fills the second.
async fn prepare_file_draft(
    state: &AppState,
    uploads: &Uploads,
    input: FileDraftInput,
) -> ApiResult<(FileJob, ChatJobStarted)> {
    let upload = uploads.take(&input.upload_id)?;
    if upload.written != u64::from(upload.size) {
        remove_path(&upload.path);
        return Err(invalid("The upload did not finish. Attach the file again."));
    }
    let instruction = input.instruction.trim().to_string();
    let (model_id, model_label) = chat_model(state, input.model_id.clone()).await;
    let effort = input.effort.clone();
    let attachment = ChatAttachment {
        name: upload.name.clone(),
        size: upload.size,
        kind: upload.kind,
        chat_file_id: None,
    };
    let (conversation_id, category_id) = (input.conversation_id.clone(), input.category_id.clone());
    let text = instruction.clone();
    let (user, assistant) = state
        .with_db(move |db| {
            let user = conversations::add_message(
                db,
                &conversation_id,
                NewMessage {
                    role: ChatRole::User,
                    text,
                    model_id: None,
                    model_label: None,
                    effort: None,
                    attachment: Some(attachment),
                    steps: None,
                    draft: None,
                    answer: None,
                    find: None,
                    proposal: None,
                    extra: None,
                },
            )?;
            let assistant =
                add_assistant(db, &conversation_id, model_id, model_label, effort, &category_id)?;
            Ok((user, assistant))
        })
        .await?;
    let job = FileJob {
        conversation_id: input.conversation_id,
        user_message_id: user.id.clone(),
        assistant_id: assistant.id.clone(),
        name: upload.name.clone(),
        kind: upload.kind,
        upload: Some(upload),
        instruction,
        model_id: assistant.model_id.clone(),
        effort: input.effort,
        category_id: input.category_id,
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

/// A chat message while the conversation has an active draft card (FR-KN-UP-7): `revise` with the
/// stored source text; a page reply becomes a new card (the old one `superseded`, then the check
/// call for its group, tags and new terms), any other reply a plain answer.
#[tauri::command]
#[specta::specta]
pub async fn revise_draft(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    input: ReviseInput,
) -> ApiResult<ChatJobStarted> {
    let _ = &sidecars;
    let (job, started) = prepare_revise(&state, input).await?;
    emit_changed(&app, &[Entity::Conversations]);
    spawn_job(
        &app,
        &jobs,
        started.assistant_message_id.clone(),
        Job::Revise(job),
    );
    Ok(started)
}

async fn prepare_revise(
    state: &AppState,
    input: ReviseInput,
) -> ApiResult<(ReviseJob, ChatJobStarted)> {
    let conversation_id = input.conversation_id.clone();
    let card = state
        .with_db(move |db| {
            let all = conversations::messages(db.conn(), &conversation_id)?;
            Ok(active_card(&all).map(|(_, c)| c.clone()))
        })
        .await?
        .ok_or_else(|| invalid("There is no draft to revise in this conversation."))?;
    let request = input.text.trim().to_string();
    if request.is_empty() {
        return Err(invalid("Write what should change in the draft."));
    }
    let (model_id, model_label) = chat_model(state, input.model_id.clone()).await;
    let effort = input.effort.clone();
    let (conversation_id, category_id, text) = (
        input.conversation_id.clone(),
        card.category_id.clone(),
        request.clone(),
    );
    let (user, assistant) = state
        .with_db(move |db| {
            let user = conversations::add_message(
                db,
                &conversation_id,
                NewMessage {
                    role: ChatRole::User,
                    text,
                    model_id: None,
                    model_label: None,
                    effort: None,
                    attachment: None,
                    steps: None,
                    draft: None,
                    answer: None,
                    find: None,
                    proposal: None,
                    extra: None,
                },
            )?;
            let assistant =
                add_assistant(db, &conversation_id, model_id, model_label, effort, &category_id)?;
            Ok((user, assistant))
        })
        .await?;
    let job = ReviseJob {
        conversation_id: input.conversation_id,
        assistant_id: assistant.id.clone(),
        request,
        model_id: assistant.model_id.clone(),
        effort: input.effort,
    };
    Ok((
        job,
        ChatJobStarted {
            user_message_id: user.id,
            assistant_message_id: assistant.id,
        },
    ))
}

/// *Retry* on a failed assistant message: runs its job again from the stored text (or, if the
/// text wasn't read yet, reports that the file has to be attached again).
#[tauri::command]
#[specta::specta]
pub async fn retry_chat_job(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    message_id: String,
    model_id: Option<String>,
) -> ApiResult<()> {
    let _ = &sidecars;
    let id = message_id.clone();
    let (job, assistant) = state
        .with_db(move |db| plan_retry(db, &id, model_id))
        .await?;
    emit_changed(&app, &[Entity::Conversations]);
    spawn_job(&app, &jobs, assistant, job);
    Ok(())
}

/// *Use the extracted text as is* (FR-KN-UP-10): the failed assistant message gets a card made from
/// the stored text without the LLM (`as_is`).
#[tauri::command]
#[specta::specta]
pub async fn draft_as_is(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: String,
) -> ApiResult<ChatMessage> {
    let m = state
        .with_db(move |db| use_text_as_is(db, &message_id))
        .await?;
    emit_changed(&app, &[Entity::Conversations]);
    Ok(m)
}

// ---------------------------------------------------------------- database helpers

/// The model id and label the assistant message shows (best effort: the job reports a missing
/// model itself).
pub(crate) async fn chat_model(state: &AppState, picked: Option<String>) -> (Option<String>, Option<String>) {
    match llm::resolve(state, ModelRole::Conversation, picked.clone()).await {
        Ok(r) => (Some(r.model.id), Some(r.model.label)),
        Err(_) => (picked, None),
    }
}

fn job_extra(category_id: &str) -> String {
    json!({ "categoryId": category_id }).to_string()
}

fn add_assistant(
    db: &mut cairn_core::Db,
    conversation_id: &str,
    model_id: Option<String>,
    model_label: Option<String>,
    effort: Option<String>,
    category_id: &str,
) -> cairn_core::Result<ChatMessage> {
    conversations::add_message(
        db,
        conversation_id,
        NewMessage {
            role: ChatRole::Assistant,
            text: String::new(),
            model_id,
            model_label,
            effort,
            attachment: None,
            steps: Some(vec![]),
            draft: None,
            answer: None,
            find: None,
            proposal: None,
            extra: Some(job_extra(category_id)),
        },
    )
}

pub(crate) fn category_of(m: &ChatMessage) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(m.extra.as_deref()?).ok()?;
    Some(v.get("categoryId")?.as_str()?.to_string())
}

/// A card the user can still refine: not saved, discarded or replaced. The latest one.
fn active_card(all: &[ChatMessage]) -> Option<(&ChatMessage, &DraftCard)> {
    all.iter().rev().find_map(|m| {
        let d = m.draft.as_ref()?;
        (d.state != DraftState::Saved && !d.discarded && !d.superseded).then_some((m, d))
    })
}

pub(crate) fn previous_user_message(all: &[ChatMessage], id: &str) -> Option<ChatMessage> {
    let i = all.iter().position(|m| m.id == id)?;
    all[..i]
        .last()
        .filter(|m| m.role == ChatRole::User)
        .cloned()
}

/// Resets the failed message and says which job to run again.
pub(crate) fn plan_retry(
    db: &mut cairn_core::Db,
    message_id: &str,
    model_id: Option<String>,
) -> cairn_core::Result<(Job, String)> {
    let message = conversations::get_message(db.conn(), message_id)?;
    let all = conversations::messages(db.conn(), &message.conversation_id)?;
    let user = previous_user_message(&all, message_id)
        .filter(|_| message.role == ChatRole::Assistant)
        .ok_or_else(|| cairn_core::CoreError::Invalid("This message can't be retried.".into()))?;
    let model_id = model_id.or(message.model_id.clone());
    // M6: a message of the assistant (`extra` says so) runs its job again from the stored
    // messages.
    if let Some(job) = super::assistant::retry_job(db, &message, &user, model_id.clone())? {
        conversations::reset_message(db, message_id)?;
        return Ok((Job::Ask(job), message.id));
    }
    let category_id = match category_of(&message) {
        Some(p) => p,
        None => general_category(db)?,
    };
    let job = match &user.attachment {
        Some(a) => {
            let file_id = a.chat_file_id.clone().ok_or_else(|| {
                cairn_core::CoreError::Invalid(
                    "The file's text was not read yet. Attach the file again.".into(),
                )
            })?;
            if conversations::chat_file(db.conn(), &file_id)?
                .text
                .is_none()
            {
                return Err(cairn_core::CoreError::Invalid(
                    "The file's text is no longer kept. Attach the file again.".into(),
                ));
            }
            Job::File(FileJob {
                conversation_id: message.conversation_id.clone(),
                user_message_id: user.id.clone(),
                assistant_id: message.id.clone(),
                upload: None,
                name: a.name.clone(),
                kind: a.kind,
                instruction: user.text.trim().to_string(),
                model_id,
                effort: message.effort.clone(),
                category_id,
                chat_file_id: Some(file_id),
            })
        }
        None => {
            if active_card(&all).is_none() {
                return Err(cairn_core::CoreError::Invalid(
                    "There is no draft to revise any more.".into(),
                ));
            }
            Job::Revise(ReviseJob {
                conversation_id: message.conversation_id.clone(),
                assistant_id: message.id.clone(),
                request: user.text.trim().to_string(),
                model_id,
                effort: message.effort.clone(),
            })
        }
    };
    conversations::update_message(
        db,
        message_id,
        MessagePatch {
            text: Some(String::new()),
            steps: Some(vec![]),
            error: Some(None),
            ..MessagePatch::default()
        },
    )?;
    Ok((job, message.id))
}

pub(crate) fn general_category(db: &cairn_core::Db) -> cairn_core::Result<String> {
    Ok(db.conn().query_row(
        "SELECT id FROM categories WHERE is_general = 1 LIMIT 1",
        [],
        |r| r.get(0),
    )?)
}

/// A card from text read as it is (FR-KN-UP-8, FR-KN-UP-10).
fn as_is_card(name: &str, text: &str, category_id: &str, chat_file_id: &str) -> DraftCard {
    let page = replies::page(text, file_stem(name));
    DraftCard {
        title: page.title,
        body: page.body,
        category_id: category_id.to_string(),
        source_info: Some(name.to_string()),
        file_name: Some(name.to_string()),
        chat_file_id: Some(chat_file_id.to_string()),
        as_is: true,
        ..DraftCard::default()
    }
}

fn use_text_as_is(db: &mut cairn_core::Db, message_id: &str) -> cairn_core::Result<ChatMessage> {
    let message = conversations::get_message(db.conn(), message_id)?;
    let all = conversations::messages(db.conn(), &message.conversation_id)?;
    let attachment = previous_user_message(&all, message_id)
        .and_then(|u| u.attachment)
        .ok_or_else(|| {
            cairn_core::CoreError::Invalid("There is no file for this message.".into())
        })?;
    let file_id = attachment.chat_file_id.ok_or_else(|| {
        cairn_core::CoreError::Invalid(
            "The file's text was not read yet. Attach the file again.".into(),
        )
    })?;
    let file = conversations::chat_file(db.conn(), &file_id)?;
    let text = file.text.ok_or_else(|| {
        cairn_core::CoreError::Invalid(
            "The file's text is no longer kept. Attach the file again.".into(),
        )
    })?;
    let category_id = match category_of(&message) {
        Some(p) => p,
        None => general_category(db)?,
    };
    let card = as_is_card(&attachment.name, &text, &category_id, &file_id);
    let steps = vec![ChatStep {
        label: "Used the extracted text as it is".into(),
        state: StepState::Done,
    }];
    conversations::update_message(
        db,
        message_id,
        MessagePatch {
            text: Some(format!("Here is a draft page from {}.", attachment.name)),
            steps: Some(steps),
            draft: Some(card),
            error: Some(None),
            ..MessagePatch::default()
        },
    )
}

fn file_stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

// ---------------------------------------------------------------- the job's environment

/// What a job needs; `app` is `None` only in tests (no window: no events, no check call).
pub(crate) struct Ctx<'a> {
    pub app: Option<&'a AppHandle>,
    pub state: &'a AppState,
    pub sidecars: &'a Sidecars,
    /// Where streamed text goes when there is no app handle (tests).
    pub sink: Option<Arc<dyn Fn(String) + Send + Sync>>,
    /// D-65: this job's model calls and the reasoning tokens they reported.
    pub tally: Tally,
}

/// D-65: what a job's model calls add up to: how many were made (attempts, so a failed one counts:
/// its effort level was sent) and the reasoning tokens the provider reported.
#[derive(Default)]
pub(crate) struct Tally {
    calls: AtomicU64,
    reasoning: AtomicU64,
}

impl Ctx<'_> {
    pub(crate) fn changed(&self) {
        if let Some(app) = self.app {
            emit_changed(app, &[Entity::Conversations]);
        }
    }

    /// Updates the stored message and tells the UI. A failure is only logged: the job goes on.
    pub(crate) async fn patch(&self, id: &str, patch: MessagePatch) {
        let id = id.to_string();
        match self
            .state
            .with_db(move |db| conversations::update_message(db, &id, patch))
            .await
        {
            Ok(_) => self.changed(),
            Err(e) => tracing::warn!(error = %e.message, "could not update a chat message"),
        }
    }

    pub(crate) async fn complete(
        &self,
        target: &cairn_core::providers::CallTarget,
        messages: &[LlmMessage],
        opts: &CallOptions,
    ) -> ApiResult<llm::Reply> {
        self.tally.calls.fetch_add(1, Ordering::Relaxed);
        let reply = match self.app {
            Some(app) => llm::complete(app, self.sidecars, target, messages, opts).await,
            None => {
                let on_delta = opts.stream_id.as_ref().map(|_| {
                    let sink = self.sink.clone();
                    Box::new(move |text: String| {
                        if let Some(sink) = &sink {
                            sink(text);
                        }
                    }) as Box<dyn Fn(String) + Send + Sync>
                });
                llm::complete_with(self.sidecars, target, messages, opts, on_delta).await
            }
        };
        if let Ok(r) = &reply
            && let Some(n) = r.usage.as_ref().and_then(|u| u.reasoning_tokens)
        {
            self.tally.reasoning.fetch_add(n, Ordering::Relaxed);
        }
        reply
    }

    /// D-65: the reasoning tokens of this job's calls so far; `None` when none were reported.
    pub(crate) fn reasoning_tokens(&self) -> Option<u32> {
        let n = self.tally.reasoning.load(Ordering::Relaxed);
        (n > 0).then(|| u32::try_from(n).unwrap_or(u32::MAX))
    }

    /// D-65: the effort level the job's calls sent, for the message: `""` when none was sent or
    /// no call was made (the job used no model, e.g. a .md file taken as it is).
    pub(crate) fn sent_effort(&self, level: Option<String>) -> Option<String> {
        let called = self.tally.calls.load(Ordering::Relaxed) > 0;
        Some(level.filter(|_| called).unwrap_or_default())
    }

    /// The glossary lines for a prompt about `texts` of `category_id` (FR-LLM-AS-8).
    pub(crate) async fn glossary(&self, category_id: &str, texts: Vec<String>) -> String {
        let category_id = category_id.to_string();
        let found = self
            .state
            .with_db(move |db| {
                let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                glossary::prompt_entries(db.conn(), &category_id, &refs)
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
}

/// The processing steps of an assistant message, written to the database as they change.
pub(crate) struct Progress {
    message_id: String,
    pub(crate) steps: Vec<ChatStep>,
    /// The file's text is stored, so *Use the extracted text as is* works.
    pub(crate) text_ready: bool,
}

impl Progress {
    pub(crate) fn new(message_id: &str) -> Self {
        Self {
            message_id: message_id.to_string(),
            steps: Vec::new(),
            text_ready: false,
        }
    }

    async fn flush(&self, ctx: &Ctx<'_>) {
        ctx.patch(
            &self.message_id,
            MessagePatch {
                steps: Some(self.steps.clone()),
                ..MessagePatch::default()
            },
        )
        .await;
    }

    /// Finishes the running step and starts a new one; returns its index.
    pub(crate) async fn begin(&mut self, ctx: &Ctx<'_>, label: impl Into<String>) -> usize {
        self.settle(StepState::Done);
        self.steps.push(ChatStep {
            label: label.into(),
            state: StepState::Active,
        });
        self.flush(ctx).await;
        self.steps.len() - 1
    }

    pub(crate) async fn relabel(&mut self, ctx: &Ctx<'_>, index: usize, label: impl Into<String>) {
        if let Some(step) = self.steps.get_mut(index) {
            step.label = label.into();
        }
        self.flush(ctx).await;
    }

    /// A finished remark that doesn't change the running step.
    pub(crate) async fn note(&mut self, ctx: &Ctx<'_>, label: impl Into<String>) {
        self.steps.push(ChatStep {
            label: label.into(),
            state: StepState::Done,
        });
        self.flush(ctx).await;
    }

    /// The running steps end as `state`.
    pub(crate) fn settle(&mut self, state: StepState) {
        for s in &mut self.steps {
            if s.state == StepState::Active {
                s.state = state;
            }
        }
    }
}

/// *Stop* drops the job's future; this writes the "Stopped." error from `Drop`, on its own thread
/// so it doesn't matter where the future was dropped.
struct StopGuard {
    db: Option<Arc<Mutex<cairn_core::Db>>>,
    app: Option<AppHandle>,
    message_id: String,
    armed: bool,
}

impl StopGuard {
    fn arm(ctx: &Ctx<'_>, message_id: &str) -> Self {
        Self {
            db: ctx.state.db_handle(),
            app: ctx.app.cloned(),
            message_id: message_id.to_string(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(db) = self.db.take() else { return };
        let (app, id) = (self.app.take(), std::mem::take(&mut self.message_id));
        std::thread::spawn(move || {
            let Ok(mut db) = db.lock() else { return };
            let result = conversations::fail_message(&mut db, &id, llm::CANCELLED, "Stopped.");
            if let Err(e) = result {
                tracing::warn!(error = %e, "could not record that a chat job was stopped");
            }
            if let Some(app) = app {
                emit_changed(&app, &[Entity::Conversations]);
            }
        });
    }
}

// ---------------------------------------------------------------- jobs

pub(crate) struct FileJob {
    pub(crate) conversation_id: String,
    pub(crate) user_message_id: String,
    pub(crate) assistant_id: String,
    /// `None` when the text is already stored (*Retry*).
    pub(crate) upload: Option<Upload>,
    pub(crate) name: String,
    pub(crate) kind: FileKind,
    pub(crate) instruction: String,
    pub(crate) model_id: Option<String>,
    /// D-65: the effort level picked in the chat, for the page-drafting calls.
    pub(crate) effort: Option<String>,
    pub(crate) category_id: String,
    pub(crate) chat_file_id: Option<String>,
}

pub(crate) struct ReviseJob {
    conversation_id: String,
    assistant_id: String,
    request: String,
    model_id: Option<String>,
    effort: Option<String>,
}

pub(crate) enum Job {
    File(FileJob),
    Revise(ReviseJob),
    /// M6: a message in an assistant conversation (`commands::assistant`).
    Ask(super::assistant::AskJob),
}

pub(crate) fn spawn_job(app: &AppHandle, jobs: &Arc<Jobs>, id: String, job: Job) {
    let app = app.clone();
    jobs.spawn(id, async move {
        let state = app.state::<AppState>();
        let sidecars = app.state::<Sidecars>();
        let ctx = Ctx {
            app: Some(&app),
            state: &state,
            sidecars: &sidecars,
            sink: None,
            tally: Tally::default(),
        };
        run_job(&ctx, job).await;
    });
}

/// A job's failure, as the message shows it.
pub(crate) struct JobError {
    pub(crate) error: ApiError,
    /// The file's text is stored: *Use the extracted text as is* is offered.
    pub(crate) can_use_as_is: bool,
}

pub(crate) async fn run_job(ctx: &Ctx<'_>, job: Job) {
    let id = match &job {
        Job::File(j) => j.assistant_id.clone(),
        Job::Revise(j) => j.assistant_id.clone(),
        Job::Ask(j) => j.assistant_id.clone(),
    };
    // D-65: the reply shows the effort level its calls sent: the picked one, else the model's default.
    let (model_id, picked) = match &job {
        Job::File(j) => (j.model_id.clone(), j.effort.clone()),
        Job::Revise(j) => (j.model_id.clone(), j.effort.clone()),
        Job::Ask(j) => (j.model_id.clone(), j.effort.clone()),
    };
    let level = match llm::resolve(ctx.state, ModelRole::Conversation, model_id).await {
        Ok(r) => r.model.effort.level_to_send(picked.as_deref()),
        Err(_) => None,
    };
    let mut guard = StopGuard::arm(ctx, &id);
    let mut progress = Progress::new(&id);
    let outcome = match job {
        Job::File(j) => file_job(ctx, &j, &mut progress).await,
        Job::Revise(j) => revise_job(ctx, &j, &mut progress).await,
        Job::Ask(j) => super::assistant::ask_job(ctx, &j, &mut progress).await,
    };
    match outcome {
        Ok(done) => {
            progress.settle(StepState::Done);
            ctx.patch(
                &id,
                MessagePatch {
                    text: Some(done.text),
                    steps: Some(progress.steps.clone()),
                    draft: done.card,
                    error: Some(None),
                    sent_effort: ctx.sent_effort(level.clone()),
                    reasoning_tokens: ctx.reasoning_tokens(),
                    ..MessagePatch::default()
                },
            )
            .await;
        }
        Err(e) => {
            tracing::debug!(kind = %e.error.kind, "chat job failed");
            progress.settle(StepState::Failed);
            ctx.patch(
                &id,
                MessagePatch {
                    steps: Some(progress.steps.clone()),
                    error: Some(Some(MessageError {
                        kind: e.error.kind,
                        message: e.error.message,
                        can_use_as_is: e.can_use_as_is || progress.text_ready,
                    })),
                    sent_effort: ctx.sent_effort(level),
                    reasoning_tokens: ctx.reasoning_tokens(),
                    ..MessagePatch::default()
                },
            )
            .await;
        }
    }
    guard.disarm();
}

/// What a job ends with: the assistant's text and a card, if there is one. An assistant job
/// stores its answer, find list or proposal on the message itself before it ends.
pub(crate) struct Done {
    pub(crate) text: String,
    pub(crate) card: Option<DraftCard>,
}

pub(crate) fn plain(error: ApiError) -> JobError {
    JobError {
        error,
        can_use_as_is: false,
    }
}

/// Deletes the upload and the page images when dropped (after the OCR step, or on *Stop*).
struct Cleanup(Vec<PathBuf>);

impl Cleanup {
    fn run(&mut self) {
        for p in self.0.drain(..) {
            remove_path(&p);
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.run();
    }
}

#[derive(Deserialize)]
struct Extracted {
    #[serde(default)]
    pages: u32,
    parts: Vec<ExtractedPart>,
}

#[derive(Deserialize)]
struct ExtractedPart {
    #[serde(default)]
    page: u32,
    text: Option<String>,
    image_path: Option<String>,
}

/// The reply of one OCR call as page text (README "ocr").
fn ocr_text(reply: &str) -> String {
    let t = replies::strip_fence(reply).trim();
    if t.eq_ignore_ascii_case("[no text]") {
        String::new()
    } else {
        t.to_string()
    }
}

/// One scanned page through the OCR model: the page's index and its text.
async fn ocr_page(
    ctx: &Ctx<'_>,
    target: &cairn_core::providers::CallTarget,
    path: &str,
    opts: &CallOptions,
    index: usize,
) -> ApiResult<(usize, String)> {
    let messages = prompts::ocr(&[path.to_string()])?;
    let reply = ctx.complete(target, &messages, opts).await?;
    Ok((index, ocr_text(&reply.text)))
}

/// Steps 1–3: the file's text, read and stored. Returns it with the `chat_files` id.
pub(crate) async fn read_file(
    ctx: &Ctx<'_>,
    job: &FileJob,
    upload: &Upload,
    progress: &mut Progress,
) -> Result<(String, String), JobError> {
    let out_dir = upload.path.with_extension("");
    let mut cleanup = Cleanup(vec![upload.path.clone(), out_dir.clone()]);
    progress
        .begin(ctx, format!("Reading {}", upload.name))
        .await;
    let answer = ctx
        .sidecars
        .call(
            Worker::Ai,
            "file.extract",
            json!({ "path": upload.path, "out_dir": out_dir }),
        )
        .await
        .map_err(|e| plain(e.into()))?;
    let extracted: Extracted = serde_json::from_value(answer).map_err(|_| {
        plain(ApiError::internal(
            "The AI worker sent a file reading Cairn doesn't understand.",
        ))
    })?;
    let mut parts = extracted.parts;
    parts.sort_by_key(|p| p.page);

    // Scanned pages and images go to the OCR model, a few at a time.
    let scanned: Vec<(usize, String)> = parts
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.image_path.clone().map(|path| (i, path)))
        .collect();
    let mut read: HashMap<usize, String> = HashMap::new();
    if !scanned.is_empty() {
        let ocr = llm::resolve(ctx.state, ModelRole::Ocr, job.model_id.clone())
            .await
            .map_err(plain)?;
        let total = scanned.len();
        let label = |n: usize| {
            format!(
                "Reading page {} of {total} with {}",
                (n + 1).min(total),
                ocr.model.label
            )
        };
        let step = progress.begin(ctx, label(0)).await;
        let opts = CallOptions {
            stream_id: None,
            timeout_s: Some(OCR_TIMEOUT_S),
            max_tokens: None,
            effort: None,
            web_search: None,
        };
        // Boxed, because a closure over borrowed state confuses the compiler's Send check.
        type PageRead<'a> = std::pin::Pin<
            Box<dyn std::future::Future<Output = ApiResult<(usize, String)>> + Send + 'a>,
        >;
        let mut calls: Vec<PageRead<'_>> = Vec::with_capacity(total);
        for (i, path) in &scanned {
            calls.push(Box::pin(ocr_page(ctx, &ocr.target, path, &opts, *i)));
        }
        let mut results = futures_util::stream::iter(calls).buffered(OCR_PARALLEL);
        let mut finished = 0;
        while let Some(result) = results.next().await {
            let (i, text) = result.map_err(plain)?;
            read.insert(i, text);
            finished += 1;
            progress.relabel(ctx, step, label(finished)).await;
        }
    }
    // FR-KN-UP-6: the upload and the page images go as soon as the OCR step is done.
    cleanup.run();

    let text = parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            read.remove(&i)
                .or_else(|| p.text.clone())
                .unwrap_or_default()
        })
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.is_empty() {
        return Err(plain(ApiError {
            kind: "no_text".into(),
            message: format!("No text could be read from {}.", upload.name),
        }));
    }
    let (conversation_id, message_id, name, kind, size, stored) = (
        job.conversation_id.clone(),
        job.user_message_id.clone(),
        upload.name.clone(),
        upload.kind,
        upload.size,
        text.clone(),
    );
    let pages = extracted.pages.max(1);
    let file_id = ctx
        .state
        .with_db(move |db| {
            let id = conversations::add_chat_file(
                db,
                &conversation_id,
                &message_id,
                &name,
                kind,
                size,
                pages,
                &stored,
            )?;
            conversations::update_message(
                db,
                &message_id,
                MessagePatch {
                    attachment: Some(ChatAttachment {
                        name,
                        size,
                        kind,
                        chat_file_id: Some(id.clone()),
                    }),
                    ..MessagePatch::default()
                },
            )?;
            Ok(id)
        })
        .await
        .map_err(plain)?;
    ctx.changed();
    progress.text_ready = true;
    Ok((file_id, text))
}

pub(crate) async fn file_job(ctx: &Ctx<'_>, job: &FileJob, progress: &mut Progress) -> Result<Done, JobError> {
    let (file_id, text) = match (&job.upload, &job.chat_file_id) {
        (Some(upload), _) => read_file(ctx, job, upload, progress).await?,
        (None, Some(id)) => {
            let (conn_id,) = (id.clone(),);
            let file = ctx
                .state
                .with_db(move |db| conversations::chat_file(db.conn(), &conn_id))
                .await
                .map_err(plain)?;
            let text = file.text.ok_or_else(|| {
                plain(invalid(
                    "The file's text is no longer kept. Attach the file again.",
                ))
            })?;
            progress.text_ready = true;
            (file.id, text)
        }
        (None, None) => return Err(plain(invalid("The file's text was not read yet."))),
    };

    let base = DraftCard {
        category_id: job.category_id.clone(),
        source_info: Some(job.name.clone()),
        file_name: Some(job.name.clone()),
        chat_file_id: Some(file_id.clone()),
        ..DraftCard::default()
    };
    let mut card = if job.kind == FileKind::Text && job.instruction.is_empty() {
        // FR-KN-UP-8: .md and .txt come in as they are; only the group and tags are suggested.
        as_is_card(&job.name, &text, &job.category_id, &file_id)
    } else {
        let drafted = draft_file(ctx, job, &text, progress)
            .await
            .map_err(|error| JobError {
                error,
                can_use_as_is: true,
            })?;
        DraftCard {
            title: drafted.title,
            body: drafted.body,
            incomplete: drafted.incomplete,
            ..base
        }
    };
    suggest(ctx, progress, &mut card, &[]).await;
    Ok(Done {
        text: format!("Here is a draft page from {}.", job.name),
        card: Some(card),
    })
}

// ---------------------------------------------------------------- drafting

/// Tracks the source budget while a model refuses too long input (FR-KN-UP-4a).
struct Fit {
    tokens: usize,
    budget: Option<usize>,
    has_instruction: bool,
    tries: usize,
}

impl Fit {
    fn new(text: &str, budget: Option<usize>, has_instruction: bool) -> Self {
        Self {
            tokens: estimate_tokens(text),
            budget,
            has_instruction,
            tries: 0,
        }
    }

    /// The plan and the tokens the source may take.
    fn plan(&self, text: &str) -> (SourcePlan, usize) {
        let allowed = self.budget.map_or(self.tokens, |b| b.min(self.tokens));
        (
            sources::plan(text, self.budget, self.has_instruction),
            allowed,
        )
    }

    /// The model said `too_long`: halve what was sent. `false` when there is nothing left to cut.
    fn shrink(&mut self, sent: usize) -> bool {
        self.tries += 1;
        let next = sent / 2;
        if self.tries >= MAX_FIT_TRIES || next < MIN_FIT_TOKENS {
            return false;
        }
        self.budget = Some(next);
        true
    }
}

fn is_too_long(e: &ApiError) -> bool {
    e.kind == "ai_too_long"
}

struct Drafted {
    title: String,
    body: String,
    incomplete: bool,
}

fn page_options(stream: Option<&str>, effort: Option<String>) -> CallOptions {
    CallOptions {
        stream_id: stream.map(str::to_string),
        timeout_s: Some(DRAFT_TIMEOUT_S),
        max_tokens: None,
        effort,
        web_search: None,
    }
}

/// Step 4: the conversation's model drafts the page (FR-KN-UP-3, 4a, 5, 9).
async fn draft_file(
    ctx: &Ctx<'_>,
    job: &FileJob,
    text: &str,
    progress: &mut Progress,
) -> ApiResult<Drafted> {
    let resolved = llm::resolve(ctx.state, ModelRole::Conversation, job.model_id.clone()).await?;
    let label = resolved.model.label.clone();
    let step = progress
        .begin(ctx, format!("Drafting the page with {label}"))
        .await;
    let size = llm::context_size(ctx.sidecars, &resolved).await;
    let mut fit = Fit::new(text, sources::budget(size), !job.instruction.is_empty());
    let stream = job.assistant_id.as_str();
    loop {
        let (plan, allowed) = fit.plan(text);
        let attempt: ApiResult<Drafted> = match plan {
            SourcePlan::Whole | SourcePlan::Excerpts => {
                let excerpts = plan == SourcePlan::Excerpts;
                let source = if excerpts {
                    pick_excerpts(ctx, text, &job.instruction, allowed).await
                } else {
                    text.to_string()
                };
                let glossary = ctx
                    .glossary(
                        &job.category_id,
                        vec![source.clone(), job.instruction.clone()],
                    )
                    .await;
                let messages =
                    prompts::draft_file(&job.name, &source, &job.instruction, excerpts, &glossary)?;
                ctx.complete(&resolved.target, &messages, &page_options(Some(stream), job.effort.clone()))
                    .await
                    .map(|reply| from_reply(&reply, file_stem(&job.name)))
            }
            SourcePlan::Parts(parts) => {
                draft_in_parts(ctx, job, &resolved, &parts, allowed, progress, step, &label).await
            }
        };
        match attempt {
            Err(e) if is_too_long(&e) && fit.shrink(allowed) => continue,
            other => return other,
        }
    }
}

fn from_reply(reply: &llm::Reply, fallback: &str) -> Drafted {
    let d = DraftText::from_reply(reply, fallback);
    Drafted {
        title: d.title,
        body: d.body,
        incomplete: d.incomplete,
    }
}

/// FR-KN-UP-4a, no instruction: each part is drafted, then the drafts are combined into one page
/// (in groups first when the drafts together are still too long).
#[allow(clippy::too_many_arguments)]
async fn draft_in_parts(
    ctx: &Ctx<'_>,
    job: &FileJob,
    resolved: &llm::Resolved,
    parts: &[String],
    budget: usize,
    progress: &mut Progress,
    step: usize,
    label: &str,
) -> ApiResult<Drafted> {
    let n = parts.len();
    let mut incomplete = false;
    let mut drafts: Vec<String> = Vec::with_capacity(n);
    for (i, part) in parts.iter().enumerate() {
        progress
            .relabel(
                ctx,
                step,
                format!("Drafting part {} of {n} with {label}", i + 1),
            )
            .await;
        let glossary = ctx.glossary(&job.category_id, vec![part.clone()]).await;
        let messages = prompts::draft_part(&job.name, i + 1, n, part, &glossary)?;
        let reply = ctx
            .complete(&resolved.target, &messages, &page_options(None, job.effort.clone()))
            .await?;
        incomplete |= reply.cut_off();
        drafts.push(replies::page(&reply.text, file_stem(&job.name)).body);
    }
    let total: usize = drafts.iter().map(|d| estimate_tokens(d)).sum();
    // Groups of drafts that fit are combined first, so the last call can too.
    while drafts.len() > 1 && drafts.iter().map(|d| estimate_tokens(d)).sum::<usize>() > budget {
        let mut next = Vec::new();
        let mut group: Vec<String> = Vec::new();
        let mut used = 0;
        for d in drafts.drain(..) {
            let t = estimate_tokens(&d);
            if !group.is_empty() && used + t > budget {
                next.push(std::mem::take(&mut group));
                used = 0;
            }
            used += t;
            group.push(d);
        }
        next.push(group);
        if next.iter().all(|g| g.len() == 1) {
            drafts = next.into_iter().flatten().collect();
            break;
        }
        progress
            .relabel(ctx, step, format!("Combining the parts with {label}"))
            .await;
        for group in next {
            if group.len() == 1 {
                drafts.extend(group);
                continue;
            }
            let glossary = ctx.glossary(&job.category_id, group.clone()).await;
            let messages = prompts::draft_combine(&job.name, &group, &glossary)?;
            let reply = ctx
                .complete(&resolved.target, &messages, &page_options(None, job.effort.clone()))
                .await?;
            incomplete |= reply.cut_off();
            drafts.push(replies::page(&reply.text, file_stem(&job.name)).body);
        }
    }
    progress
        .relabel(ctx, step, format!("Combining the parts with {label}"))
        .await;
    let glossary = ctx.glossary(&job.category_id, drafts.clone()).await;
    let messages = prompts::draft_combine(&job.name, &drafts, &glossary)?;
    let reply = ctx
        .complete(
            &resolved.target,
            &messages,
            &page_options(Some(&job.assistant_id), job.effort.clone()),
        )
        .await?;
    let mut out = from_reply(&reply, file_stem(&job.name));
    out.incomplete |= incomplete;
    // README "Local checks": a combined page far shorter than the drafts was probably summarised.
    if estimate_tokens(&out.body) * 2 < total {
        progress
            .note(
                ctx,
                "Warning: the combined page is much shorter than its parts, so details may have been left out.",
            )
            .await;
    }
    Ok(out)
}

/// The pieces of `text` that best match `query` by meaning, in source order (FR-KN-UP-4a). When
/// the search model isn't available, by shared words.
pub(crate) async fn pick_excerpts(ctx: &Ctx<'_>, text: &str, query: &str, budget: usize) -> String {
    let pieces = sources::pieces(text);
    let scores = match score_by_meaning(ctx, &pieces, query).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!(kind = %e.kind, "excerpts by meaning unavailable, using words");
            score_by_words(&pieces, query)
        }
    };
    sources::excerpts(&pieces, &scores, budget)
}

pub(crate) async fn score_by_meaning(
    ctx: &Ctx<'_>,
    pieces: &[String],
    query: &str,
) -> Result<Vec<f32>, crate::sidecar::SidecarError> {
    let q = ctx.sidecars.embed_query(query).await?;
    let mut scores = Vec::with_capacity(pieces.len());
    // Small batches, so a search never waits long behind this (PROTOCOL.md "Concurrency").
    for batch in pieces.chunks(8) {
        let docs = ctx
            .sidecars
            .embed_documents(batch.iter().map(|p| (String::new(), p.clone())).collect())
            .await?;
        scores.extend(
            docs.iter()
                .map(|d| q.iter().zip(d).map(|(a, b)| a * b).sum::<f32>()),
        );
    }
    Ok(scores)
}

pub(crate) fn score_by_words(pieces: &[String], query: &str) -> Vec<f32> {
    let lower = query.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 1)
        .collect();
    pieces
        .iter()
        .map(|p| {
            let p = p.to_lowercase();
            words.iter().filter(|w| p.contains(**w)).count() as f32
        })
        .collect()
}

// ---------------------------------------------------------------- the check

/// Step 5 (and after a revision that is a page): group, tags, topics and glossary terms from the
/// `check` call on the default model (FR-KN-UP-5, FR-KN-GL-7). Without a usable default model, or
/// when the call fails, the card simply has no suggestions.
pub(crate) async fn suggest(
    ctx: &Ctx<'_>,
    progress: &mut Progress,
    card: &mut DraftCard,
    shown: &[String],
) {
    let Some(app) = ctx.app else { return };
    let Ok(resolved) = llm::resolve(ctx.state, ModelRole::Default, None).await else {
        return;
    };
    progress
        .begin(
            ctx,
            format!(
                "Checking group, tags and terms with {}",
                resolved.model.label
            ),
        )
        .await;
    let input = CheckInput {
        category_id: card.category_id.clone(),
        title: card.title.clone(),
        body: card.body.clone(),
        shown_terms: Some(shown.to_vec()),
        job_id: None,
    };
    match run_check(app.clone(), input).await {
        Ok(report) => {
            match report.group {
                Some(GroupPick::Existing { group_id, .. }) => {
                    card.group_id = Some(group_id);
                    card.group_name = None;
                }
                Some(GroupPick::New { name }) => {
                    card.group_id = None;
                    card.group_name = Some(name);
                }
                None => {}
            }
            card.tags = report.tags;
            card.topics = if report.one_topic {
                Vec::new()
            } else {
                report.topics
            };
            card.shown_terms = shown.to_vec();
            for t in &report.terms {
                if !card
                    .shown_terms
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(&t.term))
                {
                    card.shown_terms.push(t.term.clone());
                }
            }
            card.terms = report.terms;
        }
        Err(e) => {
            tracing::debug!(kind = %e.kind, "the check call failed; the card has no suggestions")
        }
    }
}

// ---------------------------------------------------------------- revising

async fn revise_job(
    ctx: &Ctx<'_>,
    job: &ReviseJob,
    progress: &mut Progress,
) -> Result<Done, JobError> {
    let (conversation_id, assistant_id) = (job.conversation_id.clone(), job.assistant_id.clone());
    let request = job.request.clone();
    let loaded = ctx
        .state
        .with_db(move |db| {
            let all = conversations::messages(db.conn(), &conversation_id)?;
            let Some((card_message, card)) = active_card(&all) else {
                return Ok(None);
            };
            let file = match &card.chat_file_id {
                Some(id) => conversations::chat_file(db.conn(), id).ok(),
                None => None,
            };
            let instruction = match &file {
                Some(f) => conversations::get_message(db.conn(), &f.message_id)
                    .map(|m| m.text)
                    .unwrap_or_default(),
                None => String::new(),
            };
            // The chat since the card, without this request and its answer.
            let at = all
                .iter()
                .position(|m| m.id == card_message.id)
                .unwrap_or(0);
            let earlier: Vec<LlmMessage> = all[at + 1..]
                .iter()
                .filter(|m| m.id != assistant_id && m.draft.is_none() && !m.text.trim().is_empty())
                .map(|m| {
                    let role = if m.role == ChatRole::User {
                        Role::User
                    } else {
                        Role::Assistant
                    };
                    LlmMessage::new(role, references::for_model(db.conn(), &m.text))
                })
                .collect();
            // The request itself is the last user message.
            let earlier = match earlier.split_last() {
                Some((last, rest)) if last.role == Role::User => rest.to_vec(),
                _ => earlier,
            };
            Ok(Some((
                card_message.id.clone(),
                card.clone(),
                file,
                references::for_model(db.conn(), &instruction),
                earlier,
                references::for_model(db.conn(), &request),
                references::source_text(db.conn(), &request)?,
            )))
        })
        .await
        .map_err(plain)?;
    let Some((card_message_id, old, file, instruction, earlier, request, referenced)) = loaded else {
        return Err(plain(invalid("There is no draft to revise any more.")));
    };

    let resolved = llm::resolve(ctx.state, ModelRole::Conversation, job.model_id.clone())
        .await
        .map_err(plain)?;
    progress
        .begin(
            ctx,
            format!("Revising the draft with {}", resolved.model.label),
        )
        .await;
    // The source text (FR-KN-UP-6); a card without one (or after it was released) is revised
    // from its own text.
    let mut source_text = file
        .and_then(|f| f.text)
        .unwrap_or_else(|| old.body.clone());
    // FR-LLM-AS-14: references in a revision are sources too, including pages outside the scope.
    if !referenced.is_empty() {
        source_text.push_str("\n\n");
        source_text.push_str(&referenced);
    }
    let file_name = old.file_name.clone().unwrap_or_else(|| old.title.clone());
    let size = llm::context_size(ctx.sidecars, &resolved).await;
    let mut fit = Fit::new(&source_text, sources::budget(size), true);
    let reply = loop {
        let (plan, allowed) = fit.plan(&source_text);
        let (source, excerpts) = match plan {
            SourcePlan::Whole | SourcePlan::Parts(_) => (source_text.clone(), false),
            SourcePlan::Excerpts => (
                pick_excerpts(ctx, &source_text, &request, allowed).await,
                true,
            ),
        };
        let glossary = ctx
            .glossary(&old.category_id, vec![source.clone(), request.clone()])
            .await;
        let messages = prompts::revise(
            &file_name,
            &source,
            &instruction,
            excerpts,
            &glossary,
            &old.body,
            &earlier,
            &request,
        )
        .map_err(|e| plain(e.into()))?;
        match ctx
            .complete(
                &resolved.target,
                &messages,
                &page_options(Some(&job.assistant_id), job.effort.clone()),
            )
            .await
        {
            Err(e) if is_too_long(&e) && fit.shrink(allowed) => continue,
            Err(e) => return Err(plain(e)),
            Ok(r) => break r,
        }
    };

    if !replies::is_page(&reply.text) {
        // A plain answer: the draft stays as it was.
        return Ok(Done {
            text: replies::strip_fence(&reply.text).trim().to_string(),
            card: None,
        });
    }
    let page = from_reply(&reply, &old.title);
    let mut card = DraftCard {
        title: page.title,
        body: page.body,
        incomplete: page.incomplete,
        state: DraftState::Draft,
        terms: Vec::new(),
        as_is: false,
        saved_page_id: None,
        superseded: false,
        discarded: false,
        ..old.clone()
    };
    suggest(ctx, progress, &mut card, &old.shown_terms).await;
    // The old card is now an earlier version.
    ctx.patch(
        &card_message_id,
        MessagePatch {
            draft: Some(DraftCard {
                superseded: true,
                ..old
            }),
            ..MessagePatch::default()
        },
    )
    .await;
    Ok(Done {
        text: "Here is the revised draft.".into(),
        card: Some(card),
    })
}

#[cfg(test)]
mod tests;
