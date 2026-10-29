//! Keyword search (FR-KN-UI-2, FR-RAG-SR-1 keyword part) and spelling/grammar checks (FR-KN-ED-6..8).

use std::sync::Arc;

use cairn_core::hybrid;
use cairn_core::knowledge;
use cairn_core::lint::{self, LintHit, LintOptions};
use cairn_core::search::{self, MatchedBy, SearchHit, SearchKind, SearchQuery};
use cairn_core::settings;
use cairn_core::vectors::{self, IndexStatus, MeaningHit};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::indexer::{IndexProgress, Indexer};
use crate::sidecar::{NOT_INSTALLED, SidecarError, Sidecars};
use crate::state::AppState;

/// Whether meaning-based results could be used (FR-RAG-IX-6: the UI warns unless `ready`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MeaningState {
    /// The search model answered and every item is indexed.
    Ready,
    /// The search model answered, but some items are not indexed yet.
    Partial,
    /// The search model could not be used: keyword results only.
    Unavailable,
    /// No search model is installed (a lite package, D-82): keyword results only, and no warning.
    NotInstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MeaningStatus {
    pub state: MeaningState,
    /// Pages and tasks in the index, and how many of them are fully indexed.
    pub items: u32,
    pub items_ready: u32,
    /// Plain-language reason when `unavailable`.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HybridResult {
    pub hits: Vec<SearchHit>,
    pub meaning: MeaningStatus,
}

/// Pages and tasks matching `query.text`, best first.
#[tauri::command]
#[specta::specta]
pub async fn search(state: State<'_, AppState>, query: SearchQuery) -> ApiResult<Vec<SearchHit>> {
    state
        .with_db(move |db| search::search(db.conn(), &query))
        .await
}

/// Keyword + meaning search merged by reciprocal rank fusion (FR-RAG-SR-1, -4, -5). Embeds the
/// query in the `embed` sidecar (starting it if needed); if that fails, returns the keyword results
/// with `meaning.state = unavailable`.
#[tauri::command]
#[specta::specta]
pub async fn search_hybrid(
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    query: SearchQuery,
) -> ApiResult<HybridResult> {
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    // Both parts get twice the limit, so the merged list has enough to choose from.
    let wide = SearchQuery {
        limit: Some(limit.saturating_mul(2)),
        ..query.clone()
    };
    if query.text.trim().is_empty() {
        let status = state.with_db(|db| vectors::status(db.conn())).await?;
        return Ok(HybridResult {
            hits: Vec::new(),
            meaning: meaning_status(&status, None),
        });
    }

    // Keyword search and the query embedding run at the same time (NFR-PERF-3).
    let keyword_query = wide.clone();
    let keyword = state.with_db(move |db| search::search(db.conn(), &keyword_query));
    let embedded = sidecars.embed_query(&query.text);
    let (keyword, embedded) = tokio::join!(keyword, embedded);
    let mut keyword = keyword?;

    match embedded {
        Err(e) => {
            // FR-RAG-IX-6: keyword results still work; say that meaning could not be used.
            tracing::info!(kind = %e.kind, "search by meaning unavailable: {}", e.message);
            keyword.truncate(limit as usize);
            let status = state
                .with_db(|db| vectors::status(db.conn()))
                .await
                .unwrap_or_default();
            Ok(HybridResult {
                hits: keyword,
                meaning: meaning_status(&status, Some(e.into())),
            })
        }
        Ok(vector) => {
            let (meaning, status) = state
                .with_db(move |db| {
                    Ok((
                        vectors::search(db.conn(), &vector, &wide)?,
                        vectors::status(db.conn())?,
                    ))
                })
                .await?;
            Ok(HybridResult {
                hits: hybrid::fuse(keyword, meaning, limit),
                meaning: meaning_status(&status, None),
            })
        }
    }
}

const DEFAULT_LIMIT: u32 = 20;

/// A meaning-only hit as a search result: score = cosine similarity, snippet = start of the piece.
fn meaning_hit(h: MeaningHit) -> SearchHit {
    SearchHit {
        kind: h.kind,
        id: h.id,
        title: h.title,
        key: h.key,
        category_id: h.category_id,
        snippet: hybrid::excerpt(&h.text),
        score: f64::from(h.score),
        matched_by: MatchedBy::Meaning,
    }
}

/// Why search by meaning wasn't used.
pub(crate) enum NoMeaning {
    /// No search model is installed (D-82).
    NotInstalled,
    /// It failed or isn't ready, with the reason shown.
    Unavailable(String),
}

impl From<SidecarError> for NoMeaning {
    fn from(e: SidecarError) -> Self {
        if e.kind == NOT_INSTALLED {
            Self::NotInstalled
        } else {
            Self::Unavailable(sidecar_detail(e))
        }
    }
}

impl From<String> for NoMeaning {
    fn from(detail: String) -> Self {
        Self::Unavailable(detail)
    }
}

impl From<&str> for NoMeaning {
    fn from(detail: &str) -> Self {
        Self::Unavailable(detail.into())
    }
}

/// `unavailable` (with a reason) or `not_installed`, else `ready` or `partial` by how many items
/// are indexed.
pub(crate) fn meaning_status(status: &IndexStatus, problem: Option<NoMeaning>) -> MeaningStatus {
    let (state, detail) = match problem {
        Some(NoMeaning::NotInstalled) => (MeaningState::NotInstalled, None),
        Some(NoMeaning::Unavailable(detail)) => (MeaningState::Unavailable, Some(detail)),
        None if status.items_ready >= status.items => (MeaningState::Ready, None),
        None => (MeaningState::Partial, None),
    };
    MeaningStatus {
        state,
        items: status.items,
        items_ready: status.items_ready,
        detail,
    }
}

pub(crate) fn sidecar_detail(e: SidecarError) -> String {
    match e.hint {
        Some(h) if !h.is_empty() => format!("{} ({h})", e.message),
        _ => e.message,
    }
}

/// *Find similar pages* (FR-RAG-SR-5): pages closest in meaning to `page_id` (score = cosine
/// similarity, best first; `matchedBy = meaning`). Without the index, falls back to a keyword
/// search on the title with `meaning.state = unavailable`.
#[tauri::command]
#[specta::specta]
pub async fn find_similar_pages(
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    page_id: String,
    limit: Option<u32>,
) -> ApiResult<HybridResult> {
    let limit = limit.unwrap_or(10);
    let installed = sidecars.model_installed();
    state
        .with_db(move |db| {
            let conn = db.conn();
            let filter = SearchQuery {
                text: String::new(),
                kinds: Some(vec![SearchKind::Page]),
                include_archived: Some(false),
                limit: Some(limit),
                ..SearchQuery::default()
            };
            let status = vectors::status(conn)?;
            let similar = vectors::similar_items(conn, SearchKind::Page, &page_id, &filter)?;
            if !similar.is_empty() || status.items_ready >= status.items {
                return Ok(HybridResult {
                    hits: similar.into_iter().map(meaning_hit).collect(),
                    meaning: meaning_status(&status, None),
                });
            }

            // Not everything is indexed yet (maybe this page too): use the title's words instead.
            let title = knowledge::get_page(conn, &page_id)?.title;
            let by_title = SearchQuery {
                text: title,
                limit: Some(limit + 1),
                ..filter
            };
            let mut hits = search::search(conn, &by_title)?;
            hits.retain(|h| h.id != page_id);
            hits.truncate(limit as usize);
            Ok(HybridResult {
                hits,
                meaning: meaning_status(
                    &status,
                    Some(if installed {
                        "This page is not indexed by meaning yet; showing pages with similar words in the title.".into()
                    } else {
                        NoMeaning::NotInstalled
                    }),
                ),
            })
        })
        .await
}

/// Vector index progress (Settings › Search, FR-RAG-IX-5). Changes arrive as the `IndexProgress` event.
#[tauri::command]
#[specta::specta]
pub async fn get_index_progress(
    state: State<'_, AppState>,
    indexer: State<'_, Arc<Indexer>>,
) -> ApiResult<IndexProgress> {
    // The counts come from the database, so they are right even before the indexer's first run.
    let status = state.with_db(|db| vectors::status(db.conn())).await?;
    Ok(indexer.with_status(status))
}

/// Rebuilds the keyword and mention indexes (Settings → Search, after import it runs automatically).
#[tauri::command]
#[specta::specta]
pub async fn rebuild_search_index(app: AppHandle, state: State<'_, AppState>) -> ApiResult<()> {
    state
        .with_db(move |db| cairn_core::hooks::rebuild_all(db.conn()))
        .await?;
    emit_changed(&app, &[Entity::Search, Entity::Mentions]);
    // The pieces were re-cut: the indexer embeds only those that changed (FR-RAG-IX-2).
    if let Some(indexer) = app.try_state::<Arc<Indexer>>() {
        indexer.wake();
    }
    Ok(())
}

/// Spelling and grammar issues in Markdown `text`, with UTF-16 offsets. Uses the personal
/// dictionary from settings. Harper loads on the first call (about 0.6 s, then ~16 ms per 1,000 words).
#[tauri::command]
#[specta::specta]
pub async fn lint_text(
    state: State<'_, AppState>,
    text: String,
    options: LintOptions,
) -> ApiResult<Vec<LintHit>> {
    // The dictionary is read under the database lock; Harper runs outside it (it needs no database
    // and can take tens of milliseconds, or 0.6 s on the first call), so saves are never blocked.
    // The glossary's words (terms and other names) are accepted too (FR-KN-GL-6).
    let dictionary = state
        .with_db(|db| {
            let mut words = settings::get(db.conn())?.personal_dictionary;
            words.extend(cairn_core::glossary::spelling_words(db.conn())?);
            Ok(words)
        })
        .await?;
    tauri::async_runtime::spawn_blocking(move || lint::lint(&text, options, &dictionary))
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?
        .map_err(ApiError::from)
}
