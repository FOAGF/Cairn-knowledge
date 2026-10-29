//! Task → knowledge (FR-TASK-KN-2), the merge screen's help (FR-KN-DU-3, FR-KN-DU-5) and
//! *Suggest groups for ungrouped pages* (FR-KN-GR-3). All on the default model; without one they
//! fail with `llm_unavailable` and the UI offers the manual way (plain copy, manual merge).

use std::sync::Arc;

use cairn_core::checks::GroupPick;
use cairn_core::knowledge::{self, PageFilter};
use cairn_core::merging::{self, BlockMatch, GroupProposal, NewGroupName};
use cairn_core::prompts::{self, GroupLine, ModelRole};
use cairn_core::{glossary, categories, replies, tasks};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::error::ApiResult;
use crate::llm::{self, CallOptions, DraftText, Jobs};
use crate::sidecar::{SidecarError, Sidecars};
use crate::state::AppState;

/// Pages per `suggest-groups` call.
const GROUP_BATCH: usize = 20;
/// Blocks per `embed.documents` call (PROTOCOL.md "Concurrency").
const EMBED_BATCH: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskDraftInput {
    pub task_id: String,
    pub include_description: bool,
    /// The chosen comments; they are sent oldest first.
    pub comment_ids: Vec<String>,
    /// Streams the draft as `LlmDelta`; also the *Stop* id.
    #[specta(optional)]
    pub stream_id: Option<String>,
}

/// FR-TASK-KN-2: `task-draft` with the chosen description and comments.
#[tauri::command]
#[specta::specta]
pub async fn draft_from_task(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    input: TaskDraftInput,
) -> ApiResult<DraftText> {
    let _ = (state, sidecars);
    let stream_id = input.stream_id.clone();
    jobs.run(stream_id, async move {
        let state = app.state::<AppState>();
        let sidecars = app.state::<Sidecars>();
        let resolved = llm::resolve(&state, ModelRole::Default, None).await?;
        let (task_id, include, comment_ids) =
            (input.task_id.clone(), input.include_description, input.comment_ids.clone());
        let (task, category, comments, glossary_text) = state
            .with_db(move |db| {
                let conn = db.conn();
                let mut task = tasks::get(conn, &task_id)?;
                if !include {
                    task.description = String::new();
                }
                let category = categories::get(conn, &task.category_id)?;
                let mut chosen = tasks::comments(conn, &task.id)?;
                chosen.retain(|c| comment_ids.contains(&c.id));
                chosen.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                // Dates as YYYY-MM-DD (stored timestamps start with it).
                let comments: Vec<(String, String)> = chosen
                    .into_iter()
                    .map(|c| (c.created_at.chars().take(10).collect(), c.text))
                    .collect();
                let mut texts: Vec<&str> = vec![&task.title, &task.description];
                texts.extend(comments.iter().map(|(_, t)| t.as_str()));
                let entries = glossary::prompt_entries(conn, &category.id, &texts)?;
                let glossary_text = prompts::glossary_lines(&entries);
                Ok((task, category, comments, glossary_text))
            })
            .await?;
        let messages = prompts::task_draft(
            &task.key,
            &task.title,
            &category.name,
            &task.description,
            &comments,
            &glossary_text,
        )?;
        let opts = CallOptions {
            stream_id: input.stream_id,
            timeout_s: Some(300),
            max_tokens: None,
            effort: None,
            web_search: None,
        };
        let reply = llm::complete(&app, &sidecars, &resolved.target, &messages, &opts).await?;
        Ok(DraftText::from_reply(&reply, &task.title))
    })
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SuggestMergeInput {
    /// The existing page; its title is kept (README "Local checks").
    pub page_id: String,
    /// The new content, `# title` line included.
    pub new_body: String,
    #[specta(optional)]
    pub stream_id: Option<String>,
}

/// FR-KN-DU-5 (`suggest-merge`): fills the result area; nothing is saved.
#[tauri::command]
#[specta::specta]
pub async fn suggest_merge(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    input: SuggestMergeInput,
) -> ApiResult<DraftText> {
    let _ = (state, sidecars);
    let stream_id = input.stream_id.clone();
    jobs.run(stream_id, async move {
        let state = app.state::<AppState>();
        let sidecars = app.state::<Sidecars>();
        let resolved = llm::resolve(&state, ModelRole::Default, None).await?;
        let (page_id, new_body) = (input.page_id.clone(), input.new_body.clone());
        let (page, glossary_text) = state
            .with_db(move |db| {
                let conn = db.conn();
                let page = knowledge::get_page(conn, &page_id)?;
                let entries =
                    glossary::prompt_entries(conn, &page.category_id, &[&page.body, &new_body])?;
                Ok((page, prompts::glossary_lines(&entries)))
            })
            .await?;
        let messages = prompts::suggest_merge(&page.body, &input.new_body, &glossary_text)?;
        let opts = CallOptions {
            stream_id: input.stream_id,
            timeout_s: Some(300),
            max_tokens: None,
            effort: None,
            web_search: None,
        };
        let reply = llm::complete(&app, &sidecars, &resolved.target, &messages, &opts).await?;
        // The existing page keeps its title, whatever the model wrote.
        let mut draft = DraftText::from_reply(&reply, &page.title);
        draft.body = replies::with_title(&draft.body, &page.title);
        draft.title = page.title;
        Ok(draft)
    })
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MergeBlocksInput {
    /// The existing page's blocks, as the merge screen splits them.
    pub existing: Vec<String>,
    /// The new content's blocks.
    pub incoming: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MergeBlocksReport {
    /// One per incoming block; empty when `by_meaning` is false.
    pub matches: Vec<BlockMatch>,
    /// False when the search model couldn't be used: the UI keeps its keyword overlap.
    pub by_meaning: bool,
}

/// Vectors of `texts` as documents, in batches of [`EMBED_BATCH`].
async fn embed_blocks(sidecars: &Sidecars, texts: &[String]) -> Result<Vec<Vec<f32>>, SidecarError> {
    let mut vectors = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(EMBED_BATCH) {
        let items = chunk.iter().map(|t| (String::new(), t.clone())).collect();
        vectors.extend(sidecars.embed_documents(items).await?);
    }
    Ok(vectors)
}

/// FR-KN-DU-3: where each new block starts (add / replace / ignore), by meaning. The thresholds
/// are `merging::IGNORE_AT` / `REPLACE_AT`; a block whose numbers differ from its target's is never
/// *Ignore* (`merging::apply_number_rule`).
#[tauri::command]
#[specta::specta]
pub async fn match_merge_blocks(
    sidecars: State<'_, Sidecars>,
    input: MergeBlocksInput,
) -> ApiResult<MergeBlocksReport> {
    if input.existing.is_empty() || input.incoming.is_empty() {
        // Nothing to compare with: every new block is added.
        return Ok(MergeBlocksReport {
            matches: merging::match_blocks(&[], &vec![Vec::new(); input.incoming.len()]),
            by_meaning: true,
        });
    }
    let vectors = async {
        let existing = embed_blocks(&sidecars, &input.existing).await?;
        let incoming = embed_blocks(&sidecars, &input.incoming).await?;
        Ok::<_, SidecarError>((existing, incoming))
    };
    match vectors.await {
        Ok((existing, incoming)) => {
            let mut matches = merging::match_blocks(&existing, &incoming);
            merging::apply_number_rule(&input.existing, &input.incoming, &mut matches);
            Ok(MergeBlocksReport { matches, by_meaning: true })
        }
        Err(e) => {
            // FR-RAG-IX-6: the UI keeps its keyword overlap.
            tracing::info!(kind = %e.kind, "merge matching by meaning unavailable: {}", e.message);
            Ok(MergeBlocksReport { matches: Vec::new(), by_meaning: false })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupSuggestions {
    /// Pages without a suggestion are left out.
    pub proposals: Vec<GroupProposal>,
    pub new_groups: Vec<NewGroupName>,
}

/// FR-KN-GR-3: `suggest-groups` over the category's ungrouped pages, in batches; the user approves
/// each proposal in the UI (a new group is created with its first approval).
#[tauri::command]
#[specta::specta]
pub async fn suggest_groups(
    app: AppHandle,
    state: State<'_, AppState>,
    sidecars: State<'_, Sidecars>,
    jobs: State<'_, Arc<Jobs>>,
    category_id: String,
    job_id: Option<String>,
) -> ApiResult<GroupSuggestions> {
    let _ = (state, sidecars);
    jobs.run(job_id, async move {
        let state = app.state::<AppState>();
        let sidecars = app.state::<Sidecars>();
        let resolved = llm::resolve(&state, ModelRole::Default, None).await?;
        let pid = category_id.clone();
        let (category, groups, pages) = state
            .with_db(move |db| {
                let conn = db.conn();
                let category = categories::get(conn, &pid)?;
                // Nested groups (external mode) by their path, `Parent / Child` (D-72).
                let groups = knowledge::groups_with_paths(conn, &pid)?;
                let filter = PageFilter {
                    category_ids: Some(vec![pid]),
                    ungrouped: Some(true),
                    include_archived: Some(false),
                    ..PageFilter::default()
                };
                let mut pages = Vec::new();
                for summary in knowledge::list_pages(conn, &filter)? {
                    let page = knowledge::get_page(conn, &summary.id)?;
                    pages.push((page.id, page.title, page.body));
                }
                Ok((category, groups, pages))
            })
            .await?;
        let opts = CallOptions { stream_id: None, timeout_s: Some(120), max_tokens: None, effort: None, web_search: None };
        let mut proposals: Vec<GroupProposal> = Vec::new();
        let mut proposed: Vec<NewGroupName> = Vec::new();
        for chunk in pages.chunks(GROUP_BATCH) {
            // Short ids for this batch (`p1`, ...), mapped back after the answer.
            let batch: Vec<(String, String, String)> = chunk
                .iter()
                .enumerate()
                .map(|(i, (id, title, _))| (format!("p{}", i + 1), id.clone(), title.clone()))
                .collect();
            let lines: Vec<(String, String, String)> = chunk
                .iter()
                .zip(&batch)
                .map(|((_, title, body), (short, _, _))| (short.clone(), title.clone(), body.clone()))
                .collect();
            let mut group_lines: Vec<GroupLine> = groups
                .iter()
                .map(|g| GroupLine {
                    name: g.name.clone(),
                    description: g.description.clone(),
                    proposed: false,
                })
                .collect();
            group_lines.extend(proposed.iter().map(|g| GroupLine {
                name: g.name.clone(),
                description: g.description.clone(),
                proposed: true,
            }));
            let (pid, texts): (String, Vec<String>) = (
                category_id.clone(),
                lines.iter().flat_map(|(_, t, b)| [t.clone(), b.clone()]).collect(),
            );
            let entries = state
                .with_db(move |db| {
                    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
                    glossary::prompt_entries(db.conn(), &pid, &refs)
                })
                .await?;
            let messages = prompts::suggest_groups(
                &category.name,
                &group_lines,
                &lines,
                &prompts::glossary_lines(&entries),
            )?;
            let (reply, _) =
                llm::complete_json(&app, &sidecars, &resolved.target, &messages, &opts).await?;
            let (found, used) = merging::read_group_suggestions(&reply, &batch, &groups, &proposed);
            proposals.extend(found);
            for g in used {
                if !proposed.iter().any(|n| n.name.to_lowercase() == g.name.to_lowercase()) {
                    proposed.push(g);
                }
            }
        }
        // Only names some page uses (a name an earlier batch proposed stays once).
        let new_groups = proposed
            .into_iter()
            .filter(|g| {
                proposals.iter().any(|p| {
                    matches!(&p.group, GroupPick::New { name } if name.to_lowercase() == g.name.to_lowercase())
                })
            })
            .collect();
        Ok(GroupSuggestions { proposals, new_groups })
    })
    .await
}
