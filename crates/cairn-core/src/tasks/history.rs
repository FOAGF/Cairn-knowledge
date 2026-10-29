//! Daily versions, the completion summary and the activity log.

use rusqlite::{Connection, params};

use super::workdays::{blocked_working_days, working_days_between};
use super::*;

/// Today's version of the task: inserts or overwrites (FR-TASK-HI-1, -4).
pub(super) fn save_version(conn: &Connection, t: &Task, st: &Stamp) -> Result<()> {
    let snapshot = TaskSnapshot {
        title: t.title.clone(),
        task_type: t.task_type,
        description: t.description.clone(),
        status: t.status,
        priority: t.priority,
        labels: t.labels.clone(),
        due_date: t.due_date.clone(),
        category_id: t.category_id.clone(),
    };
    conn.execute(
        "INSERT INTO task_versions (task_id, day, snapshot) VALUES (?1, ?2, ?3)
         ON CONFLICT (task_id, day) DO UPDATE SET snapshot = excluded.snapshot",
        params![t.id, date_str(st.day), serde_json::to_string(&snapshot)?],
    )?;
    Ok(())
}

pub(super) fn versions(conn: &Connection, task_id: &str) -> Result<Vec<TaskVersion>> {
    let id = get(conn, task_id)?.id;
    let mut stmt =
        conn.prepare("SELECT day, snapshot FROM task_versions WHERE task_id = ?1 ORDER BY day")?;
    let rows = stmt
        .query_map([&id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(date, snapshot)| {
            Ok(TaskVersion {
                task_id: id.clone(),
                date,
                snapshot: serde_json::from_str(&snapshot)?,
            })
        })
        .collect()
}

pub(super) fn completion_summary(
    conn: &Connection,
    task_id: &str,
) -> Result<Option<CompletionSummary>> {
    let t = get(conn, task_id)?;
    if t.status != TaskStatus::Done {
        return Ok(None);
    }
    let Some(start) = local_day(&t.created_at) else {
        return Ok(None);
    };
    let end = local_day(t.completed_at.as_deref().unwrap_or(&t.updated_at))
        .unwrap_or(start)
        .max(start);
    let versions: Vec<_> = versions(conn, &t.id)?
        .into_iter()
        .filter_map(|v| {
            NaiveDate::parse_from_str(&v.date, "%Y-%m-%d")
                .ok()
                .map(|d| (d, v.snapshot.status))
        })
        .collect();
    Ok(Some(CompletionSummary {
        working_days: working_days_between(start, end),
        blocked_days: blocked_working_days(start, end, &versions),
    }))
}

/// Writes an activity row. A `task_updated` replaces the task's earlier one of the same local day.
/// Writes an activity row for a task or page. `task_updated` and `page_updated` keep at most one
/// row per item per local day (the earlier one of the same day is replaced).
pub(crate) fn log_activity(
    conn: &Connection,
    kind: ActivityKind,
    ref_id: &str,
    st: &Stamp,
) -> Result<()> {
    if matches!(kind, ActivityKind::TaskUpdated | ActivityKind::PageUpdated) {
        let mut stmt =
            conn.prepare("SELECT id, at FROM activity WHERE ref_id = ?1 AND kind = ?2")?;
        let same_day: Vec<String> = stmt
            .query_map(params![ref_id, enum_str(&kind)], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, at)| local_day(at) == Some(st.day))
            .map(|(id, _)| id)
            .collect();
        for id in same_day {
            conn.execute("DELETE FROM activity WHERE id = ?1", [id])?;
        }
    }
    conn.execute(
        "INSERT INTO activity (id, kind, ref_id, at) VALUES (?1, ?2, ?3, ?4)",
        params![new_id(), enum_str(&kind), ref_id, st.at],
    )?;
    Ok(())
}

pub(super) fn activity(conn: &Connection, limit: u32) -> Result<Vec<ActivityEvent>> {
    let mut stmt = conn
        .prepare("SELECT id, kind, ref_id, at FROM activity ORDER BY at DESC, id DESC LIMIT ?1")?;
    let rows = stmt
        .query_map([limit], |r| {
            let kind: String = r.get(1)?;
            Ok(ActivityEvent {
                id: r.get(0)?,
                kind: parse_enum(1, &kind)?,
                ref_id: r.get(2)?,
                at: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
