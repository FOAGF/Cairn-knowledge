//! External mode's page and group changes (D-72): the counterparts of `knowledge`'s single-item
//! functions, which hand over to these when the database is in external mode.
//!
//! Each one brings the page up to its file first when the file changed outside Cairn since the
//! index last read it ([`current`]: the file is read into the index as a change of its own, never
//! overwritten blindly), applies the change, writes the file ([`store::write_page`]) and then the
//! row through the same helpers as internal mode (daily version, activity, hooks).
//!
//! Decisions: a title is the cleaned file name (FR-KN-EXT-15); titles are unique per folder, not
//! per category (FR-KN-EXT-10); a group's name is its folder's name, cleaned the same way;
//! deleting a group moves its pages and groups up one level and is refused as a whole when a name
//! would clash; a moved or renamed group's folder moves with everything in it. Changes of folders
//! write no page versions (the pages' text didn't change).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{Connection, params};

use super::convert;
use super::store::{self, NameOf, OnDisk};
use super::{Ext, clean_title as file_title, dir_of, join, name_of};
use crate::knowledge::{
    Group, GroupPatch, NameClash, NewGroup, NewPage, Page, PageBulkChange, PageOrigin, PagePatch,
    active_category, check_group, clean_group_name, clean_source, clean_title, commit_change,
    delete_page_rows, get_group, get_page, insert_page, normalize_tags,
};
use crate::tasks::{self, Stamp};
use crate::util::{new_id, now_iso};
use crate::{CoreError, Result, categories};

/// What a change does to the page's text, for the save-conflict rule of [`current`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Text<'a> {
    /// Only other fields change (tags, archive, title, place): an edit made outside is read in
    /// first and kept.
    Kept,
    /// The text is replaced: refused when the file differs from `base_hash`, or, without one,
    /// from the version the index has.
    Replaced { base_hash: Option<&'a str> },
}

fn changed_on_disk(title: &str, editing: bool) -> CoreError {
    CoreError::ChangedOnDisk(if editing {
        format!("\"{title}\" was changed in its file after you started editing.")
    } else {
        format!("\"{title}\" was changed in its file since Cairn last read it. Open it again to see the changes.")
    })
}

/// The page as the index has it, brought up to its file when the file changed outside Cairn since
/// the index last read it (written as a change, like a scan would). FR-KN-EXT-11: a change that
/// replaces the text is refused (`changed_on_disk`) when the file differs from its `base_hash`
/// (the editor's *Save*; *Keep mine* sends the disk version's hash from `page_on_disk`), or,
/// without one, from the index's (a history restore, a merge, the Assistant): text made from an
/// older version never overwrites an edit made outside. A file that is gone is refused only
/// with a `base_hash`; without one it is written again.
fn current(conn: &Connection, ext: &Ext, id: &str, text: Text, st: &Stamp) -> Result<(Page, Option<OnDisk>)> {
    let page = get_page(conn, id)?;
    let Some(path) = page.path.clone() else {
        return Ok((page, None));
    };
    ext.check()?;
    let disk = store::read_page_file(ext, &path)?;
    let on_disk = disk.as_ref().map(|d| d.hash.as_str());
    match text {
        Text::Replaced { base_hash: Some(base) } if on_disk != Some(base) => {
            return Err(changed_on_disk(&page.title, true));
        }
        Text::Replaced { base_hash: None } if on_disk.is_some() && on_disk != page.file_hash.as_deref() => {
            return Err(changed_on_disk(&page.title, false));
        }
        _ => {}
    }
    match &disk {
        Some(d) if page.file_hash.as_deref() != Some(d.hash.as_str()) => {
            Ok((store::take_from_disk(conn, ext, &page, d, st)?, disk))
        }
        _ => Ok((page, disk)),
    }
}

/// *Take the disk version*: the page's file read into the index now.
pub(crate) fn refresh(conn: &Connection, ext: &Ext, id: &str, st: &Stamp) -> Result<Page> {
    let page = get_page(conn, id)?;
    let path = page
        .path
        .clone()
        .ok_or_else(|| CoreError::Invalid("This page is not kept in a file.".into()))?;
    if store::read_page_file(ext, &path)?.is_none() {
        return Err(CoreError::Invalid(format!(
            "The file of \"{}\" is no longer in the folder.",
            page.title
        )));
    }
    Ok(current(conn, ext, id, Text::Kept, st)?.0)
}

/// Writes the file of `p` (changed from `old`) and then its row.
fn save(conn: &Connection, ext: &Ext, old: &Page, disk: Option<&OnDisk>, p: &mut Page, st: &Stamp) -> Result<()> {
    let stamp = store::write_page(conn, ext, Some(old), disk, p)?;
    commit_change(conn, p, st)?;
    store::set_file_stamp(conn, &p.id, &stamp)?;
    // FR-KN-EXT-12: images the old text used and the new one doesn't, once this is committed.
    let now: BTreeSet<String> = convert::folder_paths(&p.body).into_iter().collect();
    let dropped: Vec<String> = convert::folder_paths(&old.body)
        .into_iter()
        .filter(|f| !now.contains(f))
        .collect();
    super::journal::drop_images_after_commit(conn, ext, dropped);
    Ok(())
}

pub(crate) fn create_page(conn: &Connection, ext: &Ext, id: String, input: NewPage, st: &Stamp) -> Result<Page> {
    let title = file_title(&clean_title(&input.title)?);
    active_category(conn, &input.category_id)?;
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
    let mut p = Page {
        id,
        category_id: input.category_id,
        group_id,
        title,
        body: input.body,
        tags: normalize_tags(input.tags.unwrap_or_default()),
        source_info: input.source_info.as_deref().and_then(clean_source),
        origin: input.origin.unwrap_or(PageOrigin::Manual),
        origin_task_id,
        check_status: input.check_status.unwrap_or(crate::knowledge::CheckStatus::NotChecked),
        archived: false,
        created_at: st.at.clone(),
        updated_at: st.at.clone(),
        path: None,
        file_hash: None,
    };
    let stamp = store::write_page(conn, ext, None, None, &mut p)?;
    insert_page(conn, &p, st)?;
    store::set_file_stamp(conn, &p.id, &stamp)?;
    // Links elsewhere that would now find this page instead of another keep the other one.
    let shift = store::Shift {
        added: p.path.clone(),
        ..Default::default()
    };
    store::relink(conn, ext, &shift, Some(&p.id))?;
    Ok(p)
}

pub(crate) fn update_page(conn: &Connection, ext: &Ext, id: &str, patch: PagePatch, st: &Stamp) -> Result<Page> {
    let text = match &patch.body {
        Some(_) => Text::Replaced {
            base_hash: patch.base_hash.as_deref(),
        },
        None => Text::Kept,
    };
    let (old, disk) = current(conn, ext, id, text, st)?;
    let mut p = old.clone();
    // A title another tool gave (not a clean file name) stays until the user changes it.
    if let Some(t) = patch.title {
        let t = clean_title(&t)?;
        if t != old.title {
            p.title = file_title(&t);
        }
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
    if p.group_id != old.group_id
        && let Some(g) = &p.group_id
    {
        check_group(conn, &p.category_id, g)?;
    }
    save(conn, ext, &old, disk.as_ref(), &mut p, st)?;
    Ok(p)
}

/// To another category (`group`: `None` = no group, as `move_page`), or with `group` given
/// (`""` = none) straight into that group (the bulk bar's *Move*).
pub(crate) fn move_page(
    conn: &Connection,
    ext: &Ext,
    id: &str,
    category_id: &str,
    group: Option<String>,
    st: &Stamp,
) -> Result<Page> {
    let page = get_page(conn, id)?;
    if group.is_none() && page.category_id == category_id {
        return Ok(page);
    }
    let group_id = group
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_string);
    active_category(conn, category_id)?;
    if let Some(g) = &group_id {
        check_group(conn, category_id, g)?;
    }
    let (old, disk) = current(conn, ext, id, Text::Kept, st)?;
    let mut p = old.clone();
    p.category_id = category_id.to_string();
    p.group_id = group_id;
    if p == old {
        return Ok(old);
    }
    save(conn, ext, &old, disk.as_ref(), &mut p, st)?;
    Ok(p)
}

/// FR-KN-EXT-10: `archived: true` in the front matter; the file stays where it is.
pub(crate) fn set_archived(conn: &Connection, ext: &Ext, id: &str, archived: bool, st: &Stamp) -> Result<Page> {
    let (old, disk) = current(conn, ext, id, Text::Kept, st)?;
    if old.archived == archived {
        return Ok(old);
    }
    let mut p = old.clone();
    p.archived = archived;
    save(conn, ext, &old, disk.as_ref(), &mut p, st)?;
    Ok(p)
}

/// FR-KN-EXT-10: the file goes to the system trash, then the rows. Returns the `attachment:`
/// images the page used (for internal mode's clean-up; normally none here).
pub(crate) fn delete_page(conn: &Connection, ext: &Ext, id: &str) -> Result<BTreeSet<String>> {
    let page = get_page(conn, id)?;
    let mut used = convert::folder_paths(&page.body);
    let mut stmt = conn.prepare("SELECT snapshot FROM page_versions WHERE page_id = ?1")?;
    let snaps = stmt
        .query_map([&page.id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for s in snaps {
        if let Ok(snap) = serde_json::from_str::<crate::knowledge::PageSnapshot>(&s) {
            used.extend(convert::folder_paths(&snap.body));
        }
    }
    if page.path.is_some() {
        ext.check()?;
    }
    let linking = match &page.path {
        Some(path) => store::linking_to(conn, &page.id, path)?,
        None => store::referrers(conn, std::slice::from_ref(&page.id))?,
    };
    store::delete_page_file(ext, &page)?;
    let names = delete_page_rows(conn, &page)?;
    // The pages that linked to it find what their files say now, as after a removal outside.
    store::reread(conn, ext, linking)?;
    super::journal::drop_images_after_commit(conn, ext, used);
    Ok(names)
}

// ---------------------------------------------------------------- groups

pub(crate) fn create_group(conn: &Connection, ext: &Ext, input: NewGroup) -> Result<Group> {
    let name = super::clean_name(&clean_group_name(&input.name)?, "Group");
    categories::get(conn, &input.category_id)?;
    let parent = match input.parent_id.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(p) => {
            let g = get_group(conn, p)?;
            if g.category_id != input.category_id {
                return Err(CoreError::Invalid("The group belongs to another category.".into()));
            }
            Some(g)
        }
    };
    let path = store::new_group_dir(conn, ext, &input.category_id, parent.as_ref(), &name)?;
    let now = now_iso();
    let g = Group {
        id: new_id(),
        category_id: input.category_id,
        parent_id: parent.map(|p| p.id),
        name,
        description: input.description.unwrap_or_default().trim().to_string(),
        path: Some(path),
        created_at: now.clone(),
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO groups (id, category_id, parent_id, name, description, path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![g.id, g.category_id, g.parent_id, g.name, g.description, g.path, g.created_at, g.updated_at],
    )?;
    Ok(g)
}

/// The folder a group's contents move into: its parent group's, or its category's.
fn parent_dir(conn: &Connection, ext: &Ext, category_id: &str, parent: Option<&Group>) -> Result<String> {
    match parent {
        Some(g) => g
            .path
            .clone()
            .ok_or_else(|| CoreError::Invalid("This group has no folder in the knowledge folder.".into())),
        None => store::category_dir(conn, ext, category_id, true),
    }
}

/// Renames a group (its folder), changes its description, or moves it into another group of the
/// same category (`parent_id`, `""` = directly in the category).
pub(crate) fn update_group(conn: &Connection, ext: &Ext, id: &str, patch: GroupPatch) -> Result<Group> {
    let old = get_group(conn, id)?;
    let mut g = old.clone();
    if let Some(n) = patch.name {
        let n = clean_group_name(&n)?;
        if n != old.name {
            g.name = super::clean_name(&n, "Group");
        }
    }
    if let Some(d) = patch.description {
        g.description = d.trim().to_string();
    }
    if let Some(p) = patch.parent_id {
        g.parent_id = match p.trim() {
            "" => None,
            p => Some(p.to_string()),
        };
    }
    if g == old {
        return Ok(old);
    }
    if g.name != old.name || g.parent_id != old.parent_id {
        let parent = match &g.parent_id {
            Some(p) => {
                let pg = get_group(conn, p)?;
                if pg.category_id != g.category_id {
                    return Err(CoreError::Invalid("The group belongs to another category.".into()));
                }
                if pg.id == g.id || old.path.as_deref().is_some_and(|op| pg.path.as_deref().is_some_and(|pp| super::is_within(pp, op))) {
                    return Err(CoreError::Invalid("A group can't go inside itself.".into()));
                }
                Some(pg)
            }
            None => None,
        };
        let old_path = old
            .path
            .clone()
            .ok_or_else(|| CoreError::Invalid("This group has no folder in the knowledge folder.".into()))?;
        if g.name.eq_ignore_ascii_case("attachments") {
            return Err(CoreError::Invalid(
                "\"attachments\" is the folder for a category's images. Choose another name.".into(),
            ));
        }
        let dir = parent_dir(conn, ext, &g.category_id, parent.as_ref())?;
        let new_path = join(&dir, &g.name);
        if new_path != old_path {
            store::check_name_free(conn, ext, &dir, &g.name, Some(&old_path), NameOf::Folder)?;
            store::move_dir(conn, ext, &old_path, &new_path)?;
        }
        g.path = Some(new_path);
    }
    g.updated_at = now_iso();
    conn.execute(
        "UPDATE groups SET name = ?2, description = ?3, parent_id = ?4, path = ?5, updated_at = ?6 WHERE id = ?1",
        params![g.id, g.name, g.description, g.parent_id, g.path, g.updated_at],
    )?;
    Ok(g)
}

/// Deletes a group: its pages and groups move up one level (refused as a whole when a name would
/// clash there), then its folder goes if nothing else is in it (D-72 contract, "Folders").
pub(crate) fn delete_group(conn: &Connection, ext: &Ext, id: &str, _st: &Stamp) -> Result<()> {
    let g = get_group(conn, id)?;
    let Some(path) = g.path.clone() else {
        conn.execute("DELETE FROM groups WHERE id = ?1", [id])?;
        return Ok(());
    };
    ext.check()?;
    let parent = match &g.parent_id {
        Some(p) => Some(get_group(conn, p)?),
        None => None,
    };
    let up = parent_dir(conn, ext, &g.category_id, parent.as_ref())?;
    let mut stmt = conn.prepare("SELECT id, path FROM pages WHERE group_id = ?1 AND path IS NOT NULL")?;
    let pages = stmt
        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare("SELECT id, path FROM groups WHERE parent_id = ?1 AND path IS NOT NULL")?;
    let subs = stmt
        .query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    // Every name that moves up must be free there, and unique among themselves.
    let mut clashes: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for (_, p) in pages.iter().chain(subs.iter()) {
        let name = name_of(p);
        let what = if pages.iter().any(|(_, q)| q == p) { NameOf::Page } else { NameOf::Folder };
        if !seen.insert(name.to_lowercase()) || store::check_name_free(conn, ext, &up, name, Some(p), what).is_err() {
            clashes.push(name.to_string());
        }
    }
    if !clashes.is_empty() {
        let place = if up.is_empty() { "the knowledge folder".to_string() } else { format!("\"{up}\"") };
        return Err(CoreError::Conflict(format!(
            "The group \"{}\" can't be deleted: {place} already has {}. Rename or move them first.",
            g.name,
            clashes.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ")
        )));
    }

    let mut moves: Vec<(String, String)> = Vec::new();
    for (page_id, p) in &pages {
        let to = join(&up, name_of(p));
        let from_abs = ext.writable(p)?;
        if super::exists_exact(&from_abs) {
            super::move_path(&from_abs, &ext.writable(&to)?)?;
        }
        conn.execute(
            "UPDATE pages SET path = ?2, group_id = ?3 WHERE id = ?1",
            params![page_id, to, g.parent_id],
        )?;
        moves.push((p.clone(), to));
    }
    for (sub_id, p) in &subs {
        let to = join(&up, name_of(p));
        let from_abs = ext.writable(p)?;
        if super::exists_exact(&from_abs) {
            super::move_path(&from_abs, &ext.writable(&to)?)?;
        }
        store::rebase_rows(conn, p, &to)?;
        conn.execute(
            "UPDATE groups SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![sub_id, g.parent_id, now_iso()],
        )?;
        moves.push((p.clone(), to));
    }
    let shift = store::Shift {
        moves,
        ..Default::default()
    };
    store::relink(conn, ext, &shift, None)?;
    conn.execute("DELETE FROM groups WHERE id = ?1", [id])?;
    let abs = ext.writable(&path)?;
    if abs.is_dir() {
        super::remove_dir_if_empty(&abs)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- the bulk bar

/// FR-KN-EXT-10: the names a bulk *Move* would give twice in its target folder (other changes
/// keep every file where it is).
pub(crate) fn name_clashes(conn: &Connection, ext: &Ext, ids: &[String], change: &PageBulkChange) -> Result<Vec<NameClash>> {
    let PageBulkChange::Move {
        category_id,
        group_id,
    } = change
    else {
        return Ok(Vec::new());
    };
    let dir = store::target_dir(conn, ext, category_id, group_id.as_deref(), false)?;
    // By title (the file name without `.md` / `.markdown`, ignoring case): the pages of the change
    // that end up with it; `X.md` and `X.markdown` clash too.
    let mut by_name: BTreeMap<String, (String, Vec<Page>)> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let Ok(p) = get_page(conn, id) else { continue };
        let name = match &p.path {
            Some(path) => name_of(path).to_string(),
            None => format!("{}.md", file_title(&p.title)),
        };
        by_name
            .entry(super::title_of(&name).to_lowercase())
            .or_insert_with(|| (name.clone(), Vec::new()))
            .1
            .push(p);
    }
    let mut out = Vec::new();
    for (name, pages) in by_name.into_values() {
        let arriving: Vec<&Page> = pages
            .iter()
            .filter(|p| p.path.as_deref().is_none_or(|path| dir_of(path) != dir))
            .collect();
        if arriving.is_empty() {
            continue;
        }
        // A file already there that isn't one of the moving pages.
        let staying: Vec<&str> = pages
            .iter()
            .filter_map(|p| p.path.as_deref())
            .filter(|path| dir_of(path) == dir)
            .collect();
        let taken = pages.len() > 1
            || store::check_name_free(conn, ext, &dir, &name, staying.first().copied(), NameOf::Page).is_err();
        if taken {
            out.push(NameClash {
                folder: dir.clone(),
                name,
                page_ids: pages.iter().map(|p| p.id.clone()).collect(),
                titles: pages.iter().map(|p| p.title.clone()).collect(),
            });
        }
    }
    Ok(out)
}

/// The refusal of a bulk change with name clashes.
pub(crate) fn clash_error(clashes: &[NameClash]) -> CoreError {
    let names: Vec<String> = clashes.iter().map(|c| format!("\"{}\"", c.name)).collect();
    CoreError::Conflict(format!(
        "Nothing was changed: the move would give two files the same name ({}) in one folder.",
        names.join(", ")
    ))
}
