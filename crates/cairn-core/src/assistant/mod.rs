//! The Assistant (M6, D-44, spec §6.4).
//!
//! Each message in a conversation goes through (the Tauri layer runs it as a job,
//! `src-tauri/src/commands/assistant.rs`):
//! 1. **Web** (🌐 on, FR-LLM-AS-12): only `ask-web` with the provider's web search; nothing else
//!    from the user's data. A reply without a search is labelled, and on Chat Completions or a
//!    local model removes the model's *Web search* tick.
//! 2. **The first step** (`ask-plan`, the conversation's model, JSON → [`plan::Plan`]).
//! 3. By the plan's kind:
//!    - question, file question, general: [`retrieve`] the sources (none for general), then
//!      `ask-answer`, streamed; the `[n]` markers are read by [`markers::read`] into an
//!      [`parts::AnswerPart`]; an attached document gets `doc-terms` once;
//!    - find: [`find`] lists the matches under the plan's intro ([`parts::FindPart`]);
//!    - create, change: `ask-action` → [`actions::read`] → a [`parts::Proposal`] card (with
//!      `ask-page` into a draft card and `edit-page` diffs);
//!    - file draft: the M5 file flow.
//!
//! The earlier conversation goes with each call within the history budget ([`history`]). The
//! prompt variables are formatted by [`formats`] exactly as the prompts README specifies.

pub mod actions;
pub mod find;
pub mod formats;
pub mod history;
pub mod items;
pub mod markers;
pub mod parts;
pub mod plan;
pub mod references;
pub mod retrieve;
