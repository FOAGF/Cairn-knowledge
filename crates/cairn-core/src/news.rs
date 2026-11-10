//! The tech news on Home (FR-HOME-7, D-92): what the last fetch found, kept in `news.json` in the
//! data folder (like `backup-state.json`), so it is never in a backup or an export.
//!
//! A fetch is one call to the web search model with the `news` template; only the topics and the
//! date are sent. It runs at most once a day by itself (the first visit to Home that day), and
//! again on *Refresh*. The Tauri layer (`commands::news`) makes the call; this module reads the
//! reply and keeps the state.

use std::path::PathBuf;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths::DataPaths;
use crate::{Result, replies};

const STATE_FILE: &str = "news.json";

/// Most stories shown (FR-HOME-7).
pub const MAX_STORIES: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NewsStory {
    pub title: String,
    /// One sentence.
    pub summary: String,
    /// The site or publication.
    pub source: String,
    /// `YYYY-MM-DD`, or empty when the page gave none.
    pub date: String,
    /// An `http(s)` address, opened in the system browser.
    pub url: String,
}

/// What the card shows. A missing or damaged file is "nothing fetched yet".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NewsState {
    /// The last fetch that worked (UTC, RFC 3339).
    pub fetched_at: Option<String>,
    /// The local day of the last try, worked or not: the daily fetch runs once a day.
    pub tried_on: Option<String>,
    /// The topics and the model of the stories shown.
    pub topics: String,
    pub model_label: Option<String>,
    pub stories: Vec<NewsStory>,
    /// Why the last try failed; cleared by the next fetch that works. The stories stay.
    pub error: Option<String>,
}

fn state_path(paths: &DataPaths) -> PathBuf {
    paths.root.join(STATE_FILE)
}

pub fn load_state(paths: &DataPaths) -> NewsState {
    std::fs::read_to_string(state_path(paths))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_state(paths: &DataPaths, state: &NewsState) -> Result<()> {
    let file = state_path(paths);
    let part = file.with_extension("json.part");
    std::fs::write(&part, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(part, file)?;
    Ok(())
}

/// The daily fetch is due: nothing was tried on `today` yet.
pub fn due(state: &NewsState, today: NaiveDate) -> bool {
    state.tried_on.as_deref() != Some(today.format("%Y-%m-%d").to_string().as_str())
}

/// The stories of a `news` reply (prompts README "JSON shapes"): `None` when the reply isn't a
/// JSON object with a `stories` list. A story needs a title and an `http(s)` address; the same
/// address counts once; at most [`MAX_STORIES`], in the reply's order.
pub fn parse_stories(reply: &str) -> Option<Vec<NewsStory>> {
    let object = replies::json_object(reply)?;
    let list = object.get("stories")?.as_array()?;
    let text = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
    };
    let mut stories: Vec<NewsStory> = Vec::new();
    for v in list {
        let url = text(v, "url");
        let web = url.starts_with("https://") || url.starts_with("http://");
        let story = NewsStory {
            title: text(v, "title"),
            summary: text(v, "summary"),
            source: text(v, "source"),
            date: Some(text(v, "date"))
                .filter(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
                .unwrap_or_default(),
            url,
        };
        if story.title.is_empty() || !web || stories.iter().any(|s| s.url == story.url) {
            continue;
        }
        stories.push(story);
        if stories.len() == MAX_STORIES {
            break;
        }
    }
    Some(stories)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stories_need_a_title_and_a_web_address() {
        let reply = r#"```json
{"stories": [
  {"title": "Rust 2.0", "summary": "It  ships.", "source": "Blog", "date": "2026-11-09", "url": "https://blog.rust-lang.org/x"},
  {"title": "", "summary": "no title", "url": "https://a.example"},
  {"title": "Bad link", "url": "javascript:alert(1)"},
  {"title": "Again", "url": "https://blog.rust-lang.org/x"},
  {"title": "Odd date", "date": "yesterday", "url": "http://b.example"}
]}
```"#;
        let stories = parse_stories(reply).unwrap();
        assert_eq!(stories.len(), 2);
        assert_eq!(stories[0].summary, "It ships.");
        assert_eq!(stories[0].date, "2026-11-09");
        assert_eq!(
            (stories[1].title.as_str(), stories[1].date.as_str()),
            ("Odd date", "")
        );
        assert_eq!(parse_stories("Here is the news: none"), None);
        assert_eq!(parse_stories(r#"{"stories": []}"#), Some(vec![]));
    }

    #[test]
    fn at_most_eight_stories() {
        let list: Vec<String> = (0..12)
            .map(|i| format!(r#"{{"title": "S{i}", "url": "https://n.example/{i}"}}"#))
            .collect();
        let stories = parse_stories(&format!(r#"{{"stories": [{}]}}"#, list.join(","))).unwrap();
        assert_eq!(stories.len(), MAX_STORIES);
        assert_eq!(stories[7].title, "S7");
    }

    #[test]
    fn the_state_survives_a_damaged_file_and_the_fetch_is_daily() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path().to_path_buf());
        assert_eq!(load_state(&paths), NewsState::default());
        std::fs::write(dir.path().join(STATE_FILE), "{oops").unwrap();
        assert_eq!(load_state(&paths), NewsState::default());
        let day = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let mut state = NewsState::default();
        assert!(due(&state, day));
        state.tried_on = Some("2026-11-10".into());
        state.stories.push(NewsStory {
            title: "T".into(),
            url: "https://x.example".into(),
            ..NewsStory::default()
        });
        save_state(&paths, &state).unwrap();
        let back = load_state(&paths);
        assert_eq!(back, state);
        assert!(!due(&back, day));
        assert!(due(&back, day.succ_opt().unwrap()));
    }
}
