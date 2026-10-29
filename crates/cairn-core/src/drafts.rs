//! Unsaved editor content (FR-KN-ED-10). Drafts are local to this device and not exported.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::knowledge::Draft;
use crate::util::now_iso;
use crate::{CoreError, Result};

const KINDS: &[&str] = &[
    "page",
    "new_page",
    "task_description",
    "comment",
    "new_comment",
];

fn check_kind(kind: &str) -> Result<()> {
    if KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!(
            "\"{kind}\" is not a draft kind. Use one of: {}.",
            KINDS.join(", ")
        )))
    }
}

fn from_row(r: &Row) -> rusqlite::Result<Draft> {
    Ok(Draft {
        kind: r.get(0)?,
        ref_id: r.get(1)?,
        content: r.get(2)?,
        updated_at: r.get(3)?,
    })
}

pub fn get(conn: &Connection, kind: &str, ref_id: &str) -> Result<Option<Draft>> {
    Ok(conn
        .query_row(
            "SELECT kind, ref_id, content, updated_at FROM drafts WHERE kind = ?1 AND ref_id = ?2",
            params![kind, ref_id],
            from_row,
        )
        .optional()?)
}

/// Insert or replace. `kind` must be one of page, new_page, task_description, comment, new_comment.
pub fn save(conn: &Connection, kind: &str, ref_id: &str, content: &str) -> Result<Draft> {
    check_kind(kind)?;
    let d = Draft {
        kind: kind.to_string(),
        ref_id: ref_id.to_string(),
        content: content.to_string(),
        updated_at: now_iso(),
    };
    conn.execute(
        "INSERT INTO drafts (kind, ref_id, content, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (kind, ref_id) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
        params![d.kind, d.ref_id, d.content, d.updated_at],
    )?;
    Ok(d)
}

pub fn delete(conn: &Connection, kind: &str, ref_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM drafts WHERE kind = ?1 AND ref_id = ?2",
        params![kind, ref_id],
    )?;
    Ok(())
}

/// Drafts of one kind, newest first (e.g. new pages left unsaved).
pub fn list(conn: &Connection, kind: &str) -> Result<Vec<Draft>> {
    let mut stmt = conn.prepare(
        "SELECT kind, ref_id, content, updated_at FROM drafts WHERE kind = ?1
         ORDER BY updated_at DESC, ref_id",
    )?;
    let rows = stmt
        .query_map([kind], from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
