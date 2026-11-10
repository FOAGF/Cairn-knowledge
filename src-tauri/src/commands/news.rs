//! The tech news on Home (FR-HOME-7, D-92). `cairn_core::news` keeps what the last fetch found; a
//! fetch runs in the background on the web search model (D-91) and ends with
//! `DataChanged { news }`, which the card reads again.

use std::sync::atomic::{AtomicBool, Ordering};

use cairn_core::assistant::{find, formats};
use cairn_core::news::{self, NewsState, NewsStory};
use cairn_core::prompts::{self, ModelRole};
use cairn_core::settings::{self, Language};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};

use crate::error::{ApiError, ApiResult};
use crate::events::{Entity, emit_changed};
use crate::llm::{self, CallOptions};
use crate::sidecar::Sidecars;
use crate::state::AppState;

/// A web search with a few searches and page reads can take a while.
const NEWS_TIMEOUT_S: u32 = 300;

/// Set while a fetch runs, so a second visit to Home doesn't start another.
#[derive(Default)]
pub struct NewsFetch(AtomicBool);

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewsView {
    pub news: NewsState,
    pub fetching: bool,
}

fn view(state: &AppState, fetch: &NewsFetch) -> NewsView {
    NewsView {
        news: news::load_state(&state.paths),
        fetching: fetch.0.load(Ordering::SeqCst),
    }
}

/// What the card shows; fetches nothing.
#[tauri::command]
#[specta::specta]
pub async fn news(state: State<'_, AppState>, fetch: State<'_, NewsFetch>) -> ApiResult<NewsView> {
    Ok(view(&state, &fetch))
}

/// Starts a fetch when the news is on and one is due (the first today), or always with `force`
/// (*Refresh*, *Try again*). `language` is the interface language as the UI shows it (the
/// setting may be "system", which only the UI can resolve). Returns at once.
#[tauri::command]
#[specta::specta]
pub async fn fetch_news(
    app: AppHandle,
    state: State<'_, AppState>,
    fetch: State<'_, NewsFetch>,
    language: Language,
    force: bool,
) -> ApiResult<NewsView> {
    let settings = state.with_db(|db| settings::get(db.conn())).await?;
    let today = find::local_today();
    let mut current = news::load_state(&state.paths);
    if !settings.tech_news || !(force || news::due(&current, today)) {
        return Ok(view(&state, &fetch));
    }
    if fetch.0.swap(true, Ordering::SeqCst) {
        return Ok(view(&state, &fetch));
    }
    // Recorded first, so a failed fetch isn't repeated on every visit today.
    current.tried_on = Some(today.format("%Y-%m-%d").to_string());
    if let Err(e) = news::save_state(&state.paths, &current) {
        tracing::warn!(error = %e, "could not save news.json");
    }
    let topics = settings.news_topics.clone();
    let searches = settings.web_searches_per_message;
    let task_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run(&task_app, &topics, language, searches).await;
        let state = task_app.state::<AppState>();
        let mut saved = news::load_state(&state.paths);
        match result {
            Ok((stories, label)) => {
                saved.fetched_at = Some(chrono::Utc::now().to_rfc3339());
                saved.topics = topics;
                saved.model_label = Some(label);
                saved.stories = stories;
                saved.error = None;
            }
            Err(e) => {
                tracing::warn!(kind = %e.kind, "the news fetch failed");
                saved.error = Some(e.message);
            }
        }
        if let Err(e) = news::save_state(&state.paths, &saved) {
            tracing::warn!(error = %e, "could not save news.json");
        }
        task_app
            .state::<NewsFetch>()
            .0
            .store(false, Ordering::SeqCst);
        emit_changed(&task_app, &[Entity::News]);
    });
    emit_changed(&app, &[Entity::News]);
    Ok(view(&state, &fetch))
}

/// One fetch: the `news` template with web search on; only the topics and the date are sent. An
/// unreadable reply is asked again once without the web (it only needs reformatting). Returns
/// the stories and the label of the model that found them.
async fn run(
    app: &AppHandle,
    topics: &str,
    language: Language,
    searches: u32,
) -> ApiResult<(Vec<NewsStory>, String)> {
    let state = app.state::<AppState>();
    let sidecars = app.state::<Sidecars>();
    let resolved = llm::resolve(&state, ModelRole::Web, None).await?;
    let label = resolved.model.label.clone();
    let today = formats::today_line(find::local_today());
    let language = match language {
        Language::ZhCn => "Simplified Chinese",
        Language::En | Language::System => "English",
    };
    let messages = prompts::news(&today, topics, language)?;
    let mut opts = CallOptions::new(None, None, NEWS_TIMEOUT_S);
    opts.web_search = Some(searches);
    let reply = llm::complete(app, &sidecars, &resolved.target, &messages, &opts).await?;
    // FR-LLM-AS-12: a reply with no sign of a web search is never shown as found on the web.
    if !reply.web.as_ref().is_some_and(|w| w.searches > 0) {
        return Err(ApiError::invalid(format!(
            "{label} made no web search, so its answer isn't shown as news. Try again, or choose another web search model in Settings › Models & defaults."
        )));
    }
    if let Some(stories) = news::parse_stories(&reply.text) {
        return Ok((stories, label));
    }
    let again = llm::json_retry(&messages, &reply.text)?;
    let opts = CallOptions::new(None, None, NEWS_TIMEOUT_S);
    let second = llm::complete(app, &sidecars, &resolved.target, &again, &opts).await?;
    news::parse_stories(&second.text)
        .map(|stories| (stories, label))
        .ok_or_else(llm::unreadable)
}
