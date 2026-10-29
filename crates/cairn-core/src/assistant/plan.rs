//! The first step of every assistant message (`ask-plan`, D-44): the model's JSON read into a
//! [`Plan`] with the README's safe fallbacks ("Local checks › ask-plan";
//! `poc/i-assistant/formats.py` `check_plan` is the reference).
//!
//! Owned by the M6 answers agent.

use chrono::NaiveDate;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::search::SearchKind;
use crate::tasks::{Priority, TaskStatus};

/// The kind of request (README "Which M6 template runs when").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Answered from the notes (`ask-answer`).
    Question,
    /// A list Rust makes from the filters; the plan's `intro` heads it (FR-LLM-AS-2).
    Find,
    /// `ask-action` (FR-LLM-AS-3).
    Create,
    Change,
    /// General knowledge: no search, labelled (FR-LLM-AS-9).
    General,
    /// A question about the attached file (FR-LLM-AS-11).
    FileQuestion,
    /// The attached file becomes a draft page (the M5 file flow).
    FileDraft,
}

/// A date range; either end may be open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateRange {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanFilters {
    /// In-scope categories the message named; empty = the conversation's scope.
    pub category_ids: Vec<String>,
    /// `Page` / `Task`; empty = both.
    pub kinds: Vec<SearchKind>,
    pub statuses: Vec<TaskStatus>,
    pub priorities: Vec<Priority>,
    pub due: Option<DateRange>,
    pub updated: Option<DateRange>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub kind: RequestKind,
    /// Focused search words; `""` = no search (a general question, a draft, or only references).
    pub search: String,
    pub filters: PlanFilters,
    /// Indices (0-based) into the items shown with the previous answer (`c1` is 0).
    pub refs: Vec<usize>,
    /// Names of categories the message named that are outside the scope (FR-LLM-AS-4).
    pub outside_scope: Vec<String>,
    /// A find's intro line; `""` otherwise.
    pub intro: String,
}

/// A category as the plan sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanCategory {
    pub id: String,
    pub name: String,
    pub in_scope: bool,
}

/// Reads the model's JSON object with the fallbacks of `check_plan`:
/// - an unknown `kind` is a question; a file kind without a file is a question; with a file, any
///   kind but the two file kinds becomes `FileQuestion` (the file stays a source);
/// - category names match case-insensitively; unknown ones are dropped; a named category that is
///   not in scope moves to `outside_scope`;
/// - enum lists keep only known ids; dates that don't parse are dropped, and a reversed range is
///   swapped;
/// - `refs` keep only `cN` labels with `1 <= N <= shown`;
/// - `intro` only for a find.
///
/// Returns the plan and notes on what was repaired (logged at debug level).
pub fn read(
    reply: &Map<String, Value>,
    categories: &[PlanCategory],
    has_attachment: bool,
    shown: usize,
) -> (Plan, Vec<String>) {
    let mut notes = Vec::new();
    let given = reply.get("kind").and_then(Value::as_str).unwrap_or("");
    let mut kind = match given {
        "question" => RequestKind::Question,
        "find" => RequestKind::Find,
        "create" => RequestKind::Create,
        "change" => RequestKind::Change,
        "general" => RequestKind::General,
        "file_question" => RequestKind::FileQuestion,
        "file_draft" => RequestKind::FileDraft,
        other => {
            notes.push(format!("kind {other:?} -> question"));
            RequestKind::Question
        }
    };
    let file_kind = matches!(kind, RequestKind::FileQuestion | RequestKind::FileDraft);
    if file_kind && !has_attachment {
        notes.push(format!("{given} without a file -> question"));
        kind = RequestKind::Question;
    } else if has_attachment && !file_kind {
        notes.push(format!("{given} with a file -> file_question"));
        kind = RequestKind::FileQuestion;
    }

    let search = reply
        .get("search")
        .and_then(Value::as_str)
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();

    let filters = reply.get("filters").and_then(Value::as_object);
    let by_name = |name: &str| {
        let lower = name.to_lowercase();
        categories.iter().find(|p| p.name.to_lowercase() == lower)
    };
    let names_in = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|n| match n {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect()
    };
    let mut category_ids: Vec<String> = Vec::new();
    let mut outside: Vec<String> = Vec::new();
    let named = names_in(filters.and_then(|f| f.get("categories")));
    for n in &named {
        match by_name(n) {
            None => notes.push(format!("unknown category {n:?} dropped")),
            Some(p) if !p.in_scope => {
                notes.push(format!("category {:?} is not in scope -> outside_scope", p.name));
            }
            Some(p) => {
                if !category_ids.contains(&p.id) {
                    category_ids.push(p.id.clone());
                }
            }
        }
    }
    for n in names_in(reply.get("outside_scope")) {
        match by_name(&n) {
            Some(p) if !p.in_scope => {
                if !outside.contains(&p.name) {
                    outside.push(p.name.clone());
                }
            }
            Some(_) => {}
            None => notes.push(format!("unknown outside_scope category {n:?} dropped")),
        }
    }
    for n in &named {
        if let Some(p) = by_name(n)
            && !p.in_scope
            && !outside.contains(&p.name)
        {
            outside.push(p.name.clone());
        }
    }

    let list = |key: &str| filters.and_then(|f| f.get(key));
    let mut kinds: Vec<SearchKind> = Vec::new();
    for k in enum_list::<String>(list("types")) {
        let kind = match k.as_str() {
            "task" => SearchKind::Task,
            "page" => SearchKind::Page,
            _ => continue,
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    let plan_filters = PlanFilters {
        category_ids,
        kinds,
        statuses: enum_list::<TaskStatus>(list("statuses")),
        priorities: enum_list::<Priority>(list("priorities")),
        due: range(list("due")),
        updated: range(list("updated")),
    };

    let mut refs: Vec<usize> = Vec::new();
    for r in reply
        .get("refs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(label) = r.as_str() else { continue };
        let n = label
            .strip_prefix('c')
            .filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|d| d.parse::<usize>().ok());
        match n {
            Some(n) if (1..=shown).contains(&n) => {
                if !refs.contains(&(n - 1)) {
                    refs.push(n - 1);
                }
            }
            _ => notes.push(format!("ref {label:?} dropped")),
        }
    }

    let intro = match (kind, reply.get("intro").and_then(Value::as_str)) {
        (RequestKind::Find, Some(s)) => s.trim().to_string(),
        _ => String::new(),
    };
    let plan = Plan {
        kind,
        search,
        filters: plan_filters,
        refs,
        outside_scope: outside,
        intro,
    };
    (plan, notes)
}

/// The values of a JSON list that are known ids of `T`, once each.
fn enum_list<T: DeserializeOwned + PartialEq>(v: Option<&Value>) -> Vec<T> {
    let mut out: Vec<T> = Vec::new();
    for item in v.and_then(Value::as_array).into_iter().flatten() {
        if !item.is_string() {
            continue;
        }
        if let Ok(x) = serde_json::from_value::<T>(item.clone())
            && !out.contains(&x)
        {
            out.push(x);
        }
    }
    out
}

fn parse_date(v: Option<&Value>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v?.as_str()?.trim(), "%Y-%m-%d").ok()
}

/// `{"from": …, "to": …}` with each end read as `YYYY-MM-DD` or dropped; `None` without a valid
/// end; a reversed range is swapped.
fn range(v: Option<&Value>) -> Option<DateRange> {
    let o = v?.as_object()?;
    let (mut from, mut to) = (parse_date(o.get("from")), parse_date(o.get("to")));
    if let (Some(a), Some(b)) = (from, to)
        && a > b
    {
        (from, to) = (Some(b), Some(a));
    }
    (from.is_some() || to.is_some()).then_some(DateRange { from, to })
}

/// The plan when the reply couldn't be read even after `check-retry`: a question searched with
/// the message's own words (the file kinds stay a file question when a file is attached).
pub fn fallback(message: &str, has_attachment: bool) -> Plan {
    Plan {
        kind: if has_attachment {
            RequestKind::FileQuestion
        } else {
            RequestKind::Question
        },
        search: message.split_whitespace().collect::<Vec<_>>().join(" "),
        filters: PlanFilters::default(),
        refs: Vec::new(),
        outside_scope: Vec::new(),
        intro: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn categories() -> Vec<PlanCategory> {
        let p = |id: &str, name: &str, in_scope| PlanCategory {
            id: id.into(),
            name: name.into(),
            in_scope,
        };
        vec![
            p("p1", "Site operations", true),
            p("p2", "Home lab", false),
            p("p3", "Fleet", true),
        ]
    }

    fn read_json(v: Value, file: bool, shown: usize) -> (Plan, Vec<String>) {
        read(v.as_object().unwrap(), &categories(), file, shown)
    }

    #[test]
    fn a_full_plan_is_read() {
        let (plan, notes) = read_json(
            json!({
                "kind": "find", "search": "  hose   wear ",
                "filters": {"categories": ["site OPERATIONS", "Nowhere", "Home lab"],
                            "types": ["task", "page", "bogus", "task"],
                            "statuses": ["todo", "later", "blocked"], "priorities": ["high", 3],
                            "due": {"from": "2026-10-20", "to": "2026-10-12"},
                            "updated": {"from": "yesterday", "to": null}},
                "refs": ["c2", "c9", "x1", "c0", "c2"], "outside_scope": ["Fleet", "home lab", "Mars"],
                "intro": " Open hose tasks: "
            }),
            false,
            3,
        );
        assert_eq!(plan.kind, RequestKind::Find);
        assert_eq!(plan.search, "hose wear");
        assert_eq!(plan.filters.category_ids, vec!["p1"]);
        assert_eq!(plan.filters.kinds, vec![SearchKind::Task, SearchKind::Page]);
        assert_eq!(plan.filters.statuses, vec![TaskStatus::Todo, TaskStatus::Blocked]);
        assert_eq!(plan.filters.priorities, vec![Priority::High]);
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            plan.filters.due,
            Some(DateRange { from: Some(d("2026-10-12")), to: Some(d("2026-10-20")) })
        );
        assert_eq!(plan.filters.updated, None);
        assert_eq!(plan.refs, vec![1]);
        // Fleet is in scope, so it is not outside; Home lab once.
        assert_eq!(plan.outside_scope, vec!["Home lab"]);
        assert_eq!(plan.intro, "Open hose tasks:");
        assert!(notes.iter().any(|n| n.contains("Nowhere")));
        assert!(notes.iter().any(|n| n.contains("Mars")));
    }

    #[test]
    fn kinds_are_repaired() {
        let kind = |v: Value, file| read_json(v, file, 0).0.kind;
        assert_eq!(kind(json!({"kind": "poem"}), false), RequestKind::Question);
        assert_eq!(kind(json!({}), false), RequestKind::Question);
        assert_eq!(kind(json!({"kind": "file_draft"}), false), RequestKind::Question);
        assert_eq!(kind(json!({"kind": "file_question"}), false), RequestKind::Question);
        assert_eq!(kind(json!({"kind": "create"}), true), RequestKind::FileQuestion);
        assert_eq!(kind(json!({"kind": "file_draft"}), true), RequestKind::FileDraft);
        assert_eq!(kind(json!({"kind": "general"}), false), RequestKind::General);
        // An intro is only for a find.
        assert_eq!(read_json(json!({"kind": "question", "intro": "x"}), false, 0).0.intro, "");
        // Missing or wrong-typed fields read as empty.
        let (plan, _) = read_json(json!({"search": 4, "filters": [], "refs": "c1"}), false, 2);
        assert_eq!(plan.search, "");
        assert_eq!(plan.filters, PlanFilters::default());
        assert!(plan.refs.is_empty());
    }

    #[test]
    fn the_fallback_searches_the_message() {
        let p = fallback("  what is   the K-factor ", false);
        assert_eq!((p.kind, p.search.as_str()), (RequestKind::Question, "what is the K-factor"));
        assert_eq!(fallback("x", true).kind, RequestKind::FileQuestion);
    }
}
