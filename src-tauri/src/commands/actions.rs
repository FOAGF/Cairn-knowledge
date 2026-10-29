//! Create and change requests in the assistant (FR-LLM-AS-3, D-44): the action branch of the
//! assistant job, and the proposal card's buttons.
//!
//! [`run`] (called by `assistant::ask_job` for the plan kinds `create` / `change`, with the
//! candidates, sources and history already gathered in an [`ActionInput`]):
//! 1. The candidates loaded (`items::load`) and labelled (`formats::candidates_block`).
//! 2. `ask-action` (conversation's model, JSON, one `check-retry`), which also reads the sources (D-83) → `actions::read`.
//! 3. A new page: `ask-page`, streamed into the message's draft card (the M5 card: *Review* →
//!    save checks), then `files::suggest` fills its group, tags and terms as for a file draft.
//!    Page changes: `edit-page` per page → `UpdatePage` rows with the old and new body (a plain
//!    reply instead of a page means the change can't be made: it becomes the proposal's reply).
//! 4. `create_task` terms through `checks::filter_terms` → `Proposal::terms`.
//! 5. The message gets its `Proposal` (every row ticked) and the model's reply as its text.
//!
//! Commands: tick / untick a row, *Confirm* (applies the ticked pending rows with
//! `actions::apply`, one transaction each, and records each row's result), *Reject*.
//! *Edit…* is UI only: it opens the normal task form (or the page) filled in from the row.
//!
//! Owned by the M6 actions agent.

use serde_json::{Map, Value};
use tauri::{AppHandle, State};

use cairn_core::assistant::actions::{self, PageEditRequest};
use cairn_core::assistant::formats::{self, ItemInfo, CategoryLine, SourcePiece};
use cairn_core::assistant::items::{self, ItemRef};
use cairn_core::assistant::parts::{Proposal, ProposalRow, ProposedChange, RowState};
use cairn_core::assistant::plan::{Plan, PlanCategory};
use cairn_core::assistant::retrieve;
use cairn_core::checks::{self, RawTerm};
use cairn_core::conversations::{
    ChatMessage, DraftCard, DraftState, MessagePatch, TermSuggestion, StepState,
};
use cairn_core::prompts::{self, AskAction, AskPage, LlmMessage, ModelRole, Role};
use cairn_core::{CoreError, Db, knowledge, categories, replies};

use super::assistant::AskJob;
use super::files::{Ctx, Done, JobError, Progress, plain, suggest};
use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::llm::{self, CallOptions};
use crate::state::AppState;

/// Seconds per attempt for the JSON call, and for a page.
const ACTION_TIMEOUT_S: u32 = 120;
const PAGE_TIMEOUT_S: u32 = 300;

/// What the answer job hands the action branch, so it doesn't search itself.
pub(crate) struct ActionInput {
    /// Items a change may target, in order: this turn's search results, the items shown with the
    /// previous answer, the plan's refs (de-duplicated). `formats::candidates_block` labels them.
    pub(crate) candidates: Vec<ItemRef>,
    /// This turn's sources, for `ask-action` (D-83), `ask-page` and `edit-page`.
    pub(crate) sources: Vec<SourcePiece>,
    /// The earlier turns within the history budget (`formats::history_messages`).
    pub(crate) history: Vec<LlmMessage>,
    /// Every category, with the conversation's scope (for `categories` / `scope` and category names).
    pub(crate) categories: Vec<PlanCategory>,
    /// `formats::today_line`.
    pub(crate) today: String,
    /// The glossary lines for the request and the candidates (FR-LLM-AS-8).
    pub(crate) glossary: String,
}

/// A call whose reply is a JSON object, on a [`Ctx`] (so the tests can run it without a window):
/// one `check-retry` when the first reply can't be read.
async fn json_call(
    ctx: &Ctx<'_>,
    target: &cairn_core::providers::CallTarget,
    messages: &[LlmMessage],
    opts: &CallOptions,
) -> ApiResult<Map<String, Value>> {
    let first = ctx.complete(target, messages, opts).await?;
    if let Some(v) = replies::json_object(&first.text) {
        return Ok(v);
    }
    tracing::debug!("JSON reply unreadable, retrying once");
    let mut again = messages.to_vec();
    again.push(LlmMessage::new(Role::Assistant, first.text));
    again.push(prompts::check_retry()?);
    let second = ctx.complete(target, &again, opts).await?;
    replies::json_object(&second.text).ok_or_else(|| ApiError {
        kind: "ai_unreadable".into(),
        message: "The model's answer couldn't be read, even after asking again.".into(),
    })
}

fn first_words(text: &str, n: usize) -> String {
    text.split_whitespace().take(n).collect::<Vec<_>>().join(" ")
}

/// The new page as a card body (README "Pages" › `ask-page`): markers and fences removed; line 1
/// is `# title`, else the proposal's title, else the first heading, else the request's first words.
fn page_of(text: &str, title: &str, request: &str) -> (String, String) {
    let cleaned = formats::strip_markers(text);
    let body = replies::strip_fence(&cleaned).trim().to_string();
    let fallback = if title.is_empty() { first_words(request, 6) } else { title.to_string() };
    let page = if !body.starts_with("# ") && !title.is_empty() {
        replies::page(&replies::with_title(&body, title), &fallback)
    } else {
        replies::page(&body, &fallback)
    };
    (page.title, page.body)
}

/// `edit-page`'s reply as the page body, with the existing title kept (a page change never
/// renames, README "Pages" › `edit-page`).
fn edited_body(text: &str, title: &str) -> String {
    let cleaned = formats::strip_markers(text);
    let body = replies::strip_fence(&cleaned).trim().to_string();
    let first = body.lines().next().unwrap_or("");
    if first.strip_prefix("# ").map(str::trim) == Some(title) {
        body
    } else {
        replies::with_title(&body, title)
    }
}

/// The page is the same text as the old one, compared by lines without trailing spaces
/// (a model's trailing newline is not a change).
fn same_page(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string();
    norm(a) == norm(b)
}

fn options(opts_stream: Option<&str>, timeout_s: u32, effort: &Option<String>) -> CallOptions {
    CallOptions {
        stream_id: opts_stream.map(str::to_string),
        timeout_s: Some(timeout_s),
        max_tokens: None,
        effort: effort.clone(),
        web_search: None,
    }
}

/// The action branch of an assistant message.
pub(crate) async fn run(
    ctx: &Ctx<'_>,
    job: &AskJob,
    plan: &Plan,
    input: ActionInput,
    progress: &mut Progress,
) -> Result<Done, JobError> {
    let _ = plan;
    let resolved = llm::resolve(ctx.state, ModelRole::Conversation, job.model_id.clone())
        .await
        .map_err(plain)?;

    // 1. The candidates.
    let (candidates, plan_categories) = (input.candidates.clone(), input.categories.clone());
    let (loaded, keys) = ctx
        .state
        .with_db(move |db| {
            let items = items::load(db.conn(), &candidates)?;
            let keys: Vec<(String, String, String)> = categories::list(db.conn(), true)?
                .into_iter()
                .map(|p| (p.id, p.name, p.key))
                .collect();
            Ok((items, keys))
        })
        .await
        .map_err(plain)?;
    let lines: Vec<CategoryLine> = plan_categories
        .iter()
        .filter_map(|p| {
            keys.iter().find(|k| k.0 == p.id).map(|k| CategoryLine {
                name: k.1.clone(),
                key: k.2.clone(),
                in_scope: p.in_scope,
            })
        })
        .collect();
    let category_name = |id: &str| {
        keys.iter().find(|k| k.0 == id).map(|k| k.1.clone()).unwrap_or_default()
    };
    let (block, map) = formats::candidates_block(&loaded);
    let labels: Vec<(String, ItemInfo)> = map.into_iter().map(|(l, i)| (l, loaded[i].clone())).collect();

    // 2. ask-action. It reads the sources too: a referenced item's content (D-83). A new page's
    // writer gets them without the note on what was left out (`without_left_out_notes`).
    let sources = formats::sources_block(&input.sources);
    let page_sources = formats::sources_block(&retrieve::without_left_out_notes(&input.sources));
    progress
        .begin(ctx, format!("Working out the changes with {}", resolved.model.label))
        .await;
    let messages = prompts::ask_action(
        &AskAction {
            today: &input.today,
            categories: &formats::categories_lines(&lines),
            scope: &formats::scope_sentence(&lines),
            candidates: &block,
            sources: &sources,
            glossary: &input.glossary,
            request: &job.text,
        },
        &input.history,
    )
    .map_err(|e| plain(e.into()))?;
    let reply = json_call(ctx, &resolved.target, &messages, &options(None, ACTION_TIMEOUT_S, &job.effort))
        .await
        .map_err(plain)?;
    let read = actions::read(&reply, &labels, &input.categories);
    // NFR-SEC-3: the notes quote the model's words; only their number is logged.
    if !read.notes.is_empty() {
        tracing::debug!(count = read.notes.len(), "ask-action reply repaired");
    }

    let mut extra: Vec<String> = Vec::new();

    // 3a. A new page, into the message's draft card.
    let mut card = None;
    if let Some(req) = &read.new_page {
        progress.begin(ctx, "Writing the page").await;
        let glossary = ctx
            .glossary(&req.category_id, vec![req.title.clone(), req.content.clone(), job.text.clone()])
            .await;
        let messages = prompts::ask_page(
            &AskPage {
                today: &input.today,
                category: &category_name(&req.category_id),
                title: &req.title,
                content: &req.content,
                sources: &page_sources,
                glossary: &glossary,
                request: &job.text,
            },
            &input.history,
        )
        .map_err(|e| plain(e.into()))?;
        let page = ctx
            .complete(
                &resolved.target,
                &messages,
                &options(Some(&job.assistant_id), PAGE_TIMEOUT_S, &job.effort),
            )
            .await
            .map_err(plain)?;
        let (title, body) = page_of(&page.text, &req.title, &job.text);
        let mut draft = DraftCard {
            title,
            body,
            category_id: req.category_id.clone(),
            incomplete: page.cut_off(),
            state: DraftState::Draft,
            ..DraftCard::default()
        };
        suggest(ctx, progress, &mut draft, &[]).await;
        card = Some(draft);
    }

    // 3b. Page changes, one `edit-page` each.
    let mut rows: Vec<ProposalRow> = read
        .changes
        .iter()
        .map(|c| ProposalRow {
            change: c.clone(),
            ticked: true,
            state: RowState::Pending,
            result: None,
            result_id: None,
        })
        .collect();
    for PageEditRequest { page_id, change } in &read.page_edits {
        let id = page_id.clone();
        let page = ctx
            .state
            .with_db(move |db| knowledge::get_page(db.conn(), &id))
            .await
            .map_err(plain)?;
        progress.begin(ctx, format!("Writing the change to {}", page.title)).await;
        let glossary = ctx
            .glossary(&page.category_id, vec![page.body.clone(), change.clone()])
            .await;
        let messages = prompts::edit_page(&page.body, change, &sources, &glossary, &input.history)
            .map_err(|e| plain(e.into()))?;
        let edited = ctx
            .complete(&resolved.target, &messages, &options(None, PAGE_TIMEOUT_S, &job.effort))
            .await
            .map_err(plain)?;
        if !replies::is_page(&edited.text) {
            // A plain answer: the change can't be made.
            extra.push(replies::strip_fence(&edited.text).trim().to_string());
            continue;
        }
        if edited.cut_off() {
            extra.push(format!(
                "The change to “{}” was cut off, so it is not offered. Please ask again.",
                page.title
            ));
            continue;
        }
        let new_body = edited_body(&edited.text, &page.title);
        if same_page(&new_body, &page.body) {
            extra.push(format!("No change was made to “{}”.", page.title));
            continue;
        }
        rows.push(ProposalRow {
            change: ProposedChange::UpdatePage {
                page_id: page.id.clone(),
                title: page.title.clone(),
                change: change.clone(),
                old_body: page.body.clone(),
                new_body,
                base_updated_at: page.updated_at.clone(),
            },
            ticked: true,
            state: RowState::Pending,
            result: None,
            result_id: None,
        });
    }

    // 4. Glossary terms for the new tasks.
    let created: Vec<(usize, String, String, Vec<RawTerm>)> = read
        .terms
        .iter()
        .filter_map(|(i, raw)| match read.changes.get(*i) {
            Some(ProposedChange::CreateTask { category_id, title, description, .. }) => {
                Some((*i, category_id.clone(), format!("{title}\n{description}"), raw.clone()))
            }
            _ => None,
        })
        .collect();
    let (terms, shown_terms, terms_category_id) = if created.is_empty() {
        (Vec::new(), Vec::new(), None)
    } else {
        ctx.state
            .with_db(move |db| {
                let mut kept: Vec<TermSuggestion> = Vec::new();
                let mut shown: Vec<String> = Vec::new();
                let mut category = None;
                for (_, category_id, text, raw) in created {
                    for t in checks::filter_terms(db.conn(), &category_id, &text, raw, &shown)?.kept {
                        shown.push(t.term.clone());
                        category.get_or_insert_with(|| category_id.clone());
                        kept.push(t);
                    }
                }
                Ok((kept, shown, category))
            })
            .await
            .map_err(plain)?
    };

    // 5. The message. When the model proposed changes and every one was dropped (the task is
    // already like that, or the target couldn't be matched), its reply describes changes that won't
    // happen, so it is replaced.
    // A page change that couldn't be made explains itself in `extra` instead.
    let all_dropped = rows.is_empty()
        && card.is_none()
        && read.proposed > 0
        && extra.iter().all(|e| e.is_empty());
    let mut text = if all_dropped {
        "Nothing needed changing: what you asked for is already the case, or it didn't match a task or page I could find.".to_string()
    } else {
        read.reply.clone()
    };
    for e in extra.iter().filter(|e| !e.is_empty()) {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(e);
    }
    if text.is_empty() {
        text = match (rows.is_empty(), card.is_some()) {
            (false, _) => "Here is what I would change.".into(),
            (true, true) => "Here is a draft page.".into(),
            (true, false) => "Nothing to change was found.".into(),
        };
    }
    if !rows.is_empty() {
        let proposal = Proposal {
            reply: text.clone(),
            rows,
            terms,
            shown_terms,
            terms_category_id,
        };
        ctx.patch(
            &job.assistant_id,
            MessagePatch { proposal: Some(proposal), ..MessagePatch::default() },
        )
        .await;
    }
    progress.settle(StepState::Done);
    Ok(Done { text, card })
}

// ---------------------------------------------------------------- the card's buttons

/// Changes the stored proposal of a message with `f`.
fn edit_proposal(
    db: &mut Db,
    message_id: &str,
    f: impl FnOnce(&mut Db, &mut Proposal) -> cairn_core::Result<()>,
) -> cairn_core::Result<ChatMessage> {
    let message = cairn_core::conversations::get_message(db.conn(), message_id)?;
    let mut proposal = message
        .proposal
        .ok_or_else(|| CoreError::Invalid("This message has no proposal.".into()))?;
    f(db, &mut proposal)?;
    cairn_core::conversations::update_message(
        db,
        message_id,
        MessagePatch { proposal: Some(proposal), ..MessagePatch::default() },
    )
}

fn set_row(db: &mut Db, message_id: &str, row: u32, ticked: bool) -> cairn_core::Result<ChatMessage> {
    edit_proposal(db, message_id, |_, p| {
        let r = p
            .rows
            .get_mut(row as usize)
            .ok_or_else(|| CoreError::Invalid("There is no such change.".into()))?;
        if r.state != RowState::Pending {
            return Err(CoreError::Invalid("That change was already handled.".into()));
        }
        r.ticked = ticked;
        Ok(())
    })
}

/// What a confirmation touched, to tell the UI.
#[derive(Debug, Default, PartialEq, Eq)]
struct Touched {
    tasks: bool,
    knowledge: bool,
}

impl Touched {
    fn entities(&self) -> Vec<Entity> {
        let mut e = vec![Entity::Conversations];
        if self.tasks {
            e.push(Entity::Tasks);
        }
        if self.knowledge {
            e.push(Entity::Knowledge);
        }
        e
    }
}

fn confirm(db: &mut Db, message_id: &str) -> cairn_core::Result<(ChatMessage, Touched)> {
    let mut touched = Touched::default();
    let message = edit_proposal(db, message_id, |db, p| {
        for row in p.rows.iter_mut().filter(|r| r.state == RowState::Pending) {
            if !row.ticked {
                row.state = RowState::Rejected;
                continue;
            }
            match actions::apply(db, &row.change) {
                Ok((line, id)) => {
                    row.state = RowState::Done;
                    row.result = Some(line);
                    row.result_id = id;
                    match &row.change {
                        ProposedChange::UpdatePage { .. } => touched.knowledge = true,
                        ProposedChange::CreateTask { links, .. }
                        | ProposedChange::UpdateTask { links, .. } => {
                            touched.tasks = true;
                            touched.knowledge |= links
                                .iter()
                                .any(|l| l.kind == cairn_core::assistant::parts::LinkKind::Page);
                        }
                        ProposedChange::Comment { .. } => touched.tasks = true,
                    }
                }
                Err(e) => {
                    row.state = RowState::Failed;
                    row.result = Some(e.to_string());
                }
            }
        }
        Ok(())
    })?;
    Ok((message, touched))
}

fn reject(db: &mut Db, message_id: &str) -> cairn_core::Result<ChatMessage> {
    edit_proposal(db, message_id, |_, p| {
        for row in p.rows.iter_mut().filter(|r| r.state == RowState::Pending) {
            row.state = RowState::Rejected;
        }
        Ok(())
    })
}

/// Ticks or unticks one row of a pending proposal.
#[tauri::command]
#[specta::specta]
pub async fn set_proposal_row(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: String,
    row: u32,
    ticked: bool,
) -> ApiResult<ChatMessage> {
    let m = state
        .with_db(move |db| set_row(db, &message_id, row, ticked))
        .await?;
    emit_changed(&app, &[Entity::Conversations]);
    Ok(m)
}

/// *Confirm*: applies the ticked pending rows; unticked ones become rejected. Each row records
/// its result ("Created OPS-48") or why it failed; the others still apply.
#[tauri::command]
#[specta::specta]
pub async fn confirm_proposal(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: String,
) -> ApiResult<ChatMessage> {
    let (m, touched) = state.with_db(move |db| confirm(db, &message_id)).await?;
    emit_changed(&app, &touched.entities());
    Ok(m)
}

/// *Reject*: every pending row becomes rejected; nothing changes.
#[tauri::command]
#[specta::specta]
pub async fn reject_proposal(
    app: AppHandle,
    state: State<'_, AppState>,
    message_id: String,
) -> ApiResult<ChatMessage> {
    let m = state.with_db(move |db| reject(db, &message_id)).await?;
    emit_changed(&app, &[Entity::Conversations]);
    Ok(m)
}

#[cfg(test)]
mod tests;
