//! Dev seed: the prototype's sample knowledge, inserted where ids are missing.

use rusqlite::{Connection, OptionalExtension, params};

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

pub(super) fn seed_if_missing(conn: &Connection, seed: KnowledgeSeed) -> Result<()> {
    for g in &seed.groups {
        if exists(conn, "categories", &g.category_id)? {
            conn.execute(
                "INSERT OR IGNORE INTO groups (id, category_id, name, description, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![g.id, g.category_id, g.name, g.description, g.created_at, g.updated_at],
            )?;
        }
    }
    let mut inserted_pages = Vec::new();
    for p in &seed.pages {
        if !exists(conn, "categories", &p.category_id)? {
            continue;
        }
        // A missing group or task only loses the reference, not the page.
        let group_id = match &p.group_id {
            Some(g) if exists(conn, "groups", g)? => Some(g.clone()),
            _ => None,
        };
        let origin_task_id = match &p.origin_task_id {
            Some(t) if exists(conn, "tasks", t)? => Some(t.clone()),
            _ => None,
        };
        // A page with the same title in the category (other id) is left alone.
        let n = conn.execute(
            "INSERT OR IGNORE INTO pages (id, category_id, group_id, title, body, source_info, origin,
                 origin_task_id, check_status, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                p.id,
                p.category_id,
                group_id,
                p.title,
                p.body,
                p.source_info,
                enum_str(&p.origin),
                origin_task_id,
                enum_str(&p.check_status),
                p.archived,
                p.created_at,
                p.updated_at,
            ],
        )?;
        if n == 1 {
            write_tags(conn, &p.id, &normalize_tags(p.tags.clone()))?;
            inserted_pages.push(p.id.clone());
        }
    }
    for v in &seed.versions {
        if exists(conn, "pages", &v.page_id)? {
            conn.execute(
                "INSERT OR IGNORE INTO page_versions (page_id, day, snapshot) VALUES (?1, ?2, ?3)",
                params![v.page_id, v.date, serde_json::to_string(&v.snapshot)?],
            )?;
        }
    }
    for l in &seed.task_page_links {
        if exists(conn, "tasks", &l.task_id)? && exists(conn, "pages", &l.page_id)? {
            conn.execute(
                "INSERT OR IGNORE INTO task_page_links (id, task_id, page_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![l.id, l.task_id, l.page_id, l.created_at],
            )?;
        }
    }
    // Hooks last, so every inserted page can mention every other one.
    for id in inserted_pages {
        let page = get_page(conn, &id)?;
        hooks::page_saved(conn, &page)?;
    }
    Ok(())
}
