//! Changing many tasks or pages at once (FR-TASK-UI-7, FR-KN-UI-3, D-70).
//!
//! A run is one transaction. Each item runs inside its own savepoint, so an item that can't take
//! the change (a title clash, a missing item, a closed category) is put back on its own and named
//! in `skipped`, while the others are kept. Database and file errors stop the whole run.

use std::collections::HashSet;

use rusqlite::Connection;
use serde::Serialize;
use specta::Type;

use crate::{CoreError, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult {
    /// Items changed, or already as asked.
    pub done: u32,
    pub skipped: Vec<BulkSkip>,
}

/// An item left as it was, with the reason (the message the single action would show).
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BulkSkip {
    pub id: String,
    /// What the user knows the item by (a task's key and title, a page's title); empty if it is gone.
    pub title: String,
    pub reason: String,
}

/// Runs `change` on each id once, in the given order. `name` gives an item's title for a skip.
pub(crate) fn each(
    conn: &Connection,
    ids: &[String],
    name: impl Fn(&Connection, &str) -> Option<String>,
    mut change: impl FnMut(&Connection, &str) -> Result<()>,
) -> Result<BulkResult> {
    let mut out = BulkResult::default();
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        conn.execute_batch("SAVEPOINT bulk_item")?;
        // External mode: what this item did to files is taken back with its rows.
        let mark = crate::knowledge::external::journal::mark();
        match change(conn, id) {
            Ok(()) => {
                conn.execute_batch("RELEASE bulk_item")?;
                out.done += 1;
            }
            Err(
                e @ (CoreError::NotFound { .. } | CoreError::Invalid(_) | CoreError::Conflict(_)),
            ) => {
                conn.execute_batch("ROLLBACK TO bulk_item; RELEASE bulk_item")?;
                crate::knowledge::external::journal::rollback_to(mark);
                out.skipped.push(BulkSkip {
                    id: id.clone(),
                    title: name(conn, id).unwrap_or_default(),
                    reason: e.to_string(),
                });
            }
            // The transaction is rolled back as a whole.
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}
