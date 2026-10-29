//! Keeps derived data (mention index, keyword search index) in step with every save.
//! The tasks and knowledge modules call these inside their write transactions.

use rusqlite::Connection;

use crate::Result;
use crate::glossary;
use crate::knowledge::{MentionSource, MentionTarget, Page};
use crate::mentions;
use crate::search::{self, SearchKind};
use crate::tasks::{Task, TaskComment};
use crate::vectors;

pub(crate) fn task_saved(conn: &Connection, task: &Task) -> Result<()> {
    mentions::index_source(conn, MentionSource::Task, &task.id, &task.description)?;
    // The glossary comes first: the keyword and vector index read the terms an item uses.
    glossary::reindex(conn, SearchKind::Task, &task.id)
}

/// Call before the task row is deleted (its comments are still there).
pub(crate) fn task_deleted(conn: &Connection, task_id: &str) -> Result<()> {
    mentions::remove_item(conn, MentionTarget::Task, task_id)?;
    glossary::remove_item(conn, SearchKind::Task, task_id)?;
    search::remove(conn, SearchKind::Task, task_id)?;
    vectors::remove_item(conn, SearchKind::Task, task_id)
}

pub(crate) fn comment_saved(conn: &Connection, comment: &TaskComment) -> Result<()> {
    mentions::index_source(conn, MentionSource::Comment, &comment.id, &comment.text)?;
    glossary::reindex(conn, SearchKind::Task, &comment.task_id)
}

/// Call after the comment row is deleted.
pub(crate) fn comment_deleted(conn: &Connection, comment: &TaskComment) -> Result<()> {
    mentions::remove_source(conn, MentionSource::Comment, &comment.id)?;
    glossary::reindex(conn, SearchKind::Task, &comment.task_id)
}

pub(crate) fn page_saved(conn: &Connection, page: &Page) -> Result<()> {
    mentions::index_source(conn, MentionSource::Page, &page.id, &page.body)?;
    glossary::reindex(conn, SearchKind::Page, &page.id)
}

/// Call before the page row is deleted.
pub(crate) fn page_deleted(conn: &Connection, page_id: &str) -> Result<()> {
    mentions::remove_item(conn, MentionTarget::Page, page_id)?;
    glossary::remove_item(conn, SearchKind::Page, page_id)?;
    search::remove(conn, SearchKind::Page, page_id)?;
    vectors::remove_item(conn, SearchKind::Page, page_id)
}

/// Rebuilds the derived indexes (after import, schema upgrades and the dev seeds): the mention and
/// keyword indexes from scratch, the vector pieces keeping the vectors of unchanged pieces.
pub fn rebuild_all(conn: &Connection) -> Result<()> {
    mentions::rebuild(conn)?;
    // Uses first: the keyword and vector index read them.
    glossary::rebuild(conn)?;
    search::rebuild(conn)?;
    vectors::rebuild(conn)
}
