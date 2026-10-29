//! Create and change requests (FR-LLM-AS-3, D-44): the `ask-action` JSON read into a proposal,
//! and applying the rows the user confirmed. Nothing is ever deleted through the chat: the shape
//! has no delete.
//!
//! Reading (README "Local checks › ask-action"; `poc/i-assistant/formats.py` `check_action` is the
//! reference): unknown ops, unknown target labels and targets of the wrong type are dropped;
//! values outside the enums are dropped (a create falls back to `general` / `medium`); an unknown
//! category falls back to the first in-scope category (changeable on the card); a change that changes
//! nothing, or repeats another, is dropped; dates parse or drop; links keep only known labels of
//! the right type and never point at the task itself. `create_page` and `update_page` come back
//! as page requests: the Tauri layer writes them with `ask-page` (into the message's draft card)
//! and `edit-page` (into an [`ProposedChange::UpdatePage`] row with the diff).
//!
//! Applying: one row at a time, each in its own transaction, through `tasks` and `knowledge` (so
//! history, activity, the index hooks and events work as for any edit). An `UpdatePage` whose page
//! changed since the proposal (`base_updated_at`) fails with a message asking to try again.
//!
//! Owned by the M6 actions agent.

use chrono::NaiveDate;
use rusqlite::Connection;
use serde_json::{Map, Value};

use crate::assistant::formats::{ItemInfo, TaskInfo, enum_id};
use crate::assistant::parts::{LinkKind, LinkTo, ProposalRow, ProposedChange, RowState};
use crate::assistant::plan::PlanCategory;
use crate::checks::RawTerm;
use crate::knowledge::PagePatch;
use crate::tasks::{NewTask, Priority, TaskLinkType, TaskPatch, TaskStatus, TaskType};
use crate::{CoreError, Db, Result, knowledge, categories, tasks};

/// A page the model asked to create; `ask-page` writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRequest {
    pub category_id: String,
    pub title: String,
    pub content: String,
}

/// A page the model asked to change; `edit-page` writes it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageEditRequest {
    pub page_id: String,
    pub change: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadAction {
    /// Task changes and comments, ready for rows (`UpdatePage` rows are added by the caller).
    pub changes: Vec<ProposedChange>,
    /// At most one: a message holds one draft card.
    pub new_page: Option<PageRequest>,
    pub page_edits: Vec<PageEditRequest>,
    pub reply: String,
    /// `create_task` terms as the model gave them, by the index of their change in `changes`; the
    /// caller runs `checks::filter_terms` against that task's text.
    pub terms: Vec<(usize, Vec<RawTerm>)>,
    /// What was dropped or repaired (debug log).
    pub notes: Vec<String>,
    /// How many changes the model proposed, before any was dropped (when all were dropped, its
    /// reply describes changes that won't happen).
    pub proposed: usize,
}

fn parse_date(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .map(|d| d.format("%Y-%m-%d").to_string())
}

fn status_of(s: &str) -> Option<TaskStatus> {
    Some(match s {
        "todo" => TaskStatus::Todo,
        "in_progress" => TaskStatus::InProgress,
        "blocked" => TaskStatus::Blocked,
        "done" => TaskStatus::Done,
        "wont_do" => TaskStatus::WontDo,
        _ => return None,
    })
}

fn priority_of(s: &str) -> Option<Priority> {
    Some(match s {
        "high" => Priority::High,
        "medium" => Priority::Medium,
        "low" => Priority::Low,
        _ => return None,
    })
}

fn type_of(s: &str) -> Option<TaskType> {
    Some(match s {
        "bug" => TaskType::Bug,
        "feature" => TaskType::Feature,
        "general" => TaskType::General,
        _ => return None,
    })
}

fn text(c: &Map<String, Value>, key: &str) -> String {
    c.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn str_field<'a>(c: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    c.get(key).and_then(Value::as_str)
}

fn labels_of(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

fn same_set(a: &[String], b: &[String]) -> bool {
    let norm = |v: &[String]| {
        let mut v: Vec<String> = v.iter().map(|s| s.to_lowercase()).collect();
        v.sort();
        v.dedup();
        v
    };
    norm(a) == norm(b)
}

fn raw_terms(v: Option<&Value>) -> Vec<RawTerm> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|t| {
            let term = text(t, "term");
            (!term.is_empty()).then(|| RawTerm {
                term,
                meaning: text(t, "meaning"),
                confidence: text(t, "confidence"),
            })
        })
        .collect()
}

/// The valid links of a change: known labels, a kind that fits the target's type, never the task
/// itself (`own` is the label of the task being changed).
fn links_of(c: &Map<String, Value>, labels: &[(String, ItemInfo)], own: Option<&str>) -> Vec<LinkTo> {
    let mut out: Vec<LinkTo> = Vec::new();
    for l in c.get("links").and_then(Value::as_array).into_iter().flatten() {
        let Some(l) = l.as_object() else { continue };
        let to = text(l, "to");
        if own == Some(to.as_str()) {
            continue;
        }
        let Some((_, item)) = labels.iter().find(|(label, _)| *label == to) else {
            continue;
        };
        let kind = match str_field(l, "kind") {
            Some("relates") => LinkKind::Relates,
            Some("parent") => LinkKind::Parent,
            Some("page") => LinkKind::Page,
            _ => continue,
        };
        let (target_id, target_name) = match (item, kind) {
            (ItemInfo::Page(p), LinkKind::Page) => (p.id.clone(), p.title.clone()),
            (ItemInfo::Task(t), LinkKind::Relates | LinkKind::Parent) => (t.id.clone(), t.key.clone()),
            _ => continue,
        };
        if !out.iter().any(|o| o.kind == kind && o.target_id == target_id) {
            out.push(LinkTo { kind, target_id, target_name });
        }
    }
    out
}

/// Whether the task already has this link (`TaskInfo` holds the parent's key and the keys and
/// page titles of the other links).
fn has_link(t: &TaskInfo, l: &LinkTo) -> bool {
    match l.kind {
        LinkKind::Parent => t.parent.as_deref() == Some(l.target_name.as_str()),
        LinkKind::Relates | LinkKind::Page => t.links.contains(&l.target_name),
    }
}

/// `labels` maps the candidate labels (`t1`, `p1`, …, from `formats::candidates_block`) to the
/// items.
pub fn read(
    reply: &Map<String, Value>,
    labels: &[(String, ItemInfo)],
    categories: &[PlanCategory],
) -> ReadAction {
    let mut out = ReadAction {
        reply: text(reply, "reply"),
        ..ReadAction::default()
    };
    let fallback = categories
        .iter()
        .find(|p| p.in_scope)
        .or(categories.first())
        .map(|p| p.id.clone())
        .unwrap_or_default();
    let mut seen: Vec<(String, Vec<&'static str>)> = Vec::new();
    let empty = Vec::new();
    let changes = reply.get("changes").and_then(Value::as_array).unwrap_or(&empty);
    out.proposed = changes.len();
    for c in changes {
        let Some(c) = c.as_object() else {
            out.notes.push("a change that is not an object".into());
            continue;
        };
        let op = str_field(c, "op").unwrap_or_default();
        if !matches!(op, "create_task" | "update_task" | "comment" | "create_page" | "update_page") {
            out.notes.push(format!("unknown op {op:?}"));
            continue;
        }
        let target = text(c, "target");
        let found = labels.iter().find(|(label, _)| *label == target);
        if matches!(op, "update_task" | "comment" | "update_page") {
            let Some((_, item)) = found else {
                out.notes.push(format!("unknown target {target:?}"));
                continue;
            };
            if (op == "update_page") != matches!(item, ItemInfo::Page(_)) {
                out.notes.push(format!("target {target} is the wrong type for {op}"));
                continue;
            }
        }
        let mut category_id = String::new();
        let mut title = String::new();
        if matches!(op, "create_task" | "create_page") {
            let name = text(c, "category").to_lowercase();
            category_id = match categories.iter().find(|p| p.name.to_lowercase() == name) {
                Some(p) => p.id.clone(),
                None => {
                    out.notes.push(format!("category {name:?} unknown, used the first in scope"));
                    fallback.clone()
                }
            };
            title = text(c, "title");
            if title.is_empty() {
                out.notes.push(format!("{op} without a title"));
                continue;
            }
        }
        match (op, found) {
            ("create_task", _) => {
                let links = links_of(c, labels, None);
                let terms = raw_terms(c.get("terms"));
                if !terms.is_empty() {
                    out.terms.push((out.changes.len(), terms));
                }
                out.changes.push(ProposedChange::CreateTask {
                    category_id,
                    title,
                    task_type: str_field(c, "type").and_then(type_of).unwrap_or(TaskType::General),
                    priority: str_field(c, "priority")
                        .and_then(priority_of)
                        .unwrap_or(Priority::Medium),
                    due: parse_date(c.get("due")),
                    labels: labels_of(c.get("labels")),
                    description: text(c, "description"),
                    links,
                });
            }
            ("update_task", Some((_, ItemInfo::Task(t)))) => {
                let status = str_field(c, "status").and_then(status_of).filter(|s| *s != t.status);
                let priority = str_field(c, "priority")
                    .and_then(priority_of)
                    .filter(|p| *p != t.priority);
                let task_type = str_field(c, "type").and_then(type_of).filter(|y| *y != t.task_type);
                let (mut due, mut clear_due) = (None, false);
                if let Some(v) = c.get("due") {
                    match parse_date(Some(v)) {
                        Some(d) if t.due.as_deref() != Some(d.as_str()) => due = Some(d),
                        None if v.is_null() && t.due.is_some() => clear_due = true,
                        _ => {}
                    }
                }
                let new_labels = c
                    .get("labels")
                    .filter(|v| v.is_array())
                    .map(|v| labels_of(Some(v)))
                    .filter(|l| !same_set(l, &t.labels));
                let description = str_field(c, "description")
                    .filter(|d| d.trim() != t.description.trim())
                    .map(|d| d.trim().to_string());
                let links: Vec<LinkTo> = links_of(c, labels, Some(&target))
                    .into_iter()
                    .filter(|l| !has_link(t, l))
                    .collect();
                let fields: Vec<&'static str> = [
                    ("status", status.is_some()),
                    ("priority", priority.is_some()),
                    ("type", task_type.is_some()),
                    ("due", due.is_some() || clear_due),
                    ("labels", new_labels.is_some()),
                    ("description", description.is_some()),
                    ("links", !links.is_empty()),
                ]
                .into_iter()
                .filter_map(|(name, on)| on.then_some(name))
                .collect();
                if fields.is_empty() {
                    out.notes.push(format!("update of {target} changes nothing"));
                    continue;
                }
                if seen.iter().any(|(t, f)| *t == target && *f == fields) {
                    out.notes.push(format!("duplicate update of {target}"));
                    continue;
                }
                seen.push((target.clone(), fields));
                out.changes.push(ProposedChange::UpdateTask {
                    task_id: t.id.clone(),
                    key: t.key.clone(),
                    title: t.title.clone(),
                    status,
                    priority,
                    task_type,
                    due,
                    clear_due,
                    labels: new_labels,
                    description,
                    links,
                });
            }
            ("comment", Some((_, ItemInfo::Task(t)))) => {
                let comment = text(c, "text");
                if comment.is_empty() {
                    out.notes.push("empty comment".into());
                    continue;
                }
                out.changes.push(ProposedChange::Comment {
                    task_id: t.id.clone(),
                    key: t.key.clone(),
                    text: comment,
                });
            }
            ("create_page", _) => {
                if out.new_page.is_some() {
                    out.notes.push("a second new page (a message holds one draft card)".into());
                    continue;
                }
                out.new_page = Some(PageRequest { category_id, title, content: text(c, "content") });
            }
            ("update_page", Some((_, ItemInfo::Page(p)))) => {
                let change = text(c, "change");
                if change.is_empty() {
                    out.notes.push("empty page change".into());
                    continue;
                }
                if out.page_edits.iter().any(|e| e.page_id == p.id && e.change == change) {
                    out.notes.push(format!("duplicate change of {target}"));
                    continue;
                }
                out.page_edits.push(PageEditRequest { page_id: p.id.clone(), change });
            }
            _ => {}
        }
    }
    out
}

fn status_phrase(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Todo => "to do",
        TaskStatus::InProgress => "in progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Done => "done",
        TaskStatus::WontDo => "won't do",
    }
}

/// Adds the links; the ones that fail (already there, a second parent) are reported, not fatal.
fn add_links(db: &mut Db, task_id: &str, links: &[LinkTo]) -> Vec<String> {
    let mut failed = Vec::new();
    for l in links {
        let added = match l.kind {
            LinkKind::Relates => {
                tasks::add_link(db, TaskLinkType::Relates, task_id, &l.target_id).map(|_| ())
            }
            // The target becomes the parent of this task.
            LinkKind::Parent => {
                tasks::add_link(db, TaskLinkType::Parent, &l.target_id, task_id).map(|_| ())
            }
            LinkKind::Page => knowledge::link_task_page(db, task_id, &l.target_id).map(|_| ()),
        };
        if let Err(e) = added {
            failed.push(format!("{}: {e}", l.target_name));
        }
    }
    failed
}

fn with_failed_links(result: String, failed: Vec<String>) -> String {
    if failed.is_empty() {
        result
    } else {
        format!("{result}; could not add a link ({})", failed.join(", "))
    }
}

/// Applies one confirmed change; returns the result line ("Created OPS-48") and the id of the
/// created or changed item.
pub fn apply(db: &mut Db, change: &ProposedChange) -> Result<(String, Option<String>)> {
    match change {
        ProposedChange::CreateTask {
            category_id,
            title,
            task_type,
            priority,
            due,
            labels,
            description,
            links,
        } => {
            let t = tasks::create(
                db,
                NewTask {
                    category_id: category_id.clone(),
                    title: title.clone(),
                    task_type: Some(*task_type),
                    description: Some(description.clone()),
                    status: None,
                    priority: Some(*priority),
                    labels: Some(labels.clone()),
                    due_date: due.clone(),
                },
            )?;
            let failed = add_links(db, &t.id, links);
            Ok((with_failed_links(format!("Created {}", t.key), failed), Some(t.id)))
        }
        ProposedChange::UpdateTask {
            task_id,
            key,
            status,
            priority,
            task_type,
            due,
            clear_due,
            labels,
            description,
            links,
            ..
        } => {
            let others = task_type.is_some()
                || due.is_some()
                || *clear_due
                || labels.is_some()
                || description.is_some();
            if status.is_some() || priority.is_some() || others {
                tasks::update(
                    db,
                    task_id,
                    TaskPatch {
                        title: None,
                        task_type: *task_type,
                        description: description.clone(),
                        status: *status,
                        priority: *priority,
                        labels: labels.clone(),
                        due_date: if *clear_due { Some(String::new()) } else { due.clone() },
                    },
                )?;
            }
            let failed = add_links(db, task_id, links);
            let line = match (status, priority) {
                (Some(s), None) if !others && links.is_empty() => {
                    format!("Marked {key} as {}", status_phrase(*s))
                }
                (None, Some(p)) if !others && links.is_empty() => {
                    format!("Set {key} to {} priority", enum_id(p))
                }
                _ => format!("Updated {key}"),
            };
            Ok((with_failed_links(line, failed), Some(task_id.clone())))
        }
        ProposedChange::Comment { task_id, key, text } => {
            tasks::add_comment(db, task_id, text)?;
            Ok((format!("Commented on {key}"), Some(task_id.clone())))
        }
        ProposedChange::UpdatePage { page_id, new_body, base_updated_at, .. } => {
            let page = db.tx(|tx| {
                let page = knowledge::get_page(tx, page_id)?;
                if page.updated_at != *base_updated_at {
                    return Err(CoreError::Conflict(
                        "The page changed since this was proposed. Ask again.".into(),
                    ));
                }
                // External mode (D-72): a file changed outside Cairn since the index read it is
                // never overwritten by a proposal made from the old text.
                knowledge::update_page_in(
                    tx,
                    page_id,
                    PagePatch {
                        body: Some(new_body.clone()),
                        base_hash: page.file_hash.clone(),
                        ..PagePatch::default()
                    },
                    &tasks::Stamp::now(),
                )
                .map_err(|e| match e {
                    CoreError::ChangedOnDisk(_) => CoreError::Conflict(
                        "The page changed since this was proposed. Ask again.".into(),
                    ),
                    e => e,
                })
            })?;
            Ok((format!("Updated page “{}”", page.title), Some(page.id)))
        }
    }
}

/// The change in words, for a history line.
fn describe(conn: &Connection, change: &ProposedChange) -> String {
    match change {
        ProposedChange::CreateTask { category_id, title, .. } => {
            let category = categories::list(conn, true)
                .ok()
                .and_then(|l| l.into_iter().find(|p| p.id == *category_id))
                .map(|p| p.name);
            match category {
                Some(p) => format!("Create task “{title}” in {p}"),
                None => format!("Create task “{title}”"),
            }
        }
        ProposedChange::UpdateTask {
            key,
            status,
            priority,
            task_type,
            due,
            clear_due,
            labels,
            description,
            links,
            ..
        } => {
            let mut parts: Vec<String> = Vec::new();
            if let Some(s) = status {
                parts.push(format!("status {}", enum_id(s)));
            }
            if let Some(p) = priority {
                parts.push(format!("priority {}", enum_id(p)));
            }
            if let Some(y) = task_type {
                parts.push(format!("type {}", enum_id(y)));
            }
            if let Some(d) = due {
                parts.push(format!("due {d}"));
            }
            if *clear_due {
                parts.push("no due date".into());
            }
            if labels.is_some() {
                parts.push("labels".into());
            }
            if description.is_some() {
                parts.push("description".into());
            }
            if !links.is_empty() {
                parts.push("links".into());
            }
            format!("Update {key} ({})", parts.join(", "))
        }
        ProposedChange::Comment { key, text, .. } => {
            let short: String = text.chars().take(80).collect();
            format!("Comment on {key}: “{short}”")
        }
        ProposedChange::UpdatePage { title, .. } => format!("Change page “{title}”"),
    }
}

/// The card row as a history line (`history::turns`): its result, "Rejected: …", or the pending
/// change in words.
pub fn row_line(conn: &Connection, row: &ProposalRow) -> String {
    match row.state {
        RowState::Done => row
            .result
            .clone()
            .unwrap_or_else(|| describe(conn, &row.change)),
        RowState::Failed => format!(
            "Failed: {} ({})",
            describe(conn, &row.change),
            row.result.as_deref().unwrap_or("it could not be applied")
        ),
        RowState::Rejected => format!("Rejected: {}", describe(conn, &row.change)),
        RowState::Pending => format!("Proposed: {}", describe(conn, &row.change)),
    }
}

#[cfg(test)]
mod tests;
