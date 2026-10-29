//! Knowledge pages (spec §5): groups, pages, tags, daily history and restore, task links,
//! Markdown export, and the dev seed.
//!
//! The signatures here are the M3 contract used by `src-tauri/src/commands/knowledge.rs`.
//! Every page change, in one transaction:
//! - updates `updated_at` (an update that changes nothing writes nothing, as for tasks);
//! - overwrites today's version in `page_versions` (local date; FR-KN-ED-5);
//! - writes activity: `page_created`, or one `page_updated` per page per day (replaced);
//! - calls `crate::hooks::page_saved` / `page_deleted` so mentions and search stay in step.
//!
//! Decisions: pages can't be created in or moved to an archived category; moving a page to its own
//! category does nothing; `set_check_status` changes only the status (no new version, activity or
//! `updated_at`, because checking is not editing); archiving counts as an update.
//! Each public function runs in one transaction and delegates to an internal `*_in` function that
//! takes the clock (`Stamp`), so tests can pick the day.
//!
//! **External mode (D-72):** the rows are an index of a folder of Markdown files. Each `*_in`
//! function first asks `external::active`; in external mode it hands over to its counterpart in
//! `external::pages`, which writes the file (or folder) and then the row through the same helpers,
//! so every caller works unchanged. Internal mode's code below is untouched by it.

mod export;
pub mod external;
mod folders;
pub mod mode;
pub mod model;
mod seed;

#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashSet};

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::bulk::BulkResult;
use crate::tasks::history::log_activity;
use crate::tasks::{ActivityKind, Stamp, date_str, enum_str, parse_enum};
use crate::util::{new_id, now_iso};
use crate::{CoreError, Db, Result, attachments, hooks, categories, tasks};
pub use folders::*;
pub use model::*;

// ---------------------------------------------------------------- helpers

fn page_not_found(id: &str) -> CoreError {
    CoreError::NotFound {
        entity: "page",
        id: id.to_string(),
    }
}

fn group_not_found(id: &str) -> CoreError {
    CoreError::NotFound {
        entity: "group",
        id: id.to_string(),
    }
}

/// Words in `text`; each Han character counts as one (FR-KN-ED-4). Mirrors `countWords` in the UI.
pub fn count_words(text: &str) -> u32 {
    fn cjk(c: char) -> bool {
        matches!(c, '\u{3400}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = 0;
    let mut i = 0;
    let alnum = |c: char| c.is_ascii_alphanumeric();
    while i < chars.len() {
        let c = chars[i];
        if cjk(c) {
            n += 1;
            i += 1;
        } else if alnum(c) {
            // A word: alphanumerics, joined by single ' ’ or - between alphanumerics.
            while i < chars.len() && alnum(chars[i]) {
                i += 1;
                if i + 1 < chars.len()
                    && matches!(chars[i], '\'' | '’' | '-')
                    && alnum(chars[i + 1])
                {
                    i += 1;
                }
            }
            n += 1;
        } else {
            i += 1;
        }
    }
    n
}

pub(crate) fn sort_tags(mut tags: Vec<String>) -> Vec<String> {
    tags.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)));
    tags
}

/// Trimmed, non-empty, case-insensitively unique (first spelling wins), sorted.
pub(crate) fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let kept = tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && seen.insert(t.to_lowercase()))
        .collect();
    sort_tags(kept)
}

fn clean_title(title: &str) -> Result<String> {
    let t = title.trim();
    if t.is_empty() {
        return Err(CoreError::Invalid("A page needs a title.".into()));
    }
    Ok(t.to_string())
}

pub(crate) fn clean_source(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

// ---------------------------------------------------------------- groups

const GROUP_COLUMNS: &str = "id, category_id, name, description, created_at, updated_at, parent_id, path";

fn group_from_row(r: &Row) -> rusqlite::Result<Group> {
    Ok(Group {
        id: r.get(0)?,
        category_id: r.get(1)?,
        parent_id: r.get(6)?,
        name: r.get(2)?,
        description: r.get(3)?,
        path: r.get(7)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

fn get_group(conn: &Connection, id: &str) -> Result<Group> {
    conn.query_row(
        &format!("SELECT {GROUP_COLUMNS} FROM groups WHERE id = ?1"),
        [id],
        group_from_row,
    )
    .optional()?
    .ok_or_else(|| group_not_found(id))
}

fn clean_group_name(name: &str) -> Result<String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(CoreError::Invalid("A group needs a name.".into()));
    }
    Ok(n.to_string())
}

/// `Conflict` if another group of the category has this name (case-insensitive).
fn check_group_name_free(
    conn: &Connection,
    category_id: &str,
    name: &str,
    except: Option<&str>,
) -> Result<()> {
    let lower = name.to_lowercase();
    let mut stmt = conn.prepare("SELECT id, name FROM groups WHERE category_id = ?1")?;
    let rows = stmt
        .query_map([category_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows
        .iter()
        .any(|(id, n)| Some(id.as_str()) != except && n.to_lowercase() == lower)
    {
        return Err(CoreError::Conflict(format!(
            "This category already has a group named \"{name}\"."
        )));
    }
    Ok(())
}

/// Groups of one category, or of all categories, sorted by name.
pub fn groups(conn: &Connection, category_id: Option<&str>) -> Result<Vec<Group>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {GROUP_COLUMNS} FROM groups WHERE (?1 IS NULL OR category_id = ?1)
         ORDER BY name COLLATE NOCASE, id"
    ))?;
    let rows = stmt
        .query_map([category_id], group_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The groups of a category with nested ones named by their path, `Parent / Child` (for the LLM:
/// the checks and *Suggest groups*, D-72). Internal mode's groups keep their names.
pub fn groups_with_paths(conn: &Connection, category_id: &str) -> Result<Vec<Group>> {
    let all = groups(conn, Some(category_id))?;
    let by_id: std::collections::HashMap<&str, &Group> =
        all.iter().map(|g| (g.id.as_str(), g)).collect();
    let label = |g: &Group| {
        let mut parts = vec![g.name.clone()];
        let mut at = g.parent_id.as_deref();
        let mut guard = 0;
        while let Some(id) = at
            && let Some(parent) = by_id.get(id)
            && guard < 64
        {
            parts.push(parent.name.clone());
            at = parent.parent_id.as_deref();
            guard += 1;
        }
        parts.reverse();
        parts.join(" / ")
    };
    let mut out: Vec<Group> = all
        .iter()
        .map(|g| Group {
            name: label(g),
            ..g.clone()
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    Ok(out)
}

/// Name required, unique within the category (FR-KN-ED-12). External mode: unique within the
/// folder it is made in, which is made on disk; `parent_id` nests it (FR-KN-EXT-2).
pub fn create_group(db: &mut Db, input: NewGroup) -> Result<Group> {
    db.tx(|tx| create_group_in(tx, input))
}

fn create_group_in(conn: &Connection, input: NewGroup) -> Result<Group> {
    if let Some(ext) = external::active(conn)? {
        return external::pages::create_group(conn, &ext, input);
    }
    if input.parent_id.as_deref().is_some_and(|p| !p.trim().is_empty()) {
        return Err(not_nested());
    }
    let name = clean_group_name(&input.name)?;
    categories::get(conn, &input.category_id)?;
    check_group_name_free(conn, &input.category_id, &name, None)?;
    let now = now_iso();
    let g = Group {
        id: new_id(),
        category_id: input.category_id,
        parent_id: None,
        name,
        description: input.description.unwrap_or_default().trim().to_string(),
        path: None,
        created_at: now.clone(),
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO groups (id, category_id, name, description, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            g.id,
            g.category_id,
            g.name,
            g.description,
            g.created_at,
            g.updated_at
        ],
    )?;
    Ok(g)
}

/// Groups are one level in internal mode (FR-KN-GR-1).
fn not_nested() -> CoreError {
    CoreError::Invalid("Groups inside groups need knowledge kept in a folder (external mode).".into())
}

pub fn update_group(db: &mut Db, id: &str, patch: GroupPatch) -> Result<Group> {
    db.tx(|tx| {
        if let Some(ext) = external::active(tx)? {
            return external::pages::update_group(tx, &ext, id, patch);
        }
        if patch.parent_id.as_deref().is_some_and(|p| !p.trim().is_empty()) {
            return Err(not_nested());
        }
        let old = get_group(tx, id)?;
        let mut g = old.clone();
        if let Some(n) = patch.name {
            g.name = clean_group_name(&n)?;
        }
        if let Some(d) = patch.description {
            g.description = d.trim().to_string();
        }
        if g == old {
            return Ok(old);
        }
        if g.name != old.name {
            check_group_name_free(tx, &g.category_id, &g.name, Some(&g.id))?;
        }
        g.updated_at = now_iso();
        tx.execute(
            "UPDATE groups SET name = ?2, description = ?3, updated_at = ?4 WHERE id = ?1",
            params![g.id, g.name, g.description, g.updated_at],
        )?;
        Ok(g)
    })
}

/// Its pages become ungrouped (UI-KN-1). External mode: its pages and groups move up one level
/// (refused as a whole when a name would clash), then its folder goes if nothing else is in it.
pub fn delete_group(db: &mut Db, id: &str) -> Result<()> {
    db.tx(|tx| {
        if let Some(ext) = external::active(tx)? {
            return external::pages::delete_group(tx, &ext, id, &Stamp::now());
        }
        get_group(tx, id)?;
        // pages.group_id is ON DELETE SET NULL.
        tx.execute("DELETE FROM groups WHERE id = ?1", [id])?;
        Ok(())
    })
}

// ---------------------------------------------------------------- pages: reading

const PAGE_COLUMNS: &str = "p.id, p.category_id, p.group_id, p.title, p.body, p.source_info, p.origin, \
     p.origin_task_id, p.check_status, p.archived, p.created_at, p.updated_at, p.path, p.file_hash";

/// A page without its tags (see [`load_tags`]).
fn page_from_row(r: &Row) -> rusqlite::Result<Page> {
    let origin: String = r.get(6)?;
    let status: String = r.get(8)?;
    Ok(Page {
        id: r.get(0)?,
        category_id: r.get(1)?,
        group_id: r.get(2)?,
        title: r.get(3)?,
        body: r.get(4)?,
        tags: Vec::new(),
        source_info: r.get(5)?,
        origin: parse_enum(6, &origin)?,
        origin_task_id: r.get(7)?,
        check_status: parse_enum(8, &status)?,
        archived: r.get::<_, i64>(9)? != 0,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        path: r.get(12)?,
        file_hash: r.get(13)?,
    })
}

fn load_tags(conn: &Connection, page_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT tag FROM page_tags WHERE page_id = ?1")?;
    let tags = stmt
        .query_map([page_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(sort_tags(tags))
}

fn summary(conn: &Connection, p: Page) -> Result<PageSummary> {
    Ok(PageSummary {
        word_count: count_words(&p.body),
        tags: load_tags(conn, &p.id)?,
        id: p.id,
        category_id: p.category_id,
        group_id: p.group_id,
        title: p.title,
        source_info: p.source_info,
        origin: p.origin,
        origin_task_id: p.origin_task_id,
        check_status: p.check_status,
        archived: p.archived,
        created_at: p.created_at,
        updated_at: p.updated_at,
        path: p.path,
        file_hash: p.file_hash,
    })
}

pub fn list_pages(conn: &Connection, filter: &PageFilter) -> Result<Vec<PageSummary>> {
    let mut sql = format!("SELECT {PAGE_COLUMNS} FROM pages p WHERE 1 = 1");
    let mut args: Vec<Value> = Vec::new();
    let arg = |args: &mut Vec<Value>, v: String| {
        args.push(Value::Text(v));
        format!("?{}", args.len())
    };
    if let Some(ids) = &filter.category_ids {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let ph: Vec<String> = ids.iter().map(|i| arg(&mut args, i.clone())).collect();
        sql.push_str(&format!(" AND p.category_id IN ({})", ph.join(",")));
    }
    if let Some(g) = &filter.group_id {
        let ph = arg(&mut args, g.clone());
        sql.push_str(&format!(" AND p.group_id = {ph}"));
    }
    if filter.ungrouped == Some(true) {
        sql.push_str(" AND p.group_id IS NULL");
    }
    if let Some(tags) = &filter.tags {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let ph: Vec<String> = tags
            .iter()
            .map(|t| arg(&mut args, t.trim().to_string()))
            .collect();
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM page_tags t WHERE t.page_id = p.id AND t.tag IN ({}))",
            ph.join(",")
        ));
    }
    if let Some(s) = filter.check_status {
        let ph = arg(&mut args, enum_str(&s));
        sql.push_str(&format!(" AND p.check_status = {ph}"));
    }
    if filter.include_archived != Some(true) {
        sql.push_str(" AND p.archived = 0");
    }
    let sort = filter.sort.unwrap_or_default();
    let descending = filter
        .descending
        .unwrap_or(!matches!(sort, PageSortField::Title));
    let column = match sort {
        PageSortField::Updated => "p.updated_at",
        PageSortField::Created => "p.created_at",
        PageSortField::Title => "p.title COLLATE NOCASE",
    };
    let dir = if descending { "DESC" } else { "ASC" };
    sql.push_str(&format!(" ORDER BY {column} {dir}, p.id {dir}"));
    if let Some(n) = filter.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    let mut stmt = conn.prepare(&sql)?;
    let pages = stmt
        .query_map(params_from_iter(args), page_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    pages.into_iter().map(|p| summary(conn, p)).collect()
}

pub fn get_page(conn: &Connection, id: &str) -> Result<Page> {
    let mut p = conn
        .query_row(
            &format!("SELECT {PAGE_COLUMNS} FROM pages p WHERE p.id = ?1"),
            [id],
            page_from_row,
        )
        .optional()?
        .ok_or_else(|| page_not_found(id))?;
    p.tags = load_tags(conn, id)?;
    Ok(p)
}

fn find_page(conn: &Connection, id: &str) -> Result<Option<Page>> {
    match get_page(conn, id) {
        Ok(p) => Ok(Some(p)),
        Err(CoreError::NotFound { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// For rendering page mentions; unknown ids are skipped.
pub fn get_pages_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<PageSummary>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        if let Some(p) = find_page(conn, id)? {
            out.push(summary(conn, p)?);
        }
    }
    Ok(out)
}

/// Every tag in use, sorted, for autocomplete.
pub fn tags(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT DISTINCT tag FROM page_tags")?;
    let all = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut seen = HashSet::new();
    Ok(sort_tags(
        all.into_iter()
            .filter(|t| seen.insert(t.to_lowercase()))
            .collect(),
    ))
}

// ---------------------------------------------------------------- pages: changing

fn active_category(conn: &Connection, category_id: &str) -> Result<categories::Category> {
    let category = categories::get(conn, category_id)?;
    if category.status == categories::CategoryStatus::Archived {
        return Err(CoreError::Invalid(format!(
            "The category \"{}\" is archived. Restore it first, or choose another category.",
            category.name
        )));
    }
    Ok(category)
}

/// `Conflict` if another page of the category has this title (case-insensitive, FR-KN-ED-12).
fn check_title_free(
    conn: &Connection,
    category_id: &str,
    title: &str,
    except: Option<&str>,
) -> Result<()> {
    let lower = title.to_lowercase();
    let mut stmt = conn.prepare("SELECT id, title FROM pages WHERE category_id = ?1")?;
    let rows = stmt
        .query_map([category_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows
        .iter()
        .any(|(id, t)| Some(id.as_str()) != except && t.to_lowercase() == lower)
    {
        let category = categories::get(conn, category_id)?;
        return Err(CoreError::Conflict(format!(
            "The category \"{}\" already has a page titled \"{title}\". Choose another title.",
            category.name
        )));
    }
    Ok(())
}

/// The group must exist and belong to the page's category.
fn check_group(conn: &Connection, category_id: &str, group_id: &str) -> Result<()> {
    let g = get_group(conn, group_id)?;
    if g.category_id != category_id {
        return Err(CoreError::Invalid(
            "The group belongs to another category.".into(),
        ));
    }
    Ok(())
}

/// Writes the page's own columns and its tags.
fn write_page(conn: &Connection, p: &Page) -> Result<()> {
    conn.execute(
        "UPDATE pages SET category_id = ?2, group_id = ?3, title = ?4, body = ?5, source_info = ?6,
             check_status = ?7, archived = ?8, updated_at = ?9, path = ?10, file_hash = ?11
         WHERE id = ?1",
        params![
            p.id,
            p.category_id,
            p.group_id,
            p.title,
            p.body,
            p.source_info,
            enum_str(&p.check_status),
            p.archived,
            p.updated_at,
            p.path,
            p.file_hash,
        ],
    )?;
    write_tags(conn, &p.id, &p.tags)
}

fn write_tags(conn: &Connection, page_id: &str, tags: &[String]) -> Result<()> {
    conn.execute("DELETE FROM page_tags WHERE page_id = ?1", [page_id])?;
    for t in tags {
        conn.execute(
            "INSERT INTO page_tags (page_id, tag) VALUES (?1, ?2)",
            params![page_id, t],
        )?;
    }
    Ok(())
}

/// Today's version of the page: inserts or overwrites (FR-KN-ED-5).
fn save_version(conn: &Connection, p: &Page, st: &Stamp) -> Result<()> {
    let snapshot = PageSnapshot {
        title: p.title.clone(),
        body: p.body.clone(),
        tags: p.tags.clone(),
        group_id: p.group_id.clone(),
        source_info: p.source_info.clone(),
    };
    conn.execute(
        "INSERT INTO page_versions (page_id, day, snapshot) VALUES (?1, ?2, ?3)
         ON CONFLICT (page_id, day) DO UPDATE SET snapshot = excluded.snapshot",
        params![p.id, date_str(st.day), serde_json::to_string(&snapshot)?],
    )?;
    Ok(())
}

/// After a change to an existing page: write, version, activity, hooks.
fn commit_change(conn: &Connection, p: &mut Page, st: &Stamp) -> Result<()> {
    p.updated_at = st.at.clone();
    write_page(conn, p)?;
    save_version(conn, p, st)?;
    log_activity(conn, ActivityKind::PageUpdated, &p.id, st)?;
    hooks::page_saved(conn, p)
}

/// Title required and unique in the category; the group must belong to the same category;
/// the category must exist and not be archived.
pub fn create_page(db: &mut Db, input: NewPage) -> Result<Page> {
    db.tx(|tx| create_page_in(tx, input, &Stamp::now()))
}

pub(crate) fn create_page_in(conn: &Connection, input: NewPage, st: &Stamp) -> Result<Page> {
    create_page_as(conn, new_id(), input, st)
}

/// [`create_page_in`] with the id chosen by the caller (a folder import links its pages to each
/// other before they exist).
fn create_page_as(conn: &Connection, id: String, input: NewPage, st: &Stamp) -> Result<Page> {
    if let Some(ext) = external::active(conn)? {
        return external::pages::create_page(conn, &ext, id, input, st);
    }
    let title = clean_title(&input.title)?;
    active_category(conn, &input.category_id)?;
    check_title_free(conn, &input.category_id, &title, None)?;
    let group_id = match input.group_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(g) => {
            check_group(conn, &input.category_id, g)?;
            Some(g.to_string())
        }
    };
    let origin_task_id = match input.origin_task_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(t) => Some(tasks::get(conn, t)?.id),
    };
    let p = Page {
        id,
        category_id: input.category_id,
        group_id,
        title,
        body: input.body,
        tags: normalize_tags(input.tags.unwrap_or_default()),
        source_info: input.source_info.as_deref().and_then(clean_source),
        origin: input.origin.unwrap_or(PageOrigin::Manual),
        origin_task_id,
        check_status: input.check_status.unwrap_or(CheckStatus::NotChecked),
        archived: false,
        created_at: st.at.clone(),
        updated_at: st.at.clone(),
        path: None,
        file_hash: None,
    };
    insert_page(conn, &p, st)?;
    Ok(p)
}

/// Inserts a new page's row (with its file columns in external mode), its tags, today's version,
/// the activity and the derived indexes.
fn insert_page(conn: &Connection, p: &Page, st: &Stamp) -> Result<()> {
    conn.execute(
        "INSERT INTO pages (id, category_id, group_id, title, body, source_info, origin, origin_task_id,
                            check_status, archived, created_at, updated_at, path, file_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            p.id,
            p.category_id,
            p.group_id,
            p.title,
            p.body,
            p.source_info,
            enum_str(&p.origin),
            p.origin_task_id,
            enum_str(&p.check_status),
            p.archived,
            p.created_at,
            p.updated_at,
            p.path,
            p.file_hash,
        ],
    )?;
    write_tags(conn, &p.id, &p.tags)?;
    save_version(conn, p, st)?;
    log_activity(conn, ActivityKind::PageCreated, &p.id, st)?;
    hooks::page_saved(conn, p)
}

/// External mode: a page found in the folder (FR-KN-EXT-6), as [`insert_page`] with its file's
/// size and time.
pub(crate) fn insert_from_folder(
    conn: &Connection,
    p: &Page,
    file: &external::Stamp,
    st: &Stamp,
) -> Result<()> {
    insert_page(conn, p, st)?;
    external::store::set_file_stamp(conn, &p.id, file)
}

/// External mode: a page changed in the folder (its file edited or renamed outside Cairn): written
/// like any change (a daily version, activity, the derived indexes); `updated_at` is the file's
/// modification time.
pub(crate) fn commit_from_folder(
    conn: &Connection,
    p: &mut Page,
    file: &external::Stamp,
    st: &Stamp,
    modified: &str,
) -> Result<()> {
    p.updated_at = modified.to_string();
    write_page(conn, p)?;
    save_version(conn, p, st)?;
    log_activity(conn, ActivityKind::PageUpdated, &p.id, st)?;
    hooks::page_saved(conn, p)?;
    external::store::set_file_stamp(conn, &p.id, file)
}

pub fn update_page(db: &mut Db, id: &str, patch: PagePatch) -> Result<Page> {
    db.tx(|tx| update_page_in(tx, id, patch, &Stamp::now()))
}

pub(crate) fn update_page_in(
    conn: &Connection,
    id: &str,
    patch: PagePatch,
    st: &Stamp,
) -> Result<Page> {
    if let Some(ext) = external::active(conn)? {
        return external::pages::update_page(conn, &ext, id, patch, st);
    }
    let old = get_page(conn, id)?;
    let mut p = old.clone();
    if let Some(t) = patch.title {
        p.title = clean_title(&t)?;
    }
    if let Some(b) = patch.body {
        p.body = b;
    }
    if let Some(t) = patch.tags {
        p.tags = normalize_tags(t);
    }
    if let Some(s) = patch.source_info {
        p.source_info = clean_source(&s);
    }
    if let Some(g) = patch.group_id {
        p.group_id = match g.trim() {
            "" => None,
            g => Some(g.to_string()),
        };
    }
    if p == old {
        return Ok(old);
    }
    if p.title != old.title {
        check_title_free(conn, &p.category_id, &p.title, Some(&p.id))?;
    }
    if p.group_id != old.group_id {
        if let Some(g) = &p.group_id {
            check_group(conn, &p.category_id, g)?;
        }
    }
    commit_change(conn, &mut p, st)?;
    Ok(p)
}

/// Moves a page to another category; its group is cleared. Refused if the title exists there (FR-KN-ED-12).
pub fn move_page(db: &mut Db, id: &str, category_id: &str) -> Result<Page> {
    db.tx(|tx| move_page_in(tx, id, category_id, &Stamp::now()))
}

pub(crate) fn move_page_in(
    conn: &Connection,
    id: &str,
    category_id: &str,
    st: &Stamp,
) -> Result<Page> {
    if let Some(ext) = external::active(conn)? {
        return external::pages::move_page(conn, &ext, id, category_id, None, st);
    }
    let old = get_page(conn, id)?;
    if old.category_id == category_id {
        return Ok(old);
    }
    active_category(conn, category_id)?;
    check_title_free(conn, category_id, &old.title, Some(&old.id))?;
    let mut p = old;
    p.category_id = category_id.to_string();
    p.group_id = None;
    commit_change(conn, &mut p, st)?;
    Ok(p)
}

pub fn set_archived(db: &mut Db, id: &str, archived: bool) -> Result<Page> {
    db.tx(|tx| set_archived_in(tx, id, archived, &Stamp::now()))
}

fn set_archived_in(conn: &Connection, id: &str, archived: bool, st: &Stamp) -> Result<Page> {
    if let Some(ext) = external::active(conn)? {
        return external::pages::set_archived(conn, &ext, id, archived, st);
    }
    let old = get_page(conn, id)?;
    if old.archived == archived {
        return Ok(old);
    }
    let mut p = old;
    p.archived = archived;
    commit_change(conn, &mut p, st)?;
    Ok(p)
}

/// Used by the save pipeline and the check screens (simulated until M5).
pub fn set_check_status(db: &mut Db, id: &str, status: CheckStatus) -> Result<Page> {
    db.tx(|tx| set_check_status_in(tx, id, status))
}

fn set_check_status_in(conn: &Connection, id: &str, status: CheckStatus) -> Result<Page> {
    let mut p = get_page(conn, id)?;
    if p.check_status != status {
        conn.execute(
            "UPDATE pages SET check_status = ?2 WHERE id = ?1",
            params![id, enum_str(&status)],
        )?;
        p.check_status = status;
    }
    Ok(p)
}

/// Permanent: history, links, mentions and (unreferenced) images go too (FR-KN-ED-12, -11).
pub fn delete_page(db: &mut Db, id: &str) -> Result<()> {
    let candidates = db.tx(|tx| delete_page_in(tx, id))?;
    if let Err(e) = attachments::collect_among(db, &candidates) {
        tracing::warn!(error = %e, "could not clean up images after deleting a page");
    }
    Ok(())
}

/// Deletes the page's rows. Returns the images the page (or its history) used: only these are
/// candidates for cleaning up, so images pasted into an editor that isn't saved yet are never touched.
/// External mode: the file goes to the trash first (FR-KN-EXT-10).
fn delete_page_in(conn: &Connection, id: &str) -> Result<BTreeSet<String>> {
    if let Some(ext) = external::active(conn)? {
        return external::pages::delete_page(conn, &ext, id);
    }
    let p = get_page(conn, id)?;
    delete_page_rows(conn, &p)
}

/// The rows of a page and everything that hangs on them (not its file). Returns the
/// `attachment:` images the page (or its history) used.
pub(crate) fn delete_page_rows(conn: &Connection, p: &Page) -> Result<BTreeSet<String>> {
    let mut names = attachments::referenced_names(&p.body);
    let mut stmt = conn.prepare("SELECT snapshot FROM page_versions WHERE page_id = ?1")?;
    let snaps = stmt
        .query_map([&p.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for s in snaps {
        if let Ok(snap) = serde_json::from_str::<PageSnapshot>(&s) {
            names.extend(attachments::referenced_names(&snap.body));
        }
    }
    drop(stmt);
    hooks::page_deleted(conn, &p.id)?;
    conn.execute("DELETE FROM activity WHERE ref_id = ?1", [&p.id])?;
    conn.execute(
        "DELETE FROM drafts WHERE kind = 'page' AND ref_id = ?1",
        [&p.id],
    )?;
    // Tags, versions and task links go with it (ON DELETE CASCADE).
    conn.execute("DELETE FROM pages WHERE id = ?1", [&p.id])?;
    Ok(names)
}

// ---------------------------------------------------------------- many at once

fn bulk_name(conn: &Connection, id: &str) -> Option<String> {
    get_page(conn, id).ok().map(|p| p.title)
}

/// External mode (FR-KN-EXT-10): the file names `change` would give twice in one folder. Internal
/// mode has none (a page that can't take a change is skipped and named instead).
pub fn plan_bulk_change(conn: &Connection, ids: &[String], change: &PageBulkChange) -> Result<Vec<NameClash>> {
    match external::active(conn)? {
        Some(ext) => external::pages::name_clashes(conn, &ext, ids, change),
        None => Ok(Vec::new()),
    }
}

/// Applies one change to many pages (FR-KN-UI-3). A page that can't take it (e.g. its title is
/// used in the target category) is skipped and named; the others are changed. A move into an
/// archived category, a group of another category or an empty tag is refused as a whole.
/// External mode: a change that would give two files the same name in one folder is refused as a
/// whole (FR-KN-EXT-10; [`plan_bulk_change`] lists the clashes first).
pub fn update_many(db: &mut Db, ids: &[String], change: PageBulkChange) -> Result<BulkResult> {
    match &change {
        PageBulkChange::Move {
            category_id,
            group_id,
        } => {
            active_category(db.conn(), category_id)?;
            if let Some(g) = group_id {
                check_group(db.conn(), category_id, g)?;
            }
        }
        PageBulkChange::AddTag { tag } | PageBulkChange::RemoveTag { tag }
            if tag.trim().is_empty() =>
        {
            return Err(CoreError::Invalid("A tag can't be empty.".into()));
        }
        _ => {}
    }
    let st = Stamp::now();
    db.tx(|tx| {
        let clashes = plan_bulk_change(tx, ids, &change)?;
        if !clashes.is_empty() {
            return Err(external::pages::clash_error(&clashes));
        }
        crate::bulk::each(tx, ids, bulk_name, |c, id| change_in(c, id, &change, &st))
    })
}

fn change_in(conn: &Connection, id: &str, change: &PageBulkChange, st: &Stamp) -> Result<()> {
    if let PageBulkChange::Move {
        category_id,
        group_id,
    } = change
        && let Some(ext) = external::active(conn)?
    {
        // One move straight into the target folder (two steps could meet a name on the way).
        let group = Some(group_id.clone().unwrap_or_default());
        return external::pages::move_page(conn, &ext, id, category_id, group, st).map(|_| ());
    }
    let patch = match change {
        PageBulkChange::Archive => return set_archived_in(conn, id, true, st).map(|_| ()),
        PageBulkChange::Restore => return set_archived_in(conn, id, false, st).map(|_| ()),
        PageBulkChange::MarkChecked => {
            return set_check_status_in(conn, id, CheckStatus::Checked).map(|_| ());
        }
        PageBulkChange::Move {
            category_id,
            group_id,
        } => {
            move_page_in(conn, id, category_id, st)?;
            PagePatch {
                group_id: Some(group_id.clone().unwrap_or_default()),
                ..Default::default()
            }
        }
        PageBulkChange::AddTag { tag } => {
            let mut tags = get_page(conn, id)?.tags;
            tags.push(tag.clone());
            PagePatch {
                tags: Some(tags),
                ..Default::default()
            }
        }
        PageBulkChange::RemoveTag { tag } => {
            let gone = tag.trim().to_lowercase();
            let tags = get_page(conn, id)?.tags;
            PagePatch {
                tags: Some(
                    tags.into_iter()
                        .filter(|t| t.to_lowercase() != gone)
                        .collect(),
                ),
                ..Default::default()
            }
        }
    };
    update_page_in(conn, id, patch, st).map(|_| ())
}

/// Deletes many pages permanently, as `delete_page` does one (FR-KN-UI-3). External mode: their
/// files go to the trash.
pub fn delete_many(db: &mut Db, ids: &[String]) -> Result<BulkResult> {
    let mut candidates = BTreeSet::new();
    let out = db.tx(|tx| {
        crate::bulk::each(tx, ids, bulk_name, |c, id| {
            candidates.extend(delete_page_in(c, id)?);
            Ok(())
        })
    })?;
    if let Err(e) = attachments::collect_among(db, &candidates) {
        tracing::warn!(error = %e, "could not clean up images after deleting pages");
    }
    Ok(out)
}

// ---------------------------------------------------------------- history

/// Oldest first.
pub fn versions(conn: &Connection, page_id: &str) -> Result<Vec<PageVersion>> {
    let id = get_page(conn, page_id)?.id;
    let mut stmt =
        conn.prepare("SELECT day, snapshot FROM page_versions WHERE page_id = ?1 ORDER BY day")?;
    let rows = stmt
        .query_map([&id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(date, snapshot)| {
            Ok(PageVersion {
                page_id: id.clone(),
                date,
                snapshot: serde_json::from_str(&snapshot)?,
            })
        })
        .collect()
}

/// Restores the version of `date` as a new change, so nothing is lost (FR-KN-ED-9).
pub fn restore_version(db: &mut Db, page_id: &str, date: &str) -> Result<Page> {
    db.tx(|tx| restore_version_in(tx, page_id, date, &Stamp::now()))
}

pub(crate) fn restore_version_in(
    conn: &Connection,
    page_id: &str,
    date: &str,
    st: &Stamp,
) -> Result<Page> {
    let page = get_page(conn, page_id)?;
    let snapshot: String = conn
        .query_row(
            "SELECT snapshot FROM page_versions WHERE page_id = ?1 AND day = ?2",
            params![page_id, date],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| CoreError::NotFound {
            entity: "page version",
            id: date.to_string(),
        })?;
    let snap: PageSnapshot = serde_json::from_str(&snapshot)?;
    // The group may have been deleted, or the page moved to another category, since then.
    let group = match &snap.group_id {
        Some(g) if check_group(conn, &page.category_id, g).is_ok() => g.clone(),
        _ => String::new(),
    };
    update_page_in(
        conn,
        page_id,
        PagePatch {
            title: Some(snap.title),
            body: Some(snap.body),
            tags: Some(snap.tags),
            source_info: Some(snap.source_info.unwrap_or_default()),
            group_id: Some(group),
            base_hash: None,
        },
        st,
    )
}

// ---------------------------------------------------------------- task links (DM-5)

fn link_from_row(r: &Row) -> rusqlite::Result<TaskPageLink> {
    Ok(TaskPageLink {
        id: r.get(0)?,
        task_id: r.get(1)?,
        page_id: r.get(2)?,
        created_at: r.get(3)?,
    })
}

/// Links of a task, of a page, or both filters.
pub fn task_page_links(
    conn: &Connection,
    task_id: Option<&str>,
    page_id: Option<&str>,
) -> Result<Vec<TaskPageLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, page_id, created_at FROM task_page_links
         WHERE (?1 IS NULL OR task_id = ?1) AND (?2 IS NULL OR page_id = ?2)
         ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map(params![task_id, page_id], link_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Duplicates are refused with `Conflict`.
pub fn link_task_page(db: &mut Db, task_id: &str, page_id: &str) -> Result<TaskPageLink> {
    db.tx(|tx| {
        let task = tasks::get(tx, task_id)?;
        let page = get_page(tx, page_id)?;
        let exists = tx
            .query_row(
                "SELECT 1 FROM task_page_links WHERE task_id = ?1 AND page_id = ?2",
                params![task.id, page.id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            return Err(CoreError::Conflict(format!(
                "{} is already linked to \"{}\".",
                task.key, page.title
            )));
        }
        let l = TaskPageLink {
            id: new_id(),
            task_id: task.id,
            page_id: page.id,
            created_at: now_iso(),
        };
        tx.execute(
            "INSERT INTO task_page_links (id, task_id, page_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![l.id, l.task_id, l.page_id, l.created_at],
        )?;
        Ok(l)
    })
}

pub fn unlink_task_page(db: &mut Db, id: &str) -> Result<()> {
    db.tx(|tx| {
        let n = tx.execute("DELETE FROM task_page_links WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(CoreError::NotFound {
                entity: "link",
                id: id.to_string(),
            });
        }
        Ok(())
    })
}

// ---------------------------------------------------------------- export (TC-8a)

/// Writes the page as Markdown: front matter (title, category, group, tags, source info),
/// mentions as readable text (`PRX-12 Fix login`, page titles), images as relative files.
/// `path` is the `.md` file (Folder) or the `.zip` file (Zip).
pub fn export_markdown(
    db: &Db,
    page_id: &str,
    path: &std::path::Path,
    mode: MarkdownExportMode,
) -> Result<PageExport> {
    export::export_markdown(db, page_id, path, mode)
}

// ---------------------------------------------------------------- dev seed

/// Debug builds only: inserts sample groups, pages, versions and links where ids are missing;
/// never overwrites. Calls the page hooks so mentions and search include the samples.
/// External mode: does nothing (demo data is internal mode's, FR-KN-EXT-7). Before the mode is
/// chosen it seeds as internal mode does; choosing external mode is then refused (`mode`).
pub fn seed_if_missing(db: &mut Db, seed: KnowledgeSeed) -> Result<()> {
    if external::is_external(db.conn())? {
        tracing::info!("knowledge demo data skipped: knowledge is kept in a folder");
        return Ok(());
    }
    db.tx(|tx| seed::seed_if_missing(tx, seed))
}

// ---------------------------------------------------------------- images and the disk (D-72)

/// Stores a pasted image for a page of `category_id` (FR-KN-ED-3, FR-KN-EXT-3): in internal mode
/// in Cairn's data folder (`attachment:<name>`), in external mode in the category folder's
/// `attachments` folder (`folder:<path>`).
pub fn save_image(db: &mut Db, category_id: &str, bytes: &[u8], mime: &str) -> Result<SavedImage> {
    let Some(ext) = external::active(db.conn())? else {
        let a = attachments::save(db, bytes, mime)?;
        let file = db.paths().attachments_dir().join(&a.name);
        return Ok(SavedImage {
            target: format!("attachment:{}", a.name),
            file: file.to_string_lossy().into_owned(),
            name: a.name,
            mime: a.mime,
            size: a.size,
        });
    };
    let (extension, mime) = attachments::image_type(mime)?;
    if bytes.is_empty() {
        return Err(CoreError::Invalid("The image is empty.".into()));
    }
    let size = u32::try_from(bytes.len())
        .map_err(|_| CoreError::Invalid("The image is too large.".into()))?;
    let path = db.tx(|tx| {
        categories::get(tx, category_id)?;
        external::store::save_image(tx, &ext, category_id, bytes, extension)
    })?;
    Ok(SavedImage {
        target: format!("folder:{}", external::convert::encode_path(&path)),
        name: external::name_of(&path).to_string(),
        mime: mime.to_string(),
        size,
        file: ext.path(&path)?.to_string_lossy().into_owned(),
    })
}

/// External mode (FR-KN-EXT-11, *Show differences*): the page as its file says now, without
/// changing the index. `None` when its file is gone.
pub fn page_on_disk(db: &Db, id: &str) -> Result<Option<Page>> {
    let conn = db.conn();
    let page = get_page(conn, id)?;
    let (Some(ext), Some(path)) = (external::active(conn)?, page.path.clone()) else {
        return Err(CoreError::Invalid("This page is not kept in a file.".into()));
    };
    Ok(external::store::read_page_file(&ext, &path)?
        .map(|disk| external::store::page_from_disk(conn, &ext, &page, &disk)))
}

/// External mode (FR-KN-EXT-11, *Take the disk version*): reads the page's file into the index
/// now and returns the page.
pub fn reload_page(db: &mut Db, id: &str) -> Result<Page> {
    db.tx(|tx| {
        let ext = external::active(tx)?
            .ok_or_else(|| CoreError::Invalid("This page is not kept in a file.".into()))?;
        external::pages::refresh(tx, &ext, id, &Stamp::now())
    })
}
