//! Dev seed: the prototype's sample data, inserted where missing.

use rusqlite::{Connection, params};

use super::*;

fn exists(conn: &Connection, table: &str, id: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE id = ?1"),
            [id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

pub(super) fn seed_if_missing(conn: &Connection, seed: TaskSeed) -> Result<()> {
    for t in &seed.tasks {
        if !exists(conn, "categories", &t.category_id)? {
            continue;
        }
        let number: i64 = t
            .key
            .rsplit('-')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO tasks (id, key, number, category_id, title, type, description, status,
                 priority, due_date, created_at, updated_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                t.id,
                t.key,
                number,
                t.category_id,
                t.title,
                enum_str(&t.task_type),
                t.description,
                enum_str(&t.status),
                enum_str(&t.priority),
                t.due_date,
                t.created_at,
                t.updated_at,
                t.completed_at,
            ],
        )?;
        if inserted == 1 {
            for l in normalize_labels(t.labels.clone()) {
                conn.execute(
                    "INSERT OR IGNORE INTO task_labels (task_id, label) VALUES (?1, ?2)",
                    params![t.id, l],
                )?;
            }
        }
    }
    for l in &seed.links {
        if exists(conn, "tasks", &l.from_task_id)? && exists(conn, "tasks", &l.to_task_id)? {
            conn.execute(
                "INSERT OR IGNORE INTO task_links (id, type, from_task_id, to_task_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    l.id,
                    enum_str(&l.link_type),
                    l.from_task_id,
                    l.to_task_id,
                    l.created_at
                ],
            )?;
        }
    }
    for c in &seed.comments {
        if exists(conn, "tasks", &c.task_id)? {
            conn.execute(
                "INSERT OR IGNORE INTO task_comments (id, task_id, text, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![c.id, c.task_id, c.text, c.created_at, c.updated_at],
            )?;
        }
    }
    for v in &seed.history {
        if exists(conn, "tasks", &v.task_id)? {
            conn.execute(
                "INSERT OR IGNORE INTO task_versions (task_id, day, snapshot) VALUES (?1, ?2, ?3)",
                params![v.task_id, v.date, serde_json::to_string(&v.snapshot)?],
            )?;
        }
    }
    for a in &seed.activity {
        conn.execute(
            "INSERT OR IGNORE INTO activity (id, kind, ref_id, at) VALUES (?1, ?2, ?3, ?4)",
            params![a.id, enum_str(&a.kind), a.ref_id, a.at],
        )?;
    }
    conn.execute(
        "UPDATE categories SET next_task_number = MAX(next_task_number,
             (SELECT COALESCE(MAX(number), 0) + 1 FROM tasks WHERE category_id = categories.id))",
        [],
    )?;
    // Seeded rows bypass the per-save hooks; index them for search and "Mentioned in".
    crate::hooks::rebuild_all(conn)?;
    Ok(())
}
