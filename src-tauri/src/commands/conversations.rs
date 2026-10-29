//! Saved conversations (FR-LLM-AS-5, built in M5; UI-AS-1). Every change emits
//! `DataChanged { conversations }`.
//!
//! Thin wrappers: the rules live in `cairn_core::conversations`. File drafts and revisions are
//! written by the jobs in `commands::files`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cairn_core::conversations::{
    self, ChatMessage, Conversation, ConversationPatch, MessagePatch, NewConversation, NewMessage,
};
use tauri::{AppHandle, State};

use crate::error::ApiResult;
use crate::events::{Entity, emit_changed};
use crate::llm::Jobs;
use crate::state::AppState;

const CHANGED: &[Entity] = &[Entity::Conversations];

/// The first list of a run also settles the jobs the last run left unfinished.
static INTERRUPTED_SETTLED: AtomicBool = AtomicBool::new(false);

#[tauri::command]
#[specta::specta]
pub async fn list_conversations(
    state: State<'_, AppState>,
    jobs: State<'_, Arc<Jobs>>,
) -> ApiResult<Vec<Conversation>> {
    if !INTERRUPTED_SETTLED.swap(true, Ordering::SeqCst) {
        let jobs = Arc::clone(&jobs);
        let settled = state
            .with_db(move |db| conversations::fail_interrupted(db, |id| jobs.is_running(id)))
            .await;
        if let Err(e) = settled {
            tracing::warn!(error = %e.message, "could not settle interrupted chat jobs");
        }
    }
    state.with_db(|db| conversations::list(db.conn())).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    input: NewConversation,
) -> ApiResult<Conversation> {
    let c = state
        .with_db(move |db| conversations::create(db, input))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(c)
}

#[tauri::command]
#[specta::specta]
pub async fn update_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: ConversationPatch,
) -> ApiResult<Conversation> {
    let c = state
        .with_db(move |db| conversations::update(db, &id, patch))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(c)
}

/// FR-LLM-AS-13: archives the conversation; it is read-only and deleted for good after
/// `conversations::ARCHIVE_DAYS`.
#[tauri::command]
#[specta::specta]
pub async fn archive_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<Conversation> {
    let c = state
        .with_db(move |db| conversations::archive(db, &id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(c)
}

/// Takes the conversation out of the archive.
#[tauri::command]
#[specta::specta]
pub async fn restore_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> ApiResult<Conversation> {
    let c = state
        .with_db(move |db| conversations::restore(db, &id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(c)
}

/// FR-LLM-AS-13: deletes the conversations whose archive time is up. Blocking (the backup thread
/// calls it every minute); cheap when nothing has expired.
pub fn delete_expired(app: &AppHandle) {
    use tauri::Manager;
    let Some(db) = app.state::<AppState>().db_handle() else {
        return;
    };
    let gone = {
        let mut db = db.lock().unwrap_or_else(|p| p.into_inner());
        conversations::delete_expired_now(&mut db)
    };
    match gone {
        Ok(ids) if ids.is_empty() => {}
        Ok(ids) => {
            tracing::info!(count = ids.len(), "deleted archived conversations whose time was up");
            emit_changed(app, CHANGED);
        }
        Err(e) => tracing::warn!(error = %e, "could not delete expired archived conversations"),
    }
}

/// Stops the conversation's running jobs, then deletes it with its messages and file texts.
#[tauri::command]
#[specta::specta]
pub async fn delete_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    jobs: State<'_, Arc<Jobs>>,
    id: String,
) -> ApiResult<()> {
    // A chat job runs under its assistant message's id.
    let conversation_id = id.clone();
    let message_ids: Vec<String> = state
        .with_db(move |db| {
            Ok(conversations::messages(db.conn(), &conversation_id)?
                .into_iter()
                .map(|m| m.id)
                .collect())
        })
        .await?;
    for message_id in &message_ids {
        jobs.cancel(message_id);
    }
    state
        .with_db(move |db| conversations::delete(db, &id))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn list_messages(
    state: State<'_, AppState>,
    conversation_id: String,
) -> ApiResult<Vec<ChatMessage>> {
    state
        .with_db(move |db| conversations::messages(db.conn(), &conversation_id))
        .await
}

/// A message written by the UI: the user's question and the prototype's answers until M6.
#[tauri::command]
#[specta::specta]
pub async fn add_message(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: String,
    input: NewMessage,
) -> ApiResult<ChatMessage> {
    let m = state
        .with_db(move |db| conversations::add_message(db, &conversation_id, input))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(m)
}

/// Card changes from the UI (*Review* → `in_review`, saved, discarded). Releasing the file
/// text is done by `conversations::update_message` (FR-KN-UP-6).
#[tauri::command]
#[specta::specta]
pub async fn update_message(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: MessagePatch,
) -> ApiResult<ChatMessage> {
    let m = state
        .with_db(move |db| conversations::update_message(db, &id, patch))
        .await?;
    emit_changed(&app, CHANGED);
    Ok(m)
}
