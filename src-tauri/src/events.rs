//! Events from Rust to the UI (spec TC-13). The UI turns `DataChanged` into query refreshes.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::indexer::Indexer;

/// A kind of data the UI caches. Its camelCase name is the first part of the matching
/// TanStack Query keys in `app/src/api/` (e.g. `["categories", ...]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Entity {
    Categories,
    Settings,
    /// Tasks with their comments, links, versions and summaries.
    Tasks,
    /// The recent-activity log.
    Activity,
    /// Groups, pages (with tags, versions) and task ↔ page links.
    Knowledge,
    /// "Mentioned in" lists.
    Mentions,
    /// Keyword search results.
    Search,
    /// AI providers and their models (M4).
    Providers,
    /// Glossary entries and their "used in" lists (M4); ignored suggestions (M5).
    Glossary,
    /// Saved conversations and their messages (M5, FR-LLM-AS-5).
    Conversations,
    /// The backup list and the *Last backup* line (M7).
    Backups,
    /// Everything changed, e.g. after an import.
    All,
}

/// Sent after a command or background job changed data.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct DataChanged {
    pub entities: Vec<Entity>,
}

/// Tell the UI that `entities` changed. A failed emit is logged, never fatal.
/// Changes to pages, tasks or glossary entries also wake the background indexer (FR-RAG-IX-5).
pub fn emit_changed(app: &AppHandle, entities: &[Entity]) {
    if entities.iter().any(|e| {
        matches!(
            e,
            Entity::Tasks | Entity::Knowledge | Entity::Glossary | Entity::All
        )
    }) && let Some(indexer) = app.try_state::<Arc<Indexer>>()
    {
        indexer.wake();
    }
    // Saving a page or task updates which glossary entries it uses ("Used in", use counts).
    let mut entities = entities.to_vec();
    if entities
        .iter()
        .any(|e| matches!(e, Entity::Tasks | Entity::Knowledge))
        && !entities.contains(&Entity::Glossary)
    {
        entities.push(Entity::Glossary);
    }
    if let Err(e) = (DataChanged { entities }).emit(app) {
        tracing::warn!(error = %e, "failed to emit DataChanged");
    }
}
