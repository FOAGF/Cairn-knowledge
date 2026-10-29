//! Saved conversations (FR-LLM-AS-5, built in M5) and the text of files uploaded into them
//! (FR-KN-UP-6, FR-KN-UP-7). Tables: `conversations`, `messages`, `chat_files` (migration 0006).
//!
//! A message's optional parts (attachment, steps, draft card, error, model label, `extra`) are
//! stored as JSON in `messages.payload`.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::assistant::parts::{AnswerPart, FindPart, Proposal};
use crate::util::{new_id, now_iso};
use crate::{CoreError, Db, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: String,
    pub title: String,
    /// Empty = search everything (FR-LLM-AS-4).
    pub scope_category_ids: Vec<String>,
    /// FR-LLM-AS-10: tokens of earlier conversation sent with a question, after *Widen*; `None` =
    /// the setting.
    pub history_budget: Option<u32>,
    /// FR-LLM-ST-2 (D-67): the model picked in this conversation; `None` = the default model. The
    /// model may have been deleted since.
    pub model_id: Option<String>,
    /// The effort picked with it; `None` = not set.
    pub effort: Option<String>,
    /// FR-LLM-AS-13: when it was archived; `None` = not archived. An archived conversation is
    /// read-only and is deleted for good at `expires_at`.
    pub archived_at: Option<String>,
    /// `archived_at` plus [`ARCHIVE_DAYS`].
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// FR-LLM-AS-13 (D-68): days an archived conversation is kept before it is deleted for good.
pub const ARCHIVE_DAYS: i64 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewConversation {
    pub title: String,
    #[specta(optional)]
    pub scope_category_ids: Option<Vec<String>>,
    #[specta(optional)]
    pub model_id: Option<String>,
    #[specta(optional)]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPatch {
    #[specta(optional)]
    pub title: Option<String>,
    #[specta(optional)]
    pub scope_category_ids: Option<Vec<String>>,
    /// `0` goes back to the setting.
    #[specta(optional)]
    pub history_budget: Option<u32>,
    /// `""` goes back to the default model.
    #[specta(optional)]
    pub model_id: Option<String>,
    /// `""` = not set.
    #[specta(optional)]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
}

/// The type of an uploaded file, from its extension (FR-KN-UP-1; PROTOCOL.md `file.extract`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Text,
    Pdf,
    Docx,
    Image,
}

/// The file on a user message (UI-AS-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    pub name: String,
    /// Bytes.
    pub size: u32,
    pub kind: FileKind,
    /// The `chat_files` row with the extracted text; `None` until extraction finished.
    pub chat_file_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Active,
    Done,
    Failed,
    Skipped,
}

/// One processing step of an assistant message, e.g. "Reading page 3 of 12 with GPT-4.1"
/// (FR-KN-UP-7, FR-KN-UP-4b).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatStep {
    pub label: String,
    pub state: StepState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
    Unknown,
}

/// A possible glossary entry (FR-KN-GL-7), after the local filter (`checks::filter_terms`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TermSuggestion {
    pub term: String,
    /// `""` for `unknown`: the user writes the meaning.
    pub meaning: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DraftState {
    /// Can be refined by chatting.
    #[default]
    Draft,
    /// Sent to the review screen (the save pipeline).
    InReview,
    Saved,
}

/// A draft page card (FR-KN-UP-5, FR-KN-UP-7). `body` starts with the `# title` line.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DraftCard {
    pub title: String,
    pub body: String,
    pub category_id: String,
    /// An existing group suggested by the check (FR-KN-GR-2).
    pub group_id: Option<String>,
    /// A new group name suggested when no existing group fits.
    pub group_name: Option<String>,
    pub tags: Vec<String>,
    pub source_info: Option<String>,
    pub file_name: Option<String>,
    /// The source text this draft was made from (`chat_files`).
    pub chat_file_id: Option<String>,
    /// Detected topics when the draft covers several (FR-KN-DOC-2); empty otherwise.
    pub topics: Vec<String>,
    /// Possible glossary terms (FR-KN-GL-7) shown under the card.
    pub terms: Vec<TermSuggestion>,
    /// Every term ever shown for this draft, so a revision doesn't show one again.
    pub shown_terms: Vec<String>,
    /// The model stopped at its token limit (`finish_reason: length`): the draft may be cut off.
    pub incomplete: bool,
    /// Made without the LLM (*Use the extracted text as is*, FR-KN-UP-10, or `.md`/`.txt`
    /// imported as they are, FR-KN-UP-8).
    pub as_is: bool,
    pub state: DraftState,
    pub saved_page_id: Option<String>,
    /// A newer card replaced this one ("Earlier version").
    pub superseded: bool,
    pub discarded: bool,
}

/// A failure shown on an assistant message, with what the user can do next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageError {
    /// The `ApiError` kind, e.g. `ai_network`, `llm_unavailable`, `cancelled`.
    pub kind: String,
    pub message: String,
    /// The extracted text is there, so *Use the extracted text as is* can be offered (FR-KN-UP-10).
    pub can_use_as_is: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: ChatRole,
    pub text: String,
    pub model_id: Option<String>,
    /// The model's label when the message was written (the model may be deleted later).
    pub model_label: Option<String>,
    /// The effort level picked in the chat for this message (`None` = none picked).
    pub effort: Option<String>,
    /// D-65: the effort level the message's calls sent: the picked one, else the model's default
    /// level; `""` when none was sent (or no call was made). `None` while the job runs, and for
    /// messages from before 0.10.1.
    pub sent_effort: Option<String>,
    /// D-65: tokens the model reported spending on reasoning, over all the message's calls;
    /// `None` when the provider reported none.
    pub reasoning_tokens: Option<u32>,
    pub created_at: String,
    pub attachment: Option<ChatAttachment>,
    pub steps: Vec<ChatStep>,
    pub draft: Option<DraftCard>,
    pub error: Option<MessageError>,
    /// M6: an answer's sources, citations and labels (FR-LLM-AS-1).
    pub answer: Option<AnswerPart>,
    /// M6: a find list (FR-LLM-AS-2).
    pub find: Option<FindPart>,
    /// M6: a proposal card (FR-LLM-AS-3).
    pub proposal: Option<Proposal>,
    /// JSON text a job keeps for *Retry* (e.g. `{"categoryId": …}`); the UI doesn't read it.
    pub extra: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewMessage {
    pub role: ChatRole,
    pub text: String,
    #[specta(optional)]
    pub model_id: Option<String>,
    #[specta(optional)]
    pub model_label: Option<String>,
    #[specta(optional)]
    pub effort: Option<String>,
    #[specta(optional)]
    pub attachment: Option<ChatAttachment>,
    #[specta(optional)]
    pub steps: Option<Vec<ChatStep>>,
    #[specta(optional)]
    pub draft: Option<DraftCard>,
    #[specta(optional)]
    pub answer: Option<AnswerPart>,
    #[specta(optional)]
    pub find: Option<FindPart>,
    #[specta(optional)]
    pub proposal: Option<Proposal>,
    #[specta(optional)]
    pub extra: Option<String>,
}

/// Only the given fields change. `error: Some(None)` clears the error.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MessagePatch {
    #[specta(optional)]
    pub text: Option<String>,
    #[specta(optional)]
    pub attachment: Option<ChatAttachment>,
    #[specta(optional)]
    pub steps: Option<Vec<ChatStep>>,
    #[specta(optional)]
    pub draft: Option<DraftCard>,
    #[specta(optional)]
    pub error: Option<Option<MessageError>>,
    #[specta(optional)]
    pub answer: Option<AnswerPart>,
    #[specta(optional)]
    pub find: Option<FindPart>,
    #[specta(optional)]
    pub proposal: Option<Proposal>,
    #[specta(optional)]
    pub extra: Option<String>,
    #[specta(optional)]
    pub sent_effort: Option<String>,
    #[specta(optional)]
    pub reasoning_tokens: Option<u32>,
}

/// A row of `chat_files`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatFile {
    pub id: String,
    pub conversation_id: String,
    pub message_id: String,
    pub file_name: String,
    pub kind: FileKind,
    pub size: u32,
    pub pages: u32,
    /// `None` once released (FR-KN-UP-6).
    pub text: Option<String>,
    pub created_at: String,
}

/// What `messages.payload` holds: the parts only some messages have.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attachment: Option<ChatAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    steps: Vec<ChatStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    draft: Option<DraftCard>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<MessageError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    answer: Option<AnswerPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    find: Option<FindPart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposal: Option<Proposal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    extra: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sent_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u32>,
}

const CONVERSATION_COLS: &str =
    "id, title, scope, created_at, updated_at, history_budget, model_id, effort, archived_at";
const MESSAGE_COLS: &str = "id, conversation_id, role, text, model_id, effort, payload, created_at";

/// When a conversation archived at `archived_at` is deleted.
fn expiry(archived_at: &str) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(archived_at).ok()?;
    let end = at.with_timezone(&chrono::Utc) + chrono::Duration::days(ARCHIVE_DAYS);
    Some(end.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn conversation_from_row(r: &Row) -> rusqlite::Result<Conversation> {
    let scope: String = r.get(2)?;
    let archived_at: Option<String> = r.get(8)?;
    Ok(Conversation {
        id: r.get(0)?,
        title: r.get(1)?,
        scope_category_ids: serde_json::from_str(&scope).unwrap_or_default(),
        history_budget: r.get(5)?,
        model_id: r.get(6)?,
        effort: r.get(7)?,
        expires_at: archived_at.as_deref().and_then(expiry),
        archived_at,
        created_at: r.get(3)?,
        updated_at: r.get(4)?,
    })
}

fn message_from_row(r: &Row) -> rusqlite::Result<ChatMessage> {
    let role: String = r.get(2)?;
    let payload: String = r.get(6)?;
    // A payload that can't be read leaves the message plain rather than hiding the conversation.
    let p: Payload = serde_json::from_str(&payload).unwrap_or_default();
    Ok(ChatMessage {
        id: r.get(0)?,
        conversation_id: r.get(1)?,
        role: if role == "user" {
            ChatRole::User
        } else {
            ChatRole::Assistant
        },
        text: r.get(3)?,
        model_id: r.get(4)?,
        model_label: p.model_label,
        effort: r.get(5)?,
        sent_effort: p.sent_effort,
        reasoning_tokens: p.reasoning_tokens,
        created_at: r.get(7)?,
        attachment: p.attachment,
        steps: p.steps,
        draft: p.draft,
        error: p.error,
        answer: p.answer,
        find: p.find,
        proposal: p.proposal,
        extra: p.extra,
    })
}

fn payload_of(m: &ChatMessage) -> Payload {
    Payload {
        model_label: m.model_label.clone(),
        attachment: m.attachment.clone(),
        steps: m.steps.clone(),
        draft: m.draft.clone(),
        error: m.error.clone(),
        answer: m.answer.clone(),
        find: m.find.clone(),
        proposal: m.proposal.clone(),
        extra: m.extra.clone(),
        sent_effort: m.sent_effort.clone(),
        reasoning_tokens: m.reasoning_tokens,
    }
}

/// Newest activity first.
pub fn list(conn: &Connection) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CONVERSATION_COLS} FROM conversations ORDER BY updated_at DESC, id"
    ))?;
    let rows = stmt
        .query_map([], conversation_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Conversation> {
    conn.query_row(
        &format!("SELECT {CONVERSATION_COLS} FROM conversations WHERE id = ?1"),
        [id],
        conversation_from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "conversation",
        id: id.to_string(),
    })
}

/// The title is trimmed; an empty title becomes "New conversation".
pub fn create(db: &mut Db, input: NewConversation) -> Result<Conversation> {
    let title = input.title.trim();
    let title = if title.is_empty() {
        "New conversation"
    } else {
        title
    };
    let id = new_id();
    let now = now_iso();
    let scope = serde_json::to_string(&input.scope_category_ids.unwrap_or_default())?;
    db.conn().execute(
        "INSERT INTO conversations (id, title, scope, created_at, updated_at, model_id, effort)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6)",
        params![id, title, scope, now, non_empty(input.model_id), non_empty(input.effort)],
    )?;
    get(db.conn(), &id)
}

pub fn update(db: &mut Db, id: &str, patch: ConversationPatch) -> Result<Conversation> {
    let current = get(db.conn(), id)?;
    let title = match patch.title.as_deref().map(str::trim) {
        Some("") => "New conversation".to_string(),
        Some(t) => t.to_string(),
        None => current.title,
    };
    let scope =
        serde_json::to_string(&patch.scope_category_ids.unwrap_or(current.scope_category_ids))?;
    let history_budget = match patch.history_budget {
        Some(0) => None,
        Some(n) => Some(n),
        None => current.history_budget,
    };
    let model_id = patch.model_id.map_or(current.model_id, |m| non_empty(Some(m)));
    let effort = patch.effort.map_or(current.effort, |e| non_empty(Some(e)));
    db.conn().execute(
        "UPDATE conversations SET title = ?2, scope = ?3, history_budget = ?4, model_id = ?5, effort = ?6
         WHERE id = ?1",
        params![id, title, scope, history_budget, model_id, effort],
    )?;
    get(db.conn(), id)
}

/// `None` for a missing or blank value.
fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Deletes the conversation with its messages and file texts (cascade).
/// FR-LLM-AS-13: only an archived conversation can be deleted.
pub fn delete(db: &mut Db, id: &str) -> Result<()> {
    if get(db.conn(), id)?.archived_at.is_none() {
        return Err(CoreError::Invalid(
            "Only an archived conversation can be deleted. Archive it first.".into(),
        ));
    }
    db.conn()
        .execute("DELETE FROM conversations WHERE id = ?1", [id])?;
    Ok(())
}

/// FR-LLM-AS-13: archives the conversation (read-only from now on). Its place in the list and its
/// `updated_at` stay. Archiving an archived conversation changes nothing.
pub fn archive(db: &mut Db, id: &str) -> Result<Conversation> {
    let c = get(db.conn(), id)?;
    if c.archived_at.is_none() {
        db.conn().execute(
            "UPDATE conversations SET archived_at = ?2 WHERE id = ?1",
            params![id, now_iso()],
        )?;
    }
    get(db.conn(), id)
}

/// Takes the conversation out of the archive, so it can be continued.
pub fn restore(db: &mut Db, id: &str) -> Result<Conversation> {
    get(db.conn(), id)?;
    db.conn()
        .execute("UPDATE conversations SET archived_at = NULL WHERE id = ?1", [id])?;
    get(db.conn(), id)
}

/// [`delete_expired`] at the current time.
pub fn delete_expired_now(db: &mut Db) -> Result<Vec<String>> {
    delete_expired(db, chrono::Utc::now())
}

/// Deletes the conversations archived [`ARCHIVE_DAYS`] or more before `now`, with their messages
/// and file texts. Returns their ids.
pub fn delete_expired(db: &mut Db, now: chrono::DateTime<chrono::Utc>) -> Result<Vec<String>> {
    let expired: Vec<String> = list(db.conn())?
        .into_iter()
        .filter(|c| {
            c.expires_at
                .as_deref()
                .and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok())
                .is_some_and(|e| e <= now)
        })
        .map(|c| c.id)
        .collect();
    db.tx(|tx| {
        for id in &expired {
            tx.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
        }
        Ok(())
    })?;
    Ok(expired)
}

/// The messages of a conversation, oldest first.
pub fn messages(conn: &Connection, conversation_id: &str) -> Result<Vec<ChatMessage>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MESSAGE_COLS} FROM messages WHERE conversation_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt
        .query_map([conversation_id], message_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_message(conn: &Connection, id: &str) -> Result<ChatMessage> {
    conn.query_row(
        &format!("SELECT {MESSAGE_COLS} FROM messages WHERE id = ?1"),
        [id],
        message_from_row,
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "message",
        id: id.to_string(),
    })
}

/// Appends a message (next `seq`) and moves the conversation's `updated_at`.
pub fn add_message(db: &mut Db, conversation_id: &str, input: NewMessage) -> Result<ChatMessage> {
    if get(db.conn(), conversation_id)?.archived_at.is_some() {
        return Err(CoreError::Invalid(
            "This conversation is archived. Restore it to continue.".into(),
        ));
    }
    let id = new_id();
    let now = now_iso();
    let payload = serde_json::to_string(&Payload {
        model_label: input.model_label,
        attachment: input.attachment,
        steps: input.steps.unwrap_or_default(),
        draft: input.draft,
        error: None,
        answer: input.answer,
        find: input.find,
        proposal: input.proposal,
        extra: input.extra,
        ..Payload::default()
    })?;
    let role = match input.role {
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
    };
    db.tx(|tx| {
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?1",
            [conversation_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO messages (id, conversation_id, seq, role, text, model_id, effort, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, conversation_id, seq, role, input.text, input.model_id, input.effort, payload, now],
        )?;
        tx.execute(
            "UPDATE conversations SET updated_at = ?2 WHERE id = ?1",
            params![conversation_id, now],
        )?;
        Ok(())
    })?;
    get_message(db.conn(), &id)
}

/// A card that still counts as the working draft: not saved, discarded or replaced.
fn card_is_active(d: &DraftCard) -> bool {
    d.state != DraftState::Saved && !d.discarded && !d.superseded
}

/// Applies the patch. When a draft card becomes `saved` or `discarded` and no other active card
/// uses the same `chat_file_id`, that file's text is released (FR-KN-UP-6).
pub fn update_message(db: &mut Db, id: &str, patch: MessagePatch) -> Result<ChatMessage> {
    let mut m = get_message(db.conn(), id)?;
    if let Some(text) = patch.text {
        m.text = text;
    }
    if let Some(a) = patch.attachment {
        m.attachment = Some(a);
    }
    if let Some(steps) = patch.steps {
        m.steps = steps;
    }
    if let Some(d) = patch.draft {
        m.draft = Some(d);
    }
    if let Some(e) = patch.error {
        m.error = e;
    }
    if let Some(a) = patch.answer {
        m.answer = Some(a);
    }
    if let Some(f) = patch.find {
        m.find = Some(f);
    }
    if let Some(p) = patch.proposal {
        m.proposal = Some(p);
    }
    if let Some(x) = patch.extra {
        m.extra = Some(x);
    }
    if let Some(e) = patch.sent_effort {
        m.sent_effort = Some(e);
    }
    if let Some(n) = patch.reasoning_tokens {
        m.reasoning_tokens = Some(n);
    }
    let payload = serde_json::to_string(&payload_of(&m))?;
    db.conn().execute(
        "UPDATE messages SET text = ?2, payload = ?3 WHERE id = ?1",
        params![id, m.text, payload],
    )?;
    if let Some(card) = &m.draft
        && (card.state == DraftState::Saved || card.discarded)
        && let Some(file_id) = &card.chat_file_id
    {
        let in_use = messages(db.conn(), &m.conversation_id)?
            .iter()
            .filter(|o| o.id != m.id)
            .filter_map(|o| o.draft.as_ref())
            .any(|d| card_is_active(d) && d.chat_file_id.as_ref() == Some(file_id));
        if !in_use {
            release_chat_file(db, file_id)?;
        }
    }
    Ok(m)
}

/// Clears an assistant message for a new run (*Retry*, *Search all categories*): its text, steps,
/// error and every part. The message keeps its id, model, effort and `extra`.
pub fn reset_message(db: &mut Db, id: &str) -> Result<ChatMessage> {
    let mut m = get_message(db.conn(), id)?;
    m.text.clear();
    m.steps.clear();
    m.error = None;
    m.draft = None;
    m.answer = None;
    m.find = None;
    m.proposal = None;
    // The run again decides what it sends and reports (D-65).
    m.sent_effort = None;
    m.reasoning_tokens = None;
    let payload = serde_json::to_string(&payload_of(&m))?;
    db.conn().execute(
        "UPDATE messages SET text = '', payload = ?2 WHERE id = ?1",
        params![id, payload],
    )?;
    Ok(m)
}

/// Ends a job that didn't finish (*Stop*, or the app closed): its running steps fail and the
/// message gets `error`, with *Use the extracted text as is* offered when the file's text is
/// stored (FR-KN-UP-10).
pub fn fail_message(db: &mut Db, id: &str, kind: &str, message: &str) -> Result<ChatMessage> {
    let m = get_message(db.conn(), id)?;
    let can_use_as_is = messages(db.conn(), &m.conversation_id)?
        .iter()
        .take_while(|o| o.id != m.id)
        .last()
        .filter(|o| o.role == ChatRole::User)
        .and_then(|o| o.attachment.as_ref())
        .and_then(|a| a.chat_file_id.as_deref())
        .and_then(|f| chat_file(db.conn(), f).ok())
        .is_some_and(|f| f.text.is_some());
    let mut steps = m.steps;
    for s in &mut steps {
        if s.state == StepState::Active {
            s.state = StepState::Failed;
        }
    }
    update_message(
        db,
        id,
        MessagePatch {
            steps: Some(steps),
            error: Some(Some(MessageError {
                kind: kind.into(),
                message: message.into(),
                can_use_as_is,
            })),
            ..MessagePatch::default()
        },
    )
}

/// At startup: assistant messages whose job was running when the app closed (a running step, no
/// card, no error) fail with `interrupted`, so the UI offers *Retry* instead of waiting forever.
/// `running` says which message ids have a live job. Returns the ids changed.
pub fn fail_interrupted(db: &mut Db, running: impl Fn(&str) -> bool) -> Result<Vec<String>> {
    let ids: Vec<String> = db
        .conn()
        .prepare("SELECT id FROM messages WHERE role = 'assistant' AND payload LIKE '%\"state\":\"active\"%'")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut failed = Vec::new();
    for id in ids {
        if running(&id) {
            continue;
        }
        let m = get_message(db.conn(), &id)?;
        if m.draft.is_none() && m.error.is_none() {
            fail_message(
                db,
                &id,
                "interrupted",
                "Cairn was closed before this finished.",
            )?;
            failed.push(id);
        }
    }
    Ok(failed)
}

fn kind_name(k: FileKind) -> &'static str {
    match k {
        FileKind::Text => "text",
        FileKind::Pdf => "pdf",
        FileKind::Docx => "docx",
        FileKind::Image => "image",
    }
}

/// Stores the text read from an uploaded file; returns the `chat_files` id.
#[allow(clippy::too_many_arguments)]
pub fn add_chat_file(
    db: &mut Db,
    conversation_id: &str,
    message_id: &str,
    file_name: &str,
    kind: FileKind,
    size: u32,
    pages: u32,
    text: &str,
) -> Result<String> {
    let id = new_id();
    db.conn().execute(
        "INSERT INTO chat_files (id, conversation_id, message_id, file_name, kind, size, pages, text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, conversation_id, message_id, file_name, kind_name(kind), size, pages, text, now_iso()],
    )?;
    Ok(id)
}

pub fn chat_file(conn: &Connection, id: &str) -> Result<ChatFile> {
    conn.query_row(
        "SELECT id, conversation_id, message_id, file_name, kind, size, pages, text, created_at
         FROM chat_files WHERE id = ?1",
        [id],
        |r| {
            let kind: String = r.get(4)?;
            Ok(ChatFile {
                id: r.get(0)?,
                conversation_id: r.get(1)?,
                message_id: r.get(2)?,
                file_name: r.get(3)?,
                kind: match kind.as_str() {
                    "pdf" => FileKind::Pdf,
                    "docx" => FileKind::Docx,
                    "image" => FileKind::Image,
                    _ => FileKind::Text,
                },
                size: r.get(5)?,
                pages: r.get(6)?,
                text: r.get(7)?,
                created_at: r.get(8)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| CoreError::NotFound {
        entity: "file",
        id: id.to_string(),
    })
}

/// FR-KN-UP-6: forget the extracted text (the row stays, `text` becomes NULL).
pub fn release_chat_file(db: &mut Db, id: &str) -> Result<()> {
    db.conn()
        .execute("UPDATE chat_files SET text = NULL WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(dir: &std::path::Path) -> Db {
        Db::open_at(dir).unwrap()
    }

    fn user(text: &str) -> NewMessage {
        NewMessage {
            role: ChatRole::User,
            text: text.into(),
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
        }
    }

    fn card(file: Option<&str>) -> DraftCard {
        DraftCard {
            title: "T".into(),
            body: "# T\n\nbody".into(),
            category_id: "p".into(),
            chat_file_id: file.map(str::to_string),
            ..DraftCard::default()
        }
    }

    #[test]
    fn conversations_are_created_updated_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let a = create(
            &mut db,
            NewConversation {
                title: "  ".into(),
                scope_category_ids: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(a.title, "New conversation");
        assert!(a.scope_category_ids.is_empty());
        let b = create(
            &mut db,
            NewConversation {
                title: " Pumps ".into(),
                scope_category_ids: Some(vec!["x".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            (b.title.as_str(), b.scope_category_ids.clone()),
            ("Pumps", vec!["x".to_string()])
        );
        let b2 = update(
            &mut db,
            &b.id,
            ConversationPatch {
                title: Some("Hoses".into()),
                scope_category_ids: Some(vec![]),
                history_budget: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(b2.title, "Hoses");
        assert!(b2.scope_category_ids.is_empty());
        // New activity moves a conversation to the top.
        std::thread::sleep(std::time::Duration::from_millis(5));
        add_message(&mut db, &a.id, user("hi")).unwrap();
        assert_eq!(list(db.conn()).unwrap()[0].id, a.id);
        archive(&mut db, &a.id).unwrap();
        delete(&mut db, &a.id).unwrap();
        assert_eq!(list(db.conn()).unwrap().len(), 1);
        assert!(matches!(
            get(db.conn(), &a.id),
            Err(CoreError::NotFound { .. })
        ));
        assert!(matches!(
            delete(&mut db, &a.id),
            Err(CoreError::NotFound { .. })
        ));
        assert!(messages(db.conn(), &a.id).unwrap().is_empty());
    }

    /// FR-LLM-AS-13: archive, restore, delete only when archived, no new messages while archived,
    /// and deleted for good three days after archiving.
    #[test]
    fn archived_conversations_are_read_only_and_expire() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let a = create(&mut db, NewConversation::default()).unwrap();
        add_message(&mut db, &a.id, user("hi")).unwrap();
        assert!(matches!(delete(&mut db, &a.id), Err(CoreError::Invalid(_))));
        let a = get(db.conn(), &a.id).unwrap();

        let archived = archive(&mut db, &a.id).unwrap();
        let at = archived.archived_at.clone().unwrap();
        assert_eq!(archived.updated_at, a.updated_at, "archiving is not activity");
        let expires = chrono::DateTime::parse_from_rfc3339(archived.expires_at.as_deref().unwrap()).unwrap();
        let start = chrono::DateTime::parse_from_rfc3339(&at).unwrap();
        assert_eq!(expires - start, chrono::Duration::days(ARCHIVE_DAYS));
        assert!(matches!(add_message(&mut db, &a.id, user("more")), Err(CoreError::Invalid(_))));
        // Archiving again keeps the first time.
        assert_eq!(archive(&mut db, &a.id).unwrap().archived_at, Some(at));

        let restored = restore(&mut db, &a.id).unwrap();
        assert_eq!((restored.archived_at, restored.expires_at), (None, None));
        add_message(&mut db, &a.id, user("back")).unwrap();

        // Expiry: one second before nothing goes, at the time it does; others stay.
        let b = create(&mut db, NewConversation::default()).unwrap();
        let a = archive(&mut db, &a.id).unwrap();
        let end = chrono::DateTime::parse_from_rfc3339(a.expires_at.as_deref().unwrap())
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(delete_expired(&mut db, end - chrono::Duration::seconds(1)).unwrap().is_empty());
        assert_eq!(delete_expired(&mut db, end).unwrap(), vec![a.id.clone()]);
        assert!(messages(db.conn(), &a.id).unwrap().is_empty());
        assert_eq!(list(db.conn()).unwrap().into_iter().map(|c| c.id).collect::<Vec<_>>(), vec![b.id]);
    }

    /// D-67: the model and effort picked in a conversation stay with it until changed.
    #[test]
    fn a_conversation_keeps_its_model_and_effort() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let c = create(
            &mut db,
            NewConversation {
                title: "Q".into(),
                model_id: Some("m1".into()),
                effort: Some("high".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!((c.model_id.as_deref(), c.effort.as_deref()), (Some("m1"), Some("high")));
        let patch = |db: &mut Db, p: ConversationPatch| update(db, &c.id, p).unwrap();
        let c2 = patch(&mut db, ConversationPatch { model_id: Some("m2".into()), ..Default::default() });
        assert_eq!((c2.model_id.as_deref(), c2.effort.as_deref()), (Some("m2"), Some("high")));
        let c3 = patch(&mut db, ConversationPatch { title: Some("Renamed".into()), ..Default::default() });
        assert_eq!((c3.model_id.as_deref(), c3.effort.as_deref()), (Some("m2"), Some("high")));
        let c4 = patch(&mut db, ConversationPatch { model_id: Some("".into()), effort: Some(" ".into()), ..Default::default() });
        assert_eq!((c4.model_id, c4.effort), (None, None));
        // Changing the pick is not activity: the conversation keeps its place in the list.
        assert_eq!(c4.updated_at, c.updated_at);
    }

    /// Step 7 gives conversations from before it the model and effort of their last message with a model.
    #[test]
    fn upgrading_keeps_the_model_of_the_last_reply() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::DataPaths::new(dir.path());
        paths.ensure().unwrap();
        {
            let mut conn = rusqlite::Connection::open(paths.db_file()).unwrap();
            crate::migrations::migrations().to_version(&mut conn, 6).unwrap();
            conn.execute_batch(
                "INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('c1', 'A', 'x', 'x'), ('c2', 'B', 'x', 'x');
                 INSERT INTO messages (id, conversation_id, seq, role, text, model_id, effort, created_at) VALUES
                   ('a', 'c1', 1, 'user', 'q', NULL, NULL, 'x'),
                   ('b', 'c1', 2, 'assistant', 'r', 'm1', 'low', 'x'),
                   ('c', 'c1', 3, 'user', 'q2', NULL, NULL, 'x'),
                   ('d', 'c1', 4, 'assistant', 'r2', 'm2', 'high', 'x'),
                   ('e', 'c1', 5, 'user', 'q3', NULL, NULL, 'x');",
            )
            .unwrap();
        }
        let db = Db::open(paths).unwrap();
        let c1 = get(db.conn(), "c1").unwrap();
        assert_eq!((c1.model_id.as_deref(), c1.effort.as_deref()), (Some("m2"), Some("high")));
        let c2 = get(db.conn(), "c2").unwrap();
        assert_eq!((c2.model_id, c2.effort), (None, None));
    }

    #[test]
    fn messages_keep_their_order_and_optional_parts() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let c = create(&mut db, NewConversation::default()).unwrap();
        let m1 = add_message(
            &mut db,
            &c.id,
            NewMessage {
                attachment: Some(ChatAttachment {
                    name: "a.pdf".into(),
                    size: 10,
                    kind: FileKind::Pdf,
                    chat_file_id: None,
                }),
                ..user("with file")
            },
        )
        .unwrap();
        let m2 = add_message(
            &mut db,
            &c.id,
            NewMessage {
                role: ChatRole::Assistant,
                model_id: Some("m1".into()),
                model_label: Some("GPT 中文".into()),
                effort: Some("high".into()),
                steps: Some(vec![ChatStep {
                    label: "Reading page 1 of 2".into(),
                    state: StepState::Active,
                }]),
                answer: None,
                find: None,
                proposal: None,
                extra: Some("{\"citations\":[]}".into()),
                ..user("")
            },
        )
        .unwrap();
        let all = messages(db.conn(), &c.id).unwrap();
        assert_eq!(all, vec![m1.clone(), m2.clone()]);
        assert_eq!(all[0].attachment.as_ref().unwrap().name, "a.pdf");
        assert_eq!(all[1].model_label.as_deref(), Some("GPT 中文"));
        assert_eq!(all[1].steps.len(), 1);

        let patched = update_message(
            &mut db,
            &m2.id,
            MessagePatch {
                text: Some("done".into()),
                steps: Some(vec![]),
                draft: Some(card(None)),
                error: Some(Some(MessageError {
                    kind: "ai_network".into(),
                    message: "x".into(),
                    can_use_as_is: true,
                })),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        assert_eq!(patched, get_message(db.conn(), &m2.id).unwrap());
        assert!(patched.error.is_some() && patched.steps.is_empty());
        // `Some(None)` clears the error; omitted fields stay.
        let cleared = update_message(
            &mut db,
            &m2.id,
            MessagePatch {
                error: Some(None),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        assert!(cleared.error.is_none());
        assert_eq!(cleared.text, "done");
        assert_eq!(cleared.draft, Some(card(None)));
        assert!(matches!(
            add_message(&mut db, "nope", user("x")),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn a_saved_or_discarded_card_releases_the_file_text_unless_another_card_uses_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let c = create(&mut db, NewConversation::default()).unwrap();
        let u = add_message(&mut db, &c.id, user("file")).unwrap();
        let file = add_chat_file(
            &mut db,
            &c.id,
            &u.id,
            "a.docx",
            FileKind::Docx,
            99,
            1,
            "source text",
        )
        .unwrap();
        let text = |db: &Db| chat_file(db.conn(), &file).unwrap().text;
        let new_card = |db: &mut Db| {
            add_message(
                db,
                &c.id,
                NewMessage {
                    role: ChatRole::Assistant,
                    draft: Some(card(Some(&file))),
                    ..user("")
                },
            )
            .unwrap()
        };
        let first = new_card(&mut db);
        // A revision: the old card is superseded, the new one is active.
        update_message(
            &mut db,
            &first.id,
            MessagePatch {
                draft: Some(DraftCard {
                    superseded: true,
                    ..card(Some(&file))
                }),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        let second = new_card(&mut db);
        assert_eq!(text(&db).as_deref(), Some("source text"));
        // Another active card keeps the text.
        let third = new_card(&mut db);
        update_message(
            &mut db,
            &second.id,
            MessagePatch {
                draft: Some(DraftCard {
                    discarded: true,
                    ..card(Some(&file))
                }),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        assert_eq!(text(&db).as_deref(), Some("source text"));
        // Review does not release; saving the last active card does.
        update_message(
            &mut db,
            &third.id,
            MessagePatch {
                draft: Some(DraftCard {
                    state: DraftState::InReview,
                    ..card(Some(&file))
                }),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        assert_eq!(text(&db).as_deref(), Some("source text"));
        update_message(
            &mut db,
            &third.id,
            MessagePatch {
                draft: Some(DraftCard {
                    state: DraftState::Saved,
                    saved_page_id: Some("pg".into()),
                    ..card(Some(&file))
                }),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        assert_eq!(text(&db), None);
        // The row stays.
        assert_eq!(chat_file(db.conn(), &file).unwrap().file_name, "a.docx");
    }

    #[test]
    fn deleting_a_conversation_removes_its_file_texts() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let c = create(&mut db, NewConversation::default()).unwrap();
        let u = add_message(&mut db, &c.id, user("file")).unwrap();
        let f = add_chat_file(&mut db, &c.id, &u.id, "a.txt", FileKind::Text, 1, 1, "x").unwrap();
        archive(&mut db, &c.id).unwrap();
        delete(&mut db, &c.id).unwrap();
        assert!(matches!(
            chat_file(db.conn(), &f),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn interrupted_jobs_fail_with_retry_offered() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = open(dir.path());
        let c = create(&mut db, NewConversation::default()).unwrap();
        let u = add_message(
            &mut db,
            &c.id,
            NewMessage {
                attachment: Some(ChatAttachment {
                    name: "a.pdf".into(),
                    size: 1,
                    kind: FileKind::Pdf,
                    chat_file_id: None,
                }),
                ..user("")
            },
        )
        .unwrap();
        let file =
            add_chat_file(&mut db, &c.id, &u.id, "a.pdf", FileKind::Pdf, 1, 1, "text").unwrap();
        update_message(
            &mut db,
            &u.id,
            MessagePatch {
                attachment: Some(ChatAttachment {
                    name: "a.pdf".into(),
                    size: 1,
                    kind: FileKind::Pdf,
                    chat_file_id: Some(file),
                }),
                ..MessagePatch::default()
            },
        )
        .unwrap();
        let running_step = || {
            Some(vec![ChatStep {
                label: "Drafting".into(),
                state: StepState::Active,
            }])
        };
        let stale = add_message(
            &mut db,
            &c.id,
            NewMessage {
                role: ChatRole::Assistant,
                steps: running_step(),
                ..user("")
            },
        )
        .unwrap();
        let live = add_message(
            &mut db,
            &c.id,
            NewMessage {
                role: ChatRole::Assistant,
                steps: running_step(),
                ..user("")
            },
        )
        .unwrap();
        let done = add_message(
            &mut db,
            &c.id,
            NewMessage {
                role: ChatRole::Assistant,
                steps: Some(vec![ChatStep {
                    label: "Done".into(),
                    state: StepState::Done,
                }]),
                draft: Some(card(None)),
                ..user("")
            },
        )
        .unwrap();
        let failed = fail_interrupted(&mut db, |id| id == live.id).unwrap();
        assert_eq!(failed, vec![stale.id.clone()]);
        let m = get_message(db.conn(), &stale.id).unwrap();
        assert_eq!(m.steps[0].state, StepState::Failed);
        let e = m.error.unwrap();
        assert_eq!(e.kind, "interrupted");
        assert!(
            e.can_use_as_is,
            "the user message before it has the file's text"
        );
        assert!(get_message(db.conn(), &live.id).unwrap().error.is_none());
        assert!(get_message(db.conn(), &done.id).unwrap().error.is_none());
        // Nothing left to fail the second time.
        assert!(fail_interrupted(&mut db, |_| false).unwrap() == vec![live.id.clone()]);
    }

    /// FR-DATA-6: conversations, messages and chat_files are exported and imported unchanged.
    #[test]
    fn export_import_round_trip() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut source = open(a.path());
        let c = create(
            &mut source,
            NewConversation {
                title: "对话 😀".into(),
                scope_category_ids: Some(vec!["p1".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let u = add_message(&mut source, &c.id, user("file \"quoted\"")).unwrap();
        let file = add_chat_file(
            &mut source,
            &c.id,
            &u.id,
            "笔记.pdf",
            FileKind::Pdf,
            1234,
            3,
            "页 1\n页 2",
        )
        .unwrap();
        let released = add_chat_file(
            &mut source,
            &c.id,
            &u.id,
            "b.txt",
            FileKind::Text,
            5,
            1,
            "gone",
        )
        .unwrap();
        release_chat_file(&mut source, &released).unwrap();
        add_message(
            &mut source,
            &c.id,
            NewMessage {
                role: ChatRole::Assistant,
                model_id: Some("m".into()),
                model_label: Some("Model".into()),
                steps: Some(vec![ChatStep {
                    label: "Done".into(),
                    state: StepState::Done,
                }]),
                draft: Some(DraftCard {
                    terms: vec![TermSuggestion {
                        term: "MMU".into(),
                        meaning: "".into(),
                        confidence: Confidence::Unknown,
                    }],
                    shown_terms: vec!["MMU".into()],
                    ..card(Some(&file))
                }),
                answer: None,
                find: None,
                proposal: None,
                extra: Some("{\"a\":1}".into()),
                ..user("")
            },
        )
        .unwrap();

        let zip = a.path().join("out.zip");
        crate::export::export_to(&source, &zip).unwrap();
        let mut target = open(b.path());
        crate::export::import_from(&mut target, &zip).unwrap();

        assert_eq!(list(target.conn()).unwrap(), list(source.conn()).unwrap());
        assert_eq!(
            messages(target.conn(), &c.id).unwrap(),
            messages(source.conn(), &c.id).unwrap()
        );
        for id in [&file, &released] {
            assert_eq!(
                chat_file(target.conn(), id).unwrap(),
                chat_file(source.conn(), id).unwrap()
            );
        }
        assert_eq!(chat_file(target.conn(), &released).unwrap().text, None);
    }
}
