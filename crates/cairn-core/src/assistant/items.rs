//! Tasks and pages as the assistant's prompts show them (`formats::ItemInfo`), loaded by id.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::Result;
use crate::assistant::formats::{ItemInfo, PageInfo, TaskInfo};
use crate::assistant::parts::{SourceKind, SourceRef};
use crate::tasks::TaskLinkType;
use crate::{CoreError, knowledge, categories, tasks};

/// A page or task by id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemRef {
    /// `Page` or `Task`.
    pub kind: SourceKind,
    pub id: String,
}

impl ItemRef {
    pub fn task(id: impl Into<String>) -> Self {
        Self { kind: SourceKind::Task, id: id.into() }
    }

    pub fn page(id: impl Into<String>) -> Self {
        Self { kind: SourceKind::Page, id: id.into() }
    }

    /// The item a source or find row points to; `None` for a file part or a web page.
    pub fn of_source(s: &SourceRef) -> Option<Self> {
        match (s.kind, &s.id) {
            (SourceKind::Page | SourceKind::Task, Some(id)) => Some(Self { kind: s.kind, id: id.clone() }),
            _ => None,
        }
    }
}

/// The items in the given order, with category, group, parent and link names filled in. Items that
/// no longer exist are skipped (a cited page may have been deleted since).
pub fn load(conn: &Connection, refs: &[ItemRef]) -> Result<Vec<ItemInfo>> {
    let category_names: HashMap<String, String> = categories::list(conn, true)?
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect();
    let category = |id: &str| category_names.get(id).cloned().unwrap_or_default();
    let mut out = Vec::with_capacity(refs.len());
    for r in refs {
        let item = match r.kind {
            SourceKind::Task => match tasks::get(conn, &r.id) {
                Ok(t) => {
                    let mut parent = None;
                    let mut links = Vec::new();
                    for l in tasks::links(conn, &t.id)? {
                        let other = if l.from_task_id == t.id { &l.to_task_id } else { &l.from_task_id };
                        let Ok(o) = tasks::get(conn, other) else { continue };
                        match l.link_type {
                            // `from` is the parent.
                            TaskLinkType::Parent if l.to_task_id == t.id => parent = Some(o.key),
                            _ => links.push(o.key),
                        }
                    }
                    for l in knowledge::task_page_links(conn, Some(&t.id), None)? {
                        if let Ok(p) = knowledge::get_page(conn, &l.page_id) {
                            links.push(p.title);
                        }
                    }
                    ItemInfo::Task(TaskInfo {
                        category: category(&t.category_id),
                        id: t.id,
                        key: t.key,
                        title: t.title,
                        status: t.status,
                        priority: t.priority,
                        task_type: t.task_type,
                        due: t.due_date,
                        labels: t.labels,
                        description: t.description,
                        parent,
                        links,
                    })
                }
                Err(CoreError::NotFound { .. }) => continue,
                Err(e) => return Err(e),
            },
            SourceKind::Page => match knowledge::get_page(conn, &r.id) {
                Ok(p) => {
                    // Nested groups (external mode) by their path, `Parent / Child`.
                    let group = match &p.group_id {
                        Some(g) => knowledge::groups_with_paths(conn, &p.category_id)?
                            .into_iter()
                            .find(|x| &x.id == g)
                            .map(|x| x.name),
                        None => None,
                    };
                    ItemInfo::Page(PageInfo {
                        category: category(&p.category_id),
                        id: p.id,
                        title: p.title,
                        group,
                        body: p.body,
                    })
                }
                Err(CoreError::NotFound { .. }) => continue,
                Err(e) => return Err(e),
            },
            SourceKind::File | SourceKind::Web => continue,
        };
        out.push(item);
    }
    Ok(out)
}
