//! The list variables of the M6 templates, exactly as `crates/cairn-core/prompts/README.md`
//! ("How the app formats the list variables") specifies them. `poc/i-assistant/formats.py` is the
//! reference the owner's prompt check used, so the app sends what was checked.

use chrono::NaiveDate;
use serde::Serialize;

use crate::prompts::{LlmMessage, Role};
use crate::tasks::{Priority, TaskStatus, TaskType};

/// `out_of_scope_note` with `{names}` to fill.
const OUT_OF_SCOPE: &str = "Not searched, because outside this conversation's scope: {names}. Say so in the answer; the user can widen the search with \"Search all categories\".";

/// How many earlier turns `history` (ask-plan) holds, and where a long one is cut.
pub const PLAN_HISTORY_TURNS: usize = 6;
pub const PLAN_HISTORY_CHARS: usize = 600;
/// Where the start of a candidate's text is cut.
pub const CANDIDATE_CHARS: usize = 600;

/// The id the templates use for an enum value (`in_progress`, `wont_do`, `high`, `bug`).
pub fn enum_id<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

/// `today`: "2026-10-15 (Thursday)", in the computer's local time zone.
pub fn today_line(day: NaiveDate) -> String {
    format!("{} ({})", day.format("%Y-%m-%d"), day.format("%A"))
}

/// One category as the prompts list it.
#[derive(Debug, Clone, PartialEq)]
pub struct CategoryLine {
    pub name: String,
    pub key: String,
    pub in_scope: bool,
}

/// `categories`: `- Name (KEY): in scope` / `not in scope`, one line per category.
pub fn categories_lines(categories: &[CategoryLine]) -> String {
    categories
        .iter()
        .map(|p| {
            format!(
                "- {} ({}): {}",
                p.name,
                p.key,
                if p.in_scope { "in scope" } else { "not in scope" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `scope`: one sentence.
pub fn scope_sentence(categories: &[CategoryLine]) -> String {
    if categories.iter().all(|p| p.in_scope) {
        return "The conversation searches all categories.".to_string();
    }
    let names: Vec<&str> = categories
        .iter()
        .filter(|p| p.in_scope)
        .map(|p| p.name.as_str())
        .collect();
    format!(
        "The conversation searches only these categories: {}.",
        names.join(", ")
    )
}

/// Removes `[n]` / `[n, m]` markers from an earlier answer, with one space before each.
pub fn strip_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match crate::assistant::markers::marker_len(after) {
            Some(len) => {
                let before = &rest[..open];
                out.push_str(before.strip_suffix(' ').unwrap_or(before));
                rest = &after[len + 1..];
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One earlier turn of the conversation, as the history rules render it (README "What the app
/// appends"): a user turn is the message as typed; an assistant turn is its text, a find answer's
/// intro and rows, or a proposal's reply and results (`history::turns` builds these).
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}

fn cut_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{} [...]", cut.trim_end())
}

/// `history` for `ask-plan`: the last 6 turns as plain text, markers removed, each cut at about
/// 600 characters.
pub fn history_text(turns: &[Turn]) -> String {
    let start = turns.len().saturating_sub(PLAN_HISTORY_TURNS);
    turns[start..]
        .iter()
        .map(|t| {
            let text = cut_chars(strip_markers(&t.text).trim(), PLAN_HISTORY_CHARS);
            let who = if t.role == Role::User {
                "User"
            } else {
                "Assistant"
            };
            format!("{who}: {text}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The earlier turns as chat messages, between the system and the user message of `ask-answer`,
/// `ask-action`, `ask-page` and `edit-page`; an assistant turn without its markers.
pub fn history_messages(turns: &[Turn]) -> Vec<LlmMessage> {
    turns
        .iter()
        .map(|t| match t.role {
            Role::Assistant => LlmMessage::new(Role::Assistant, strip_markers(&t.text)),
            role => LlmMessage::new(role, t.text.clone()),
        })
        .collect()
}

/// A task as the prompts show it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskInfo {
    pub id: String,
    pub key: String,
    pub title: String,
    /// The category's name.
    pub category: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub task_type: TaskType,
    pub due: Option<String>,
    pub labels: Vec<String>,
    pub description: String,
    /// The parent task's key.
    pub parent: Option<String>,
    /// Keys of related tasks (and titles of linked pages).
    pub links: Vec<String>,
}

/// A page as the prompts show it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    pub id: String,
    pub title: String,
    /// The category's name.
    pub category: String,
    /// The group's name.
    pub group: Option<String>,
    /// Markdown, title line included.
    pub body: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemInfo {
    Task(TaskInfo),
    Page(PageInfo),
}

impl ItemInfo {
    pub fn id(&self) -> &str {
        match self {
            ItemInfo::Task(t) => &t.id,
            ItemInfo::Page(p) => &p.id,
        }
    }

    /// `Task KEY "Title" (Category; status: …; priority: …; due: …)` or
    /// `Page "Title" (Category; group: …)`.
    pub fn head(&self) -> String {
        match self {
            ItemInfo::Task(t) => format!(
                "Task {} \"{}\" ({}; status: {}; priority: {}; due: {})",
                t.key,
                t.title,
                t.category,
                enum_id(&t.status),
                enum_id(&t.priority),
                t.due.as_deref().unwrap_or("none")
            ),
            ItemInfo::Page(p) => format!(
                "Page \"{}\" ({}; group: {})",
                p.title,
                p.category,
                p.group.as_deref().unwrap_or("none")
            ),
        }
    }
}

/// `cited` for `ask-plan`: the items shown with the previous answer, in display order, `c1`, `c2`…
pub fn cited_lines(items: &[ItemInfo]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(i, it)| match it {
            ItemInfo::Task(t) => format!("- c{}: task {} \"{}\" ({})", i + 1, t.key, t.title, t.category),
            ItemInfo::Page(p) => format!("- c{}: page \"{}\" ({})", i + 1, p.title, p.category),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Which piece of an item (or of the attached file) a source is.
#[derive(Debug, Clone, PartialEq)]
pub enum PiecePart {
    /// A task's description (key, title and status are in the head).
    Description,
    /// A task comment, with its date (`YYYY-MM-DD`).
    Comment(String),
    /// A page section: its heading path, empty before the first heading.
    Section(Vec<String>),
    /// A part of the attached file (FR-LLM-AS-11), 1-based.
    File {
        name: String,
        index: u32,
        count: u32,
    },
}

/// One source: a piece of a task or page, or a part of the attached file.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcePiece {
    /// `None` for a file part.
    pub item: Option<ItemInfo>,
    pub part: PiecePart,
    /// The piece's own text (no glossary lines).
    pub text: String,
}

/// One numbered block of `sources`.
pub fn source_block(n: usize, piece: &SourcePiece) -> String {
    match (&piece.item, &piece.part) {
        (_, PiecePart::File { name, index, count }) => format!(
            "[{n}] File \"{name}\" (attached to this message; part {index} of {count})\n{}",
            piece.text
        ),
        (Some(item), part) => {
            let sub = match part {
                PiecePart::Description => "Description:".to_string(),
                PiecePart::Comment(date) => format!("Comment {date}:"),
                PiecePart::Section(path) if path.is_empty() => "Section: (top)".to_string(),
                PiecePart::Section(path) => format!("Section: {}", path.join(" > ")),
                PiecePart::File { .. } => unreachable!(),
            };
            format!("[{n}] {}\n{sub}\n{}", item.head(), piece.text)
        }
        (None, _) => format!("[{n}]\n{}", piece.text),
    }
}

/// `sources`: numbered from 1, blank line between blocks; `""` when there are none.
pub fn sources_block(pieces: &[SourcePiece]) -> String {
    pieces
        .iter()
        .enumerate()
        .map(|(i, p)| source_block(i + 1, p))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `out_of_scope_note`: `""`, or the fixed sentence with the category names.
pub fn out_of_scope_note(names: &[String]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        OUT_OF_SCOPE.replace("{names}", &names.join(", "))
    }
}

/// `refs_note`: the earlier items the message refers to, with their source numbers this turn.
pub fn refs_note(refs: &[(usize, &ItemInfo)]) -> String {
    if refs.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = refs
        .iter()
        .map(|(n, it)| match it {
            ItemInfo::Task(t) => format!("source {n} (task {})", t.key),
            ItemInfo::Page(p) => format!("source {n} (page \"{}\")", p.title),
        })
        .collect();
    format!(
        "The message refers to these earlier items: {}.",
        parts.join("; ")
    )
}

/// Whitespace collapsed, cut at `max` characters with ` [...]`.
fn start_of(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    cut_chars(&flat, max)
}

/// The body without its `# title` line.
fn without_title(body: &str) -> &str {
    match body.strip_prefix("# ") {
        Some(rest) => rest.split_once('\n').map_or("", |(_, b)| b),
        None => body,
    }
}

/// `candidates` for `ask-action`, and the labels in order: tasks `t1`, `t2`…, pages `p1`, `p2`…
/// The label list maps each label to the index of its item in `items`.
pub fn candidates_block(items: &[ItemInfo]) -> (String, Vec<(String, usize)>) {
    let (mut blocks, mut labels) = (Vec::new(), Vec::new());
    let (mut tasks, mut pages) = (0, 0);
    let none = |s: String| if s.is_empty() { "none".to_string() } else { s };
    for (i, it) in items.iter().enumerate() {
        let (label, block) = match it {
            ItemInfo::Task(t) => {
                tasks += 1;
                let label = format!("t{tasks}");
                let head = format!(
                    "{label}: Task {} \"{}\" ({}; status: {}; priority: {}; type: {}; due: {}; labels: {}; parent: {}; links: {})",
                    t.key,
                    t.title,
                    t.category,
                    enum_id(&t.status),
                    enum_id(&t.priority),
                    enum_id(&t.task_type),
                    t.due.as_deref().unwrap_or("none"),
                    none(t.labels.join(", ")),
                    t.parent.as_deref().unwrap_or("none"),
                    none(t.links.join(", ")),
                );
                let text = start_of(&t.description, CANDIDATE_CHARS);
                let text = if text.is_empty() {
                    "(no description)".to_string()
                } else {
                    text
                };
                (label, format!("{head}\n{text}"))
            }
            ItemInfo::Page(p) => {
                pages += 1;
                let label = format!("p{pages}");
                let head = format!(
                    "{label}: Page \"{}\" ({}; group: {})",
                    p.title,
                    p.category,
                    p.group.as_deref().unwrap_or("none")
                );
                (
                    label,
                    format!("{head}\n{}", start_of(without_title(&p.body), CANDIDATE_CHARS)),
                )
            }
        };
        labels.push((label, i));
        blocks.push(block);
    }
    (blocks.join("\n\n"), labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> ItemInfo {
        ItemInfo::Task(TaskInfo {
            id: "t-1".into(),
            key: "OPS-41".into(),
            title: "Auger hose wear".into(),
            category: "Site operations".into(),
            status: TaskStatus::InProgress,
            priority: Priority::High,
            task_type: TaskType::Bug,
            due: Some("2026-10-20".into()),
            labels: vec!["mmu".into(), "hose".into()],
            description: "The hose\nsplit  at the elbow.".into(),
            parent: None,
            links: vec!["OPS-47".into()],
        })
    }

    fn page() -> ItemInfo {
        ItemInfo::Page(PageInfo {
            id: "p-1".into(),
            title: "EP-40 calibration".into(),
            category: "Site operations".into(),
            group: None,
            body: "# EP-40 calibration\n\nEvery 250 hours.".into(),
        })
    }

    #[test]
    fn today_has_the_weekday() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        assert_eq!(today_line(d), "2026-10-15 (Thursday)");
    }

    #[test]
    fn categories_and_scope() {
        let mut ps = vec![
            CategoryLine { name: "Site operations".into(), key: "OPS".into(), in_scope: true },
            CategoryLine { name: "Home lab".into(), key: "LAB".into(), in_scope: true },
        ];
        assert_eq!(scope_sentence(&ps), "The conversation searches all categories.");
        ps[1].in_scope = false;
        assert_eq!(
            categories_lines(&ps),
            "- Site operations (OPS): in scope\n- Home lab (LAB): not in scope"
        );
        assert_eq!(
            scope_sentence(&ps),
            "The conversation searches only these categories: Site operations."
        );
    }

    #[test]
    fn markers_are_stripped_with_the_space_before() {
        assert_eq!(
            strip_markers("The K-factor is 0.982 [2]. Weigh it [1, 3] [4]."),
            "The K-factor is 0.982. Weigh it."
        );
        // Not markers: links, keys, empty brackets.
        assert_eq!(
            strip_markers("See [the page](x) and [OPS-1] and []."),
            "See [the page](x) and [OPS-1] and []."
        );
    }

    #[test]
    fn plan_history_keeps_the_last_six_turns_cut() {
        let mut turns: Vec<Turn> = (0..8)
            .map(|i| Turn {
                role: if i % 2 == 0 { Role::User } else { Role::Assistant },
                text: format!("turn {i} [1]"),
            })
            .collect();
        turns[7].text = "x".repeat(700);
        let h = history_text(&turns);
        assert!(h.starts_with("User: turn 2\n\nAssistant: turn 3"));
        assert!(h.ends_with(&format!("Assistant: {} [...]", "x".repeat(600))));
    }

    #[test]
    fn source_blocks() {
        let pieces = vec![
            SourcePiece {
                item: Some(task()),
                part: PiecePart::Comment("2026-09-10".into()),
                text: "Moved the clamp.".into(),
            },
            SourcePiece {
                item: Some(page()),
                part: PiecePart::Section(vec!["Steps".into(), "Weigh".into()]),
                text: "Within 2%.".into(),
            },
            SourcePiece {
                item: Some(page()),
                part: PiecePart::Section(vec![]),
                text: "Every 250 hours.".into(),
            },
            SourcePiece {
                item: None,
                part: PiecePart::File { name: "torque.pdf".into(), index: 2, count: 5 },
                text: "210 Nm.".into(),
            },
        ];
        assert_eq!(
            sources_block(&pieces),
            "[1] Task OPS-41 \"Auger hose wear\" (Site operations; status: in_progress; priority: high; due: 2026-10-20)\nComment 2026-09-10:\nMoved the clamp.\n\n\
             [2] Page \"EP-40 calibration\" (Site operations; group: none)\nSection: Steps > Weigh\nWithin 2%.\n\n\
             [3] Page \"EP-40 calibration\" (Site operations; group: none)\nSection: (top)\nEvery 250 hours.\n\n\
             [4] File \"torque.pdf\" (attached to this message; part 2 of 5)\n210 Nm."
        );
    }

    #[test]
    fn notes() {
        assert_eq!(out_of_scope_note(&[]), "");
        assert_eq!(
            out_of_scope_note(&["Home lab".into()]),
            "Not searched, because outside this conversation's scope: Home lab. Say so in the answer; the user can widen the search with \"Search all categories\"."
        );
        let (t, p) = (task(), page());
        assert_eq!(
            refs_note(&[(2, &p), (5, &t)]),
            "The message refers to these earlier items: source 2 (page \"EP-40 calibration\"); source 5 (task OPS-41)."
        );
        assert_eq!(
            cited_lines(&[t, p]),
            "- c1: task OPS-41 \"Auger hose wear\" (Site operations)\n- c2: page \"EP-40 calibration\" (Site operations)"
        );
    }

    #[test]
    fn candidates_are_labelled_by_type() {
        let (text, labels) = candidates_block(&[page(), task()]);
        assert_eq!(labels, vec![("p1".to_string(), 0), ("t1".to_string(), 1)]);
        assert_eq!(
            text,
            "p1: Page \"EP-40 calibration\" (Site operations; group: none)\nEvery 250 hours.\n\n\
             t1: Task OPS-41 \"Auger hose wear\" (Site operations; status: in_progress; priority: high; type: bug; due: 2026-10-20; labels: mmu, hose; parent: none; links: OPS-47)\nThe hose split at the elbow."
        );
    }
}
