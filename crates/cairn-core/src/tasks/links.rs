//! Task links (FR-TASK-LN-1, -2, -4).

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::*;

fn link_from_row(r: &Row) -> rusqlite::Result<TaskLink> {
    let ty: String = r.get(1)?;
    Ok(TaskLink {
        id: r.get(0)?,
        link_type: parse_enum(1, &ty)?,
        from_task_id: r.get(2)?,
        to_task_id: r.get(3)?,
        created_at: r.get(4)?,
    })
}

const COLUMNS: &str = "id, type, from_task_id, to_task_id, created_at";

pub(super) fn links(conn: &Connection, task_id: &str) -> Result<Vec<TaskLink>> {
    let id = get(conn, task_id)?.id;
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM task_links WHERE from_task_id = ?1 OR to_task_id = ?1
         ORDER BY created_at, id"
    ))?;
    let rows = stmt
        .query_map([&id], link_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn exists(conn: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<bool> {
    Ok(conn.query_row(sql, args, |_| Ok(())).optional()?.is_some())
}

pub(super) fn add_link(
    conn: &Connection,
    link_type: TaskLinkType,
    from: &str,
    to: &str,
) -> Result<TaskLink> {
    let from = get(conn, from)?;
    let to = get(conn, to)?;
    if from.id == to.id {
        return Err(CoreError::Invalid(
            "A task can't be linked to itself.".into(),
        ));
    }
    let (from_id, to_id) = match link_type {
        TaskLinkType::Relates if from.id > to.id => (to.id.clone(), from.id.clone()),
        _ => (from.id.clone(), to.id.clone()),
    };
    let ty = enum_str(&link_type);
    if exists(
        conn,
        "SELECT 1 FROM task_links WHERE type = ?1 AND from_task_id = ?2 AND to_task_id = ?3",
        params![ty, from_id, to_id],
    )? {
        return Err(CoreError::Conflict(format!(
            "{} and {} are already linked this way.",
            from.key, to.key
        )));
    }
    if link_type == TaskLinkType::Parent {
        // `from` is the parent, `to` the sub-task.
        if exists(
            conn,
            "SELECT 1 FROM task_links WHERE type = 'parent' AND to_task_id = ?1",
            [&to.id],
        )? {
            return Err(CoreError::Conflict(format!(
                "{} already has a parent task. Remove that link first.",
                to.key
            )));
        }
        if exists(
            conn,
            "SELECT 1 FROM task_links WHERE type = 'parent' AND to_task_id = ?1",
            [&from.id],
        )? {
            return Err(CoreError::Invalid(format!(
                "{} is a sub-task itself, and sub-tasks can't have sub-tasks (one level only).",
                from.key
            )));
        }
        if exists(
            conn,
            "SELECT 1 FROM task_links WHERE type = 'parent' AND from_task_id = ?1",
            [&to.id],
        )? {
            return Err(CoreError::Invalid(format!(
                "{} has sub-tasks, so it can't become a sub-task (one level only).",
                to.key
            )));
        }
    }
    let link = TaskLink {
        id: new_id(),
        link_type,
        from_task_id: from_id,
        to_task_id: to_id,
        created_at: now_iso(),
    };
    conn.execute(
        "INSERT INTO task_links (id, type, from_task_id, to_task_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![link.id, ty, link.from_task_id, link.to_task_id, link.created_at],
    )?;
    Ok(link)
}

pub(super) fn remove_link(conn: &Connection, id: &str) -> Result<()> {
    let n = conn.execute("DELETE FROM task_links WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(CoreError::NotFound {
            entity: "link",
            id: id.to_string(),
        });
    }
    Ok(())
}
