//! Find requests (FR-LLM-AS-2): every task and page that matches the plan's filters, listed by
//! Rust, never cut short by the model.
//!
//! - Filters: the scope (or the plan's in-scope categories), kinds, task statuses, priorities, due
//!   and updated ranges. A status, priority or due filter implies tasks only. Archived pages are
//!   left out.
//! - Topic words: when the plan has search words, only items that `retrieve`'s relevance rule
//!   passes are kept (`matching`, computed by the caller with the same rule), in that order;
//!   without words, tasks come by due date (none last), then priority, then key, and pages by
//!   title.
//!
//! Owned by the M6 answers agent.

use chrono::NaiveDate;
use rusqlite::types::Value;
use rusqlite::{Connection, params_from_iter};

use crate::Result;
use crate::assistant::items::ItemRef;
use crate::assistant::parts::{FoundItem, SourceKind};
use crate::assistant::plan::{DateRange, PlanFilters};
use crate::search::SearchKind;
use crate::tasks::Priority;

#[derive(Debug, Clone)]
pub struct FindQuery<'a> {
    /// The conversation's scope; empty = every category.
    pub scope_category_ids: &'a [String],
    pub filters: &'a PlanFilters,
    /// The items the topic words matched, best first; `None` when the plan had no words.
    pub matching: Option<&'a [ItemRef]>,
    pub today: NaiveDate,
}

/// Today in the computer's time zone (`formats::today_line`, the plan's relative dates).
pub fn local_today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// One item passing the filters, with what the default order needs.
pub(crate) struct Row {
    pub(crate) item: FoundItem,
    due: Option<String>,
    priority: Priority,
    /// The task key's number (`OPS-41` is 41), so `OPS-9` comes before `OPS-10`.
    number: i64,
}

/// Whether the plan filters anything beyond the categories and the kinds.
pub(crate) fn has_item_filters(f: &PlanFilters) -> bool {
    !f.statuses.is_empty() || !f.priorities.is_empty() || f.due.is_some() || f.updated.is_some()
}

fn push_range(sql: &mut String, args: &mut Vec<Value>, column: &str, r: &DateRange) {
    if let Some(from) = r.from {
        sql.push_str(&format!(" AND {column} >= ?"));
        args.push(Value::Text(from.format("%Y-%m-%d").to_string()));
    }
    if let Some(to) = r.to {
        sql.push_str(&format!(" AND {column} <= ?"));
        args.push(Value::Text(to.format("%Y-%m-%d").to_string()));
    }
}

fn push_categories(sql: &mut String, args: &mut Vec<Value>, scope: &[String]) {
    if !scope.is_empty() {
        sql.push_str(&format!(
            " AND category_id IN ({})",
            vec!["?"; scope.len()].join(",")
        ));
        args.extend(scope.iter().cloned().map(Value::Text));
    }
}

/// Every page and task that passes the scope and the filters, unordered.
pub(crate) fn rows(conn: &Connection, scope: &[String], f: &PlanFilters) -> Result<Vec<Row>> {
    let tasks_only = !f.statuses.is_empty() || !f.priorities.is_empty() || f.due.is_some();
    let wants = |k: SearchKind| f.kinds.is_empty() || f.kinds.contains(&k);
    let updated = "date(updated_at, 'localtime')";
    let mut out = Vec::new();

    if wants(SearchKind::Task) {
        let mut sql = "SELECT id, key, category_id, title, due_date, priority, number FROM tasks WHERE 1 = 1"
            .to_string();
        let mut args: Vec<Value> = Vec::new();
        push_categories(&mut sql, &mut args, scope);
        if !f.statuses.is_empty() {
            sql.push_str(&format!(
                " AND status IN ({})",
                vec!["?"; f.statuses.len()].join(",")
            ));
            args.extend(
                f.statuses
                    .iter()
                    .map(|s| Value::Text(super::formats::enum_id(s))),
            );
        }
        if !f.priorities.is_empty() {
            sql.push_str(&format!(
                " AND priority IN ({})",
                vec!["?"; f.priorities.len()].join(",")
            ));
            args.extend(
                f.priorities
                    .iter()
                    .map(|p| Value::Text(super::formats::enum_id(p))),
            );
        }
        if let Some(r) = &f.due {
            push_range(&mut sql, &mut args, "due_date", r);
        }
        if let Some(r) = &f.updated {
            push_range(&mut sql, &mut args, updated, r);
        }
        let mut stmt = conn.prepare(&sql)?;
        let found = stmt
            .query_map(params_from_iter(args), |r| {
                let priority: String = r.get(5)?;
                Ok(Row {
                    item: FoundItem {
                        kind: SourceKind::Task,
                        id: r.get(0)?,
                        key: Some(r.get(1)?),
                        category_id: r.get(2)?,
                        title: r.get(3)?,
                    },
                    due: r.get(4)?,
                    priority: match priority.as_str() {
                        "high" => Priority::High,
                        "low" => Priority::Low,
                        _ => Priority::Medium,
                    },
                    number: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        out.extend(found);
    }

    if wants(SearchKind::Page) && !tasks_only {
        let mut sql =
            "SELECT id, category_id, title FROM pages WHERE archived = 0".to_string();
        let mut args: Vec<Value> = Vec::new();
        push_categories(&mut sql, &mut args, scope);
        if let Some(r) = &f.updated {
            push_range(&mut sql, &mut args, updated, r);
        }
        let mut stmt = conn.prepare(&sql)?;
        let found = stmt
            .query_map(params_from_iter(args), |r| {
                Ok(Row {
                    item: FoundItem {
                        kind: SourceKind::Page,
                        id: r.get(0)?,
                        key: None,
                        category_id: r.get(1)?,
                        title: r.get(2)?,
                    },
                    due: None,
                    priority: Priority::Medium,
                    number: 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        out.extend(found);
    }
    Ok(out)
}

fn priority_rank(p: Priority) -> u8 {
    match p {
        Priority::High => 0,
        Priority::Medium => 1,
        Priority::Low => 2,
    }
}

/// The key without its number, for ordering (`OPS`).
fn key_prefix(key: &str) -> &str {
    key.rsplit_once('-').map_or(key, |(p, _)| p)
}

pub fn find(conn: &Connection, q: &FindQuery<'_>) -> Result<Vec<FoundItem>> {
    let scope: &[String] = if q.filters.category_ids.is_empty() {
        q.scope_category_ids
    } else {
        &q.filters.category_ids
    };
    let mut rows = rows(conn, scope, q.filters)?;
    if let Some(matching) = q.matching {
        // The topic's order is the relevance order; the filters only keep or drop.
        let mut by_item: std::collections::HashMap<(SourceKind, String), FoundItem> = rows
            .drain(..)
            .map(|r| ((r.item.kind, r.item.id.clone()), r.item))
            .collect();
        return Ok(matching
            .iter()
            .filter_map(|m| by_item.remove(&(m.kind, m.id.clone())))
            .collect());
    }
    // Tasks first (due soonest, none last; then priority, then key), pages by title.
    rows.sort_by(|a, b| {
        let page = |r: &Row| r.item.kind == SourceKind::Page;
        page(a)
            .cmp(&page(b))
            .then_with(|| {
                if page(a) {
                    a.item
                        .title
                        .to_lowercase()
                        .cmp(&b.item.title.to_lowercase())
                } else {
                    a.due
                        .is_none()
                        .cmp(&b.due.is_none())
                        .then_with(|| a.due.cmp(&b.due))
                        .then_with(|| priority_rank(a.priority).cmp(&priority_rank(b.priority)))
                        .then_with(|| {
                            let (ka, kb) = (
                                a.item.key.as_deref().unwrap_or(""),
                                b.item.key.as_deref().unwrap_or(""),
                            );
                            key_prefix(ka)
                                .cmp(key_prefix(kb))
                                .then(a.number.cmp(&b.number))
                        })
                }
            })
            .then_with(|| a.item.id.cmp(&b.item.id))
    });
    Ok(rows.into_iter().map(|r| r.item).collect())
}

#[cfg(test)]
mod tests;
