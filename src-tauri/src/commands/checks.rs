//! The save-pipeline checks (FR-KN-SV-1..4, UI-KN-6, UI-KN-7): the `check` call (topics, group,
//! tags, glossary suggestions), the duplicate check by meaning (FR-KN-DU-1), *Keep one topic only*
//! (FR-KN-DV-3), and *Ignore* on a suggested term (FR-KN-GL-7).
//!
//! A step without a usable default model fails with `llm_unavailable` (the UI skips it and saves
//! "not checked", FR-KN-SV-3); other failures with `ai_<kind>` (*Retry* / *Skip*).

use std::sync::Arc;

use cairn_core::checks::{self, CheckReport, DuplicateHit};
use cairn_core::Db;
use cairn_core::chunking;
use cairn_core::glossary;
use cairn_core::knowledge;
use cairn_core::categories;
use cairn_core::prompts::{self, GroupLine, ModelRole};
use cairn_core::search::{self, SearchKind, SearchQuery};
use cairn_core::settings;
use cairn_core::vectors::{self, MeaningHit};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::commands::search::{MeaningStatus, NoMeaning, meaning_status};
use crate::error::ApiResult;
use crate::events::{Entity, emit_changed};
use crate::llm::{self, CallOptions, DraftText, Jobs};
use crate::sidecar::Sidecars;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckInput {
    pub category_id: String,
    pub title: String,
    /// The page Markdown, `# title` line included.
    pub body: String,
    /// Terms already shown for this draft (not suggested again).
    #[specta(optional)]
    pub shown_terms: Option<Vec<String>>,
    /// *Stop* id (`cancel_job`).
    #[specta(optional)]
    pub job_id: Option<String>,
}

/// What the `check` prompt needs from the database: the category's name and groups, the glossary
/// lines for the page.
struct CheckContext {
    category: String,
    groups: Vec<GroupLine>,
    glossary: String,
}

fn check_context(db: &Db, category_id: &str, page: &str) -> cairn_core::Result<CheckContext> {
    let conn = db.conn();
    let category = categories::get(conn, category_id)?.name;
    // Nested groups (external mode) by their path, `Parent / Child` (D-72).
    let groups = knowledge::groups_with_paths(conn, category_id)?
        .into_iter()
        .map(|g| GroupLine {
            name: g.name,
            description: g.description,
            proposed: false,
        })
        .collect();
    let entries = glossary::prompt_entries(conn, category_id, &[page])?;
    Ok(CheckContext {
        category,
        groups,
        glossary: prompts::glossary_lines(&entries),
    })
}

/// The `check` call on the default model for a page or draft: the category's groups and the
/// glossary lines go into `prompts::check`, the reply is read by `checks::read_check` and
/// `checks::filter_terms` (`shown_terms` skipped). Also used by the chat job to fill a new draft
/// card (`commands::files`), which calls it only after `llm::resolve(.., ModelRole::Default, ..)`
/// succeeded. Takes the app handle (states via `app.state::<AppState>()` / `<Sidecars>()`), so it
/// can run inside spawned jobs.
pub(crate) async fn run_check(app: AppHandle, input: CheckInput) -> ApiResult<CheckReport> {
    let state = app.state::<AppState>();
    let sidecars = app.state::<Sidecars>();
    let resolved = llm::resolve(&state, ModelRole::Default, None).await?;

    let (category_id, body) = (input.category_id.clone(), input.body.clone());
    let ctx = state
        .with_db(move |db| check_context(db, &category_id, &body))
        .await?;
    let messages = prompts::check(&ctx.category, &ctx.groups, &ctx.glossary, &input.body)?;
    let opts = CallOptions {
        timeout_s: Some(60),
        ..CallOptions::default()
    };
    let (reply, _) = llm::complete_json(&app, &sidecars, &resolved.target, &messages, &opts).await?;

    let shown = input.shown_terms.clone().unwrap_or_default();
    state
        .with_db(move |db| {
            let conn = db.conn();
            let groups = knowledge::groups_with_paths(conn, &input.category_id)?;
            let (mut report, raw) = checks::read_check(&reply, &groups, &input.title);
            let filtered = checks::filter_terms(conn, &input.category_id, &input.body, raw, &shown)?;
            // NFR-SEC-3: the reason only, never the term (text of the page).
            for (_, why) in &filtered.dropped {
                tracing::debug!(why, "glossary suggestion dropped");
            }
            report.terms = filtered.kept;
            Ok(report)
        })
        .await
}

/// [`run_check`] as a command, cancellable with `input.job_id`.
#[tauri::command]
#[specta::specta]
pub async fn check_page(
    app: AppHandle,
    _state: State<'_, AppState>,
    _sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    input: CheckInput,
) -> ApiResult<CheckReport> {
    let id = input.job_id.clone();
    jobs.run(id, run_check(app, input)).await
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateInput {
    pub category_id: String,
    pub title: String,
    pub body: String,
    /// The page itself (*Run checks*), or the page being merged away.
    #[specta(optional)]
    pub exclude_page_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateReport {
    pub hits: Vec<DuplicateHit>,
    /// How far the index is (FR-RAG-IX-6); `unavailable` = keyword fallback.
    pub meaning: MeaningStatus,
}

/// How many pages the UI shows (FR-KN-DU-1).
const DUPLICATE_LIMIT: usize = 3;
/// Candidates fetched before ranking; enough that excluding one page and merging pieces of the
/// same page still leaves [`DUPLICATE_LIMIT`].
const CANDIDATES: u32 = 30;
/// Pieces per `embed.documents` call (PROTOCOL.md "Concurrency").
const EMBED_BATCH: usize = 8;

/// The text of a piece as the indexer embeds it: heading path, then the text.
fn piece_text(heading: &str, text: &str) -> String {
    if heading.is_empty() {
        text.to_string()
    } else {
        format!("{heading}\n{text}")
    }
}

fn page_filter(limit: u32) -> SearchQuery {
    SearchQuery {
        text: String::new(),
        kinds: Some(vec![SearchKind::Page]),
        include_archived: Some(false),
        limit: Some(limit),
        ..SearchQuery::default()
    }
}

/// The duplicate report from the draft's piece vectors, or, when the index can't answer yet (no
/// vectors, or nothing found while pieces are still waiting), from keyword search on the title.
/// `embedded` is the sidecar's answer or why it failed.
fn duplicate_report(
    db: &Db,
    input: &DuplicateInput,
    embedded: Result<Vec<Vec<f32>>, NoMeaning>,
) -> cairn_core::Result<DuplicateReport> {
    let conn = db.conn();
    let threshold = settings::get(conn)?.duplicate_score;
    let status = vectors::status(conn)?;
    let exclude = input.exclude_page_id.as_deref();
    let rank = |hits: Vec<MeaningHit>| {
        let hits: Vec<MeaningHit> = hits.into_iter().filter(|h| Some(h.id.as_str()) != exclude).collect();
        checks::rank_duplicates(hits, &input.category_id, threshold, DUPLICATE_LIMIT)
    };

    let problem = match embedded {
        Ok(vectors_of_draft) => {
            let hits = rank(vectors::similar_to_vectors(conn, &vectors_of_draft, &page_filter(CANDIDATES))?);
            if !hits.is_empty() || status.items_ready >= status.items {
                return Ok(DuplicateReport {
                    hits,
                    meaning: meaning_status(&status, None),
                });
            }
            "Not every page is indexed by meaning yet; showing pages with similar words in the title.".into()
        }
        Err(problem) => problem,
    };

    // Keyword fallback: the title's words. Scores of keyword search aren't similarities, so the
    // hits carry 0 and are never "likely".
    let by_title = SearchQuery {
        text: input.title.clone(),
        ..page_filter(CANDIDATES)
    };
    let keyword = search::search(conn, &by_title)?
        .into_iter()
        .map(|h| MeaningHit {
            kind: h.kind,
            id: h.id,
            title: h.title,
            key: h.key,
            category_id: h.category_id,
            chunk_id: 0,
            heading: String::new(),
            text: h.snippet,
            score: 0.0,
        })
        .collect();
    let mut hits = rank(keyword);
    for hit in &mut hits {
        hit.likely = false;
    }
    Ok(DuplicateReport {
        hits,
        meaning: meaning_status(&status, Some(problem)),
    })
}

/// FR-KN-DU-1: the draft's pieces (`chunking::split_page`) are embedded as documents and compared
/// with the index (`vectors::similar_to_vectors`), across all categories; keyword search on the title
/// while the index isn't ready. Ranked by `checks::rank_duplicates` with Settings › Knowledge
/// "likely the same topic".
#[tauri::command]
#[specta::specta]
pub async fn find_duplicates(
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    input: DuplicateInput,
) -> ApiResult<DuplicateReport> {
    // An empty draft still has its title to compare.
    let mut items: Vec<(String, String)> = chunking::split_page(&input.body)
        .iter()
        .map(|p| (input.title.clone(), piece_text(&p.heading, &p.text)))
        .collect();
    if items.is_empty() {
        items.push((input.title.clone(), String::new()));
    }
    let mut embedded: Result<Vec<Vec<f32>>, NoMeaning> = Ok(Vec::new());
    for batch in items.chunks(EMBED_BATCH) {
        match sidecars.embed_documents(batch.to_vec()).await {
            Ok(vs) => {
                if let Ok(all) = &mut embedded {
                    all.extend(vs);
                }
            }
            Err(e) => {
                tracing::info!(kind = %e.kind, "duplicate check by meaning unavailable: {}", e.message);
                embedded = Err(e.into());
                break;
            }
        }
    }
    state
        .with_db(move |db| duplicate_report(db, &input, embedded))
        .await
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KeepTopicInput {
    pub category_id: String,
    pub body: String,
    pub topic: String,
    pub topics: Vec<String>,
    /// Streams the new draft as `LlmDelta`; also the *Stop* id.
    #[specta(optional)]
    pub stream_id: Option<String>,
}

/// FR-KN-DV-3 on the default model (`keep-one-topic`).
#[tauri::command]
#[specta::specta]
pub async fn keep_one_topic(
    app: AppHandle,
    state: State<'_, AppState>,
    _sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    input: KeepTopicInput,
) -> ApiResult<DraftText> {
    let resolved = llm::resolve(&state, ModelRole::Default, None).await?;
    let (category_id, body) = (input.category_id.clone(), input.body.clone());
    let glossary_text = state
        .with_db(move |db| {
            let entries = glossary::prompt_entries(db.conn(), &category_id, &[&body])?;
            Ok(prompts::glossary_lines(&entries))
        })
        .await?;
    let messages = prompts::keep_one_topic(&input.topic, &input.topics, &input.body, &glossary_text)?;
    let opts = CallOptions {
        stream_id: input.stream_id.clone(),
        timeout_s: Some(300),
        ..CallOptions::default()
    };
    let topic = input.topic;
    jobs.run(input.stream_id, async move {
        let sidecars = app.state::<Sidecars>();
        let reply = llm::complete(&app, &sidecars, &resolved.target, &messages, &opts).await?;
        Ok(DraftText::from_reply(&reply, &topic))
    })
    .await
}

/// FR-KN-GL-7 *Ignore*: the term is not suggested in this category again.
#[tauri::command]
#[specta::specta]
pub async fn ignore_glossary_term(
    app: AppHandle,
    state: State<'_, AppState>,
    category_id: String,
    term: String,
) -> ApiResult<()> {
    state
        .with_db(move |db| glossary::ignore_term(db, &category_id, &term))
        .await?;
    emit_changed(&app, &[Entity::Glossary]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_core::Db;
    use cairn_core::knowledge::NewPage;
    use cairn_core::search::SearchKind as Kind;

    const DIMS: usize = 8;

    fn axis(i: usize, j: usize, mix: f32) -> Vec<f32> {
        let mut v = vec![0.0; DIMS];
        v[i] = 1.0;
        v[j] += mix;
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    /// Two categories and four pages; every page gets the vector `vector(title)` unless it is None.
    fn setup(vector: impl Fn(&str) -> Option<Vec<f32>>) -> (tempfile::TempDir, Db, Vec<(String, String)>) {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        for (id, key) in [("p1", "ONE"), ("p2", "TWO")] {
            db.conn()
                .execute(
                    "INSERT INTO categories (id, name, key, created_at, updated_at) VALUES (?1, ?1, ?2, 'x', 'x')",
                    [id, key],
                )
                .unwrap();
        }
        let mut ids = Vec::new();
        for (category, title, body) in [
            ("p1", "Hose swap", "Swap the swivel hose on the auger."),
            ("p2", "Hose replacement", "Replace the hose on the auger boom."),
            ("p2", "Budget", "Quarterly budget review."),
            ("p1", "Hose notes", "Notes about hoses."),
        ] {
            let page = knowledge::create_page(
                &mut db,
                NewPage {
                    category_id: category.into(),
                    title: title.into(),
                    body: format!("# {title}\n\n{body}"),
                    group_id: None,
                    tags: None,
                    source_info: None,
                    origin: None,
                    origin_task_id: None,
                    check_status: None,
                },
            )
            .unwrap();
            ids.push((title.to_string(), page.id));
        }
        let done: Vec<_> = vectors::next_jobs(db.conn(), 100)
            .unwrap()
            .into_iter()
            .filter_map(|j| vector(&j.title).map(|v| (j, v)))
            .collect();
        vectors::store_vectors(db.conn(), &done).unwrap();
        (dir, db, ids)
    }

    fn input(title: &str, exclude: Option<&str>) -> DuplicateInput {
        DuplicateInput {
            category_id: "p1".into(),
            title: title.into(),
            body: format!("# {title}\n\nbody"),
            exclude_page_id: exclude.map(str::to_string),
        }
    }

    fn titles(r: &DuplicateReport) -> Vec<&str> {
        r.hits.iter().map(|h| h.title.as_str()).collect()
    }

    #[test]
    fn duplicates_are_ranked_by_meaning() {
        let (_d, db, ids) = setup(|title| match title {
            "Hose swap" => Some(axis(0, 1, 0.3)),
            "Hose replacement" => Some(axis(0, 1, 0.05)),
            "Hose notes" => Some(axis(0, 1, 1.0)), // cosine about 0.7 with axis 0
            _ => Some(axis(3, 4, 0.0)),
        });
        let report = duplicate_report(&db, &input("Draft", None), Ok(vec![axis(0, 2, 0.0)])).unwrap();
        // Likely ones (>= 0.8) first, the draft's own category before the other (same-category
        // "Hose swap" ranks above the other category's, though its score is lower), then the rest.
        assert_eq!(titles(&report), ["Hose swap", "Hose replacement", "Hose notes"]);
        assert!(report.hits[0].likely && report.hits[1].likely && !report.hits[2].likely);
        assert!(report.hits[1].score > report.hits[0].score);
        assert_eq!(report.meaning.state, crate::commands::search::MeaningState::Ready);

        // The page itself is not its own duplicate.
        let own = &ids[0].1;
        let report = duplicate_report(&db, &input("Draft", Some(own)), Ok(vec![axis(0, 2, 0.0)])).unwrap();
        assert_eq!(titles(&report), ["Hose replacement", "Hose notes", "Budget"]);
        assert!(report.hits.iter().all(|h| h.page_id != *own));
    }

    #[test]
    fn without_an_installed_model_the_fallback_is_not_a_warning() {
        let (_d, db, _) = setup(|_| None);
        let report = duplicate_report(&db, &input("Hose", None), Err(NoMeaning::NotInstalled)).unwrap();
        assert_eq!(report.meaning.state, crate::commands::search::MeaningState::NotInstalled);
        assert_eq!(report.meaning.detail, None);
        assert_eq!(report.hits.len(), 3);
    }

    #[test]
    fn duplicates_fall_back_to_keywords_without_the_search_model() {
        let (_d, db, _) = setup(|_| None);
        let report = duplicate_report(&db, &input("Hose", None), Err("The search model is off.".into())).unwrap();
        assert_eq!(report.meaning.state, crate::commands::search::MeaningState::Unavailable);
        assert_eq!(report.meaning.detail.as_deref(), Some("The search model is off."));
        let mut found = titles(&report);
        found.sort_unstable();
        assert_eq!(found, ["Hose notes", "Hose replacement", "Hose swap"]);
        assert!(report.hits.iter().all(|h| !h.likely && h.score == 0.0));
        // The draft's category first.
        assert_eq!(report.hits[0].category_id, "p1");
        assert!(!report.hits[0].snippet.contains('«'));
    }

    #[test]
    fn duplicates_fall_back_while_pieces_are_not_indexed() {
        // The model answers, but no page has a vector yet: nothing to compare with.
        let (_d, db, _) = setup(|_| None);
        let report = duplicate_report(&db, &input("Hose swap", None), Ok(vec![axis(0, 1, 0.0)])).unwrap();
        assert_eq!(report.meaning.state, crate::commands::search::MeaningState::Unavailable);
        assert!(titles(&report).contains(&"Hose swap"));
        let _ = Kind::Page;
    }
}
