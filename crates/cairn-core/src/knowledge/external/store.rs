//! Writing the folder (FR-KN-EXT-3, -5, -10, -12, -15; D-72 contract, "Writing keeps the user's file").
//!
//! The single-item functions of `knowledge` call these in external mode, inside their
//! transaction, before they write the row: a failed write fails the change, so the row never
//! claims what the file doesn't have.
//!
//! - **Where a file goes:** a page of a group is in the group's folder; an ungrouped page in its
//!   category's folder (made, and recorded in `categories.folder`, at the first page or group).
//!   A page whose category and group don't change stays where it is (General's pages in the root).
//! - **Names:** Cairn's own names follow Windows' rules on every system (`clean_title`), and a
//!   name that matches another in the same folder ignoring case is refused.
//! - **Content:** when a page's text (in the index's form) didn't change, its file keeps its own
//!   body byte for byte and only the front matter is rewritten (tags, source, archived); a file
//!   that moved gets just its relative links adjusted. A changed text is written whole from the
//!   index, and the index then takes the file's text read back (an `attachment:` image is copied
//!   into the category's `attachments` folder on the way, and a link to a page without a file
//!   becomes its text).
//! - **Other files:** after a page or folder moves, only the links that point into it are
//!   replaced in other files ([`relink`]), found from the index's own forms (`page:<id>`,
//!   `folder:<path>`), and those files' index rows are read back from what was written. A link
//!   keeps finding the page it found: when a page is made, renamed or moved and a `[[Name]]` link
//!   would now find it instead of the page it found before, the link names that page's place
//!   (`[[Far/Name]]`). Links that found nothing are read again, as they may find the page now.
//! - **Links the files no longer name:** a page renamed or moved outside Cairn keeps the index's
//!   links to it while the linking files still name its old place ([`still_meant`]); the new place
//!   is written when a linking page's text is next saved in Cairn.
//! - **Images** no page uses any more are moved to the trash, only inside `attachments` folders.

use std::collections::{BTreeSet, HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

use super::convert::{self, Lookup, Meta, Parsed, Places};
use super::{
    Ext, Stamp, clean_title, dir_of, fold, hash_bytes, join, make_dir, name_of, page_extension,
    rebase, stamp_of, title_key, title_of,
};
use crate::knowledge::{Group, Page};
use crate::util::now_iso;
use crate::{CoreError, Result, categories, hooks};

// ---------------------------------------------------------------- places

/// The category whose folder is `folder`.
pub(crate) fn category_of_folder(conn: &Connection, folder: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT id FROM categories WHERE folder = ?1", [folder], |r| r.get(0))
        .optional()?)
}

fn general_id(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT id FROM categories WHERE is_general = 1", [], |r| r.get(0))?)
}

/// FR-KN-EXT-5: the folder `folder` goes to a category without a folder whose name (or cleaned
/// name) is the folder's, ignoring case, when there is one. Whether one took it.
pub(crate) fn adopt_category_for_folder(conn: &Connection, folder: &str) -> Result<bool> {
    let mut stmt = conn.prepare(
        "SELECT id, name FROM categories WHERE folder IS NULL ORDER BY is_general DESC, created_at, id",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let lower = folder.to_lowercase();
    let Some((id, _)) = rows.iter().find(|(_, name)| {
        name.to_lowercase() == lower || super::clean_name(name, "Category").to_lowercase() == lower
    }) else {
        return Ok(false);
    };
    conn.execute(
        "UPDATE categories SET folder = ?2 WHERE id = ?1 AND folder IS NULL",
        params![id, folder],
    )?;
    Ok(true)
}

/// FR-KN-EXT-5: a new folder in the root becomes a category: its name, a key from
/// `categories::suggest_key` and the next colour.
pub(crate) fn new_category_for_folder(conn: &Connection, folder: &str) -> Result<String> {
    let key = categories::suggest_key(conn, folder)?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
    let color = categories::CategoryColor::PRESETS[n as usize % categories::CategoryColor::PRESETS.len()];
    let category = categories::create(
        conn,
        categories::NewCategory {
            name: folder.to_string(),
            key: if key.is_empty() { "PRJ".into() } else { key },
            description: String::new(),
            color: categories::CategoryColor::preset(color),
        },
    )?;
    conn.execute(
        "UPDATE categories SET folder = ?2 WHERE id = ?1",
        params![category.id, folder],
    )?;
    Ok(category.id)
}

/// The group whose folder is `dir`.
pub(crate) fn group_of_dir(conn: &Connection, dir: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT id FROM groups WHERE path = ?1", [dir], |r| r.get(0))
        .optional()?)
}

/// The category and parent group of the group folder `dir` (inside a category folder).
pub(crate) fn place_of_dir(conn: &Connection, dir: &str) -> Result<(Option<String>, Option<String>)> {
    let top = dir.split('/').next().unwrap_or("");
    let category = category_of_folder(conn, top)?;
    let parent_dir = dir_of(dir);
    let parent = if parent_dir.contains('/') {
        group_of_dir(conn, parent_dir)?
    } else {
        None
    };
    Ok((category, parent))
}

/// Makes the group rows for the folder `dir` and the folders above it inside its category (a
/// folder with Markdown below it). `None` when the category folder isn't a category.
pub(crate) fn ensure_group_rows(conn: &Connection, dir: &str) -> Result<Option<String>> {
    if !dir.contains('/') {
        return Ok(None);
    }
    if let Some(id) = group_of_dir(conn, dir)? {
        return Ok(Some(id));
    }
    let parent = dir_of(dir);
    let parent_id = if parent.contains('/') {
        match ensure_group_rows(conn, parent)? {
            Some(id) => Some(id),
            None => return Ok(None),
        }
    } else {
        None
    };
    let Some(category_id) = category_of_folder(conn, dir.split('/').next().unwrap_or(""))? else {
        return Ok(None);
    };
    let id = crate::util::new_id();
    let now = now_iso();
    conn.execute(
        "INSERT INTO groups (id, category_id, parent_id, name, description, path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, '', ?5, ?6, ?6)",
        params![id, category_id, parent_id, name_of(dir), dir, now],
    )?;
    Ok(Some(id))
}

/// The category and group of the page file at `path` (FR-KN-EXT-2): its top folder's category
/// and its folder's group; a file directly in the root belongs to General.
pub(crate) fn place_of_file(conn: &Connection, path: &str) -> Result<(Option<String>, Option<String>)> {
    if !path.contains('/') {
        return Ok((Some(general_id(conn)?), None));
    }
    let top = path.split('/').next().unwrap_or("");
    let Some(category) = category_of_folder(conn, top)? else {
        return Ok((None, None));
    };
    let dir = dir_of(path);
    let group = if dir.contains('/') {
        ensure_group_rows(conn, dir)?
    } else {
        None
    };
    Ok((Some(category), group))
}

pub(crate) fn set_file_stamp(conn: &Connection, id: &str, stamp: &Stamp) -> Result<()> {
    conn.execute(
        "UPDATE pages SET file_size = ?2, file_mtime = ?3 WHERE id = ?1",
        params![id, stamp.size, stamp.mtime],
    )?;
    Ok(())
}

/// The folder of a category, relative to the root. With `make`, a category without one gets it:
/// a folder already there with its (cleaned) name and no category, or a new one (FR-KN-EXT-5).
pub(crate) fn category_dir(conn: &Connection, ext: &Ext, category_id: &str, make: bool) -> Result<String> {
    let c = categories::get(conn, category_id)?;
    if let Some(folder) = c.folder {
        if make {
            make_dir(&ext.writable(&folder)?)?;
        }
        return Ok(folder);
    }
    let base = super::clean_name(&c.name, "Category");
    let base = if base.eq_ignore_ascii_case("attachments") {
        format!("{base} (category)")
    } else {
        base
    };
    let taken = |name: &str| -> Result<bool> {
        let used: Option<String> = conn
            .query_row(
                "SELECT folder FROM categories WHERE folder IS NOT NULL AND lower(folder) = lower(?1)",
                [name],
                |r| r.get(0),
            )
            .optional()?;
        Ok(used.is_some())
    };
    // A real folder of that name already in the root (any case) is taken over; never a link to
    // another place, nor a hidden one.
    let existing = std::fs::read_dir(&ext.root).ok().and_then(|entries| {
        entries.flatten().find_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let real_dir = e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink());
            (real_dir && !super::is_hidden(&n) && n.to_lowercase() == base.to_lowercase()).then_some(n)
        })
    });
    let mut folder = match existing {
        Some(n) if !taken(&n)? => n,
        _ => base.clone(),
    };
    // A name another category uses (or a file or link in the root) gets a number.
    let occupied = |f: &str| {
        ext.abs(f)
            .and_then(|a| std::fs::symlink_metadata(a).ok())
            .is_some_and(|m| !m.is_dir())
    };
    let mut n = 2;
    while taken(&folder)? || occupied(&folder) {
        folder = format!("{base} {n}");
        n += 1;
    }
    if !make {
        return Ok(folder);
    }
    make_dir(&ext.writable(&folder)?)?;
    conn.execute(
        "UPDATE categories SET folder = ?2 WHERE id = ?1",
        params![category_id, folder],
    )?;
    Ok(folder)
}

fn group_path(conn: &Connection, group_id: &str) -> Result<String> {
    let path: Option<String> = conn
        .query_row("SELECT path FROM groups WHERE id = ?1", [group_id], |r| r.get(0))
        .optional()?
        .flatten();
    path.ok_or_else(|| CoreError::Invalid("This group has no folder in the knowledge folder.".into()))
}

/// The folder a page of this category and group goes into.
pub(crate) fn target_dir(
    conn: &Connection,
    ext: &Ext,
    category_id: &str,
    group_id: Option<&str>,
    make: bool,
) -> Result<String> {
    match group_id {
        Some(g) => {
            let dir = group_path(conn, g)?;
            if make {
                make_dir(&ext.writable(&dir)?)?;
            }
            Ok(dir)
        }
        None => category_dir(conn, ext, category_id, make),
    }
}

/// What a name check is about, for its message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameOf {
    Page,
    Folder,
}

/// FR-KN-EXT-10, -15: `name` must not match another file or folder in `dir` ignoring case (the
/// one at `except` aside).
pub(crate) fn check_name_free(
    conn: &Connection,
    ext: &Ext,
    dir: &str,
    name: &str,
    except: Option<&str>,
    what: NameOf,
) -> Result<()> {
    let lower = name.to_lowercase();
    // A page also clashes with a page file of the same title (`X.md` and `X.markdown`).
    let title = (what == NameOf::Page).then(|| title_of(name).to_lowercase());
    let clash = |other: &str| {
        let same = other.to_lowercase() == lower
            || title.as_ref().is_some_and(|t| {
                page_extension(other).is_some() && title_of(other).to_lowercase() == *t
            });
        same && Some(join(dir, other).as_str()) != except
    };
    // The name already there (on disk, else in the index), as it is written there.
    let on_disk = std::fs::read_dir(ext.path(dir)?).ok().and_then(|entries| {
        entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| clash(n))
    });
    let wanted = join(dir, name);
    let mut existing = on_disk;
    for table in ["pages", "groups"] {
        if existing.is_some() {
            break;
        }
        let mut stmt = conn.prepare(&format!(
            "SELECT path FROM {table} WHERE path IS NOT NULL AND length(path) = length(?1)"
        ))?;
        let paths = stmt
            .query_map([&wanted], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        existing = paths
            .iter()
            .find(|p| dir_of(p) == dir && clash(name_of(p)))
            .map(|p| name_of(p).to_string());
    }
    if let Some(other) = existing {
        let place = if dir.is_empty() { "The knowledge folder".to_string() } else { format!("The folder \"{dir}\"") };
        return Err(CoreError::Conflict(match what {
            NameOf::Page if page_extension(&other).is_some() => format!(
                "{place} already has a page named \"{}\". Choose another title.",
                title_of(&other)
            ),
            NameOf::Page => format!("{place} already has something named \"{other}\". Choose another title."),
            NameOf::Folder => format!("{place} already has something named \"{other}\". Choose another name."),
        }));
    }
    Ok(())
}

// ---------------------------------------------------------------- the index as a lookup

// Count full namespace reads in regression tests, without timing-sensitive assertions.
#[cfg(test)]
thread_local! {
    pub(super) static STEM_LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The index (and the disk) as [`convert`] needs it while writing.
pub(crate) struct DbFolder<'a> {
    conn: &'a Connection,
    ext: &'a Ext,
    /// The page being written: its category's `attachments` folder receives copied images.
    category_id: Option<String>,
    stems: std::cell::OnceCell<HashMap<String, Vec<String>>>,
    /// The folder's other files by name, read when an embed is found nowhere nearer.
    files: std::cell::OnceCell<HashMap<String, Vec<String>>>,
    /// Images copied by this write.
    pub copied: Vec<String>,
}

impl<'a> DbFolder<'a> {
    pub(crate) fn new(conn: &'a Connection, ext: &'a Ext, category_id: Option<&str>) -> Self {
        DbFolder {
            conn,
            ext,
            category_id: category_id.map(str::to_string),
            stems: std::cell::OnceCell::new(),
            files: std::cell::OnceCell::new(),
            copied: Vec::new(),
        }
    }

    fn stems(&self) -> &HashMap<String, Vec<String>> {
        self.stems.get_or_init(|| {
            #[cfg(test)]
            STEM_LOADS.with(|loads| loads.set(loads.get() + 1));
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            if let Ok(mut stmt) = self.conn.prepare("SELECT path FROM pages WHERE path IS NOT NULL")
                && let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0))
            {
                for p in rows.flatten() {
                    add_stem(&mut map, &p);
                }
            }
            map
        })
    }
}

/// `path` among the page paths by title ([`title_key`]), as [`wiki_page_path`] looks them up.
pub(super) fn add_stem(stems: &mut HashMap<String, Vec<String>>, path: &str) {
    stems.entry(title_key(path)).or_default().push(path.to_string());
}

/// The best of several files with one name, seen from `here`: same folder, then same top folder,
/// then the shortest path (a `.md` page before a `.markdown` one at the same place).
pub(super) fn nearest<'p>(candidates: &'p [String], here: &str) -> Option<&'p String> {
    let dir = dir_of(here);
    let top = here.split('/').next().unwrap_or("");
    candidates.iter().min_by_key(|p| {
        (
            dir_of(p) != dir,
            p.split('/').next().unwrap_or("") != top,
            p.matches('/').count(),
            page_extension(name_of(p)).is_some_and(|e| e.eq_ignore_ascii_case("markdown")),
            (*p).clone(),
        )
    })
}

/// `path` without its page extension.
fn without_extension(path: &str) -> &str {
    &path[..path.len() - page_extension(name_of(path)).map_or(0, |e| e.len() + 1)]
}

/// The page file a `[[name]]` link in the page at `here` finds (`stems`: the page paths by
/// [`add_stem`]), ignoring case, also in the extension. A plain name finds the nearest page with
/// that title ([`nearest`]); a name with `/` is a path: from the linking page's folder when it
/// starts with `./` or `../`, else from the root, else the nearest page whose path ends with it
/// (`[[Pumps/Pump check]]`). A path never falls back to another folder's page of that title.
pub(super) fn wiki_page_path(stems: &HashMap<String, Vec<String>>, name: &str, here: &str) -> Option<String> {
    let candidates = stems.get(&title_key(name))?;
    let with_extension = page_extension(name_of(name)).is_some();
    // A page's path as the link would write it: folded, with an extension only if the link has one.
    let form = |p: &str| fold(if with_extension { p } else { without_extension(p) });
    let pick = |hit: &dyn Fn(&str) -> bool| -> Option<String> {
        let found: Vec<String> = candidates.iter().filter(|p| hit(&form(p))).cloned().collect();
        nearest(&found, here).cloned()
    };
    if !name.contains('/') {
        let want = fold(name);
        return pick(&|f| name_of(f) == want);
    }
    let parts: Vec<&str> = name.split('/').collect();
    if matches!(parts[0], "." | "..") {
        let mut at: Vec<&str> = dir_of(here).split('/').filter(|p| !p.is_empty()).collect();
        for part in parts {
            match part {
                "" | "." => {}
                ".." => {
                    at.pop()?;
                }
                p => at.push(p),
            }
        }
        let want = fold(&at.join("/"));
        return pick(&|f| f == want);
    }
    let want = fold(name.trim_start_matches('/'));
    let tail = format!("/{want}");
    pick(&|f| f == want).or_else(|| pick(&|f| f.ends_with(&tail)))
}

/// The file an embed `![[name]]` in the page at `here` shows: the one next to the page, else in
/// its category's `attachments` folder, else the nearest file of that name anywhere in the folder
/// (`named`: the files with that name, ignoring case), as Obsidian finds them.
pub(super) fn embed_path(
    name: &str,
    here: &str,
    exists: &dyn Fn(&str) -> bool,
    named: &dyn Fn(&str) -> Vec<String>,
) -> Option<String> {
    // Resolved like a link, so a name can't climb out of the root.
    if let Some((near, None)) = convert::resolve(dir_of(here), name)
        && exists(&near)
    {
        return Some(near);
    }
    let top = here.split('/').next().unwrap_or("");
    if here.contains('/')
        && let Some((attached, None)) = convert::resolve(&join(top, "attachments"), name)
        && exists(&attached)
    {
        return Some(attached);
    }
    nearest(&named(name), here).cloned()
}

/// FR-KN-EXT-16: whether a link the index had to the page `was` keeps it although the linking
/// file's text (unchanged since) now finds `now` (`None`: no page). It does while `was` exists,
/// unless `now` is a page that just arrived (`arrived`: added, renamed or moved by the change
/// that made Cairn read the file again): a link follows a new nearer page, but a page that was
/// already there doesn't take it. Cairn's own renames and moves rewrite the links, so this keeps
/// the links to a page renamed or moved outside Cairn, until the linking page's text is next saved
/// in Cairn. `exists`: whether a page is still in the index.
pub(crate) fn still_meant(
    exists: &dyn Fn(&str) -> bool,
    arrived: &dyn Fn(&str) -> bool,
    was: &str,
    now: Option<&str>,
) -> bool {
    exists(was) && !now.is_some_and(arrived)
}

/// The text of the page file at `here` (`file_body`, unchanged since the index read it into `old`)
/// in the index's forms now: its links found again, except those [`still_meant`] keeps.
pub(crate) fn index_body(
    look: &DbFolder,
    file_body: &str,
    here: &str,
    old: &str,
    arrived: &dyn Fn(&str) -> bool,
) -> String {
    let new = convert::to_index(file_body, here, look);
    let exists = |id: &str| look.page_path(id).is_some();
    convert::keep_links(old, &new, &|was, now| still_meant(&exists, arrived, was, now))
}

impl Lookup for DbFolder<'_> {
    fn page_at(&self, path: &str) -> Option<String> {
        self.conn
            .query_row("SELECT id FROM pages WHERE path = ?1", [path], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }
    fn exists(&self, path: &str) -> bool {
        self.ext.abs(path).is_some_and(|a| a.exists())
    }
    fn page_named(&self, name: &str, here: &str) -> Option<String> {
        wiki_page_path(self.stems(), name, here).and_then(|p| self.page_at(&p))
    }
    fn file_named(&self, name: &str, here: &str) -> Option<String> {
        let named = |n: &str| {
            let files = self.files.get_or_init(|| super::files_by_name(self.ext));
            files.get(&fold(n)).cloned().unwrap_or_default()
        };
        embed_path(name, here, &|p| self.exists(p), &named)
    }
}

impl Places for DbFolder<'_> {
    fn page_path(&self, id: &str) -> Option<String> {
        self.conn
            .query_row("SELECT path FROM pages WHERE id = ?1", [id], |r| r.get::<_, Option<String>>(0))
            .optional()
            .ok()
            .flatten()
            .flatten()
    }
    fn attachment(&mut self, name: &str) -> Option<String> {
        if name.contains(['/', '\\']) || name.starts_with('.') {
            return None;
        }
        let source = self.ext.data.as_ref()?.join("attachments").join(name);
        if !source.is_file() {
            return None;
        }
        let category = self.category_id.clone()?;
        let dir = category_dir(self.conn, self.ext, &category, true).ok()?;
        let path = join(&join(&dir, "attachments"), name);
        let target = self.ext.writable(&path).ok()?;
        if !target.exists() {
            let bytes = std::fs::read(&source).ok()?;
            super::write_atomic(&target, &bytes).ok()?;
        }
        self.copied.push(path.clone());
        Some(path)
    }
}

// ---------------------------------------------------------------- page files

/// A page's file as it is now.
#[derive(Debug, Clone)]
pub(crate) struct OnDisk {
    pub hash: String,
    pub parsed: Parsed,
}

/// The file at `path`, if it is there.
pub(crate) fn read_page_file(ext: &Ext, path: &str) -> Result<Option<OnDisk>> {
    let bytes = match std::fs::read(ext.path(path)?) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let parsed = convert::parse(&bytes).map_err(|_| {
        CoreError::Invalid(format!(
            "\"{path}\" is not UTF-8 text, so Cairn can't change it. Save it as UTF-8 first."
        ))
    })?;
    Ok(Some(OnDisk {
        hash: hash_bytes(&bytes),
        parsed,
    }))
}

/// The page as its file says now (the index's forms), for reading a file changed outside Cairn
/// into the index before changing it, and for *Take the disk version*.
pub(crate) fn page_from_disk(conn: &Connection, ext: &Ext, page: &Page, disk: &OnDisk) -> Page {
    let here = page.path.clone().unwrap_or_default();
    let look = DbFolder::new(conn, ext, Some(&page.category_id));
    let mut p = page.clone();
    p.body = convert::to_index(&disk.parsed.body, &here, &look);
    p.tags = crate::knowledge::normalize_tags(disk.parsed.tags.clone());
    p.source_info = disk.parsed.source.as_deref().and_then(crate::knowledge::clean_source);
    p.archived = disk.parsed.archived;
    p.file_hash = Some(disk.hash.clone());
    p
}

/// The page file's name for `title`: the cleaned title and the old file's extension (or `.md`).
fn file_name_for(title: &str, old_path: Option<&str>) -> String {
    let ext = old_path
        .and_then(|p| page_extension(name_of(p)).map(str::to_string))
        .unwrap_or_else(|| "md".into());
    format!("{}.{ext}", clean_title(title))
}

/// Writes `new` (in the index's forms) to its file and fills in its `path` and `file_hash`; the
/// body takes the file's text read back. `old` is the page as the index had it before (`None`
/// for a new page) and `disk` its file now (read by the caller, which brought the index up to it
/// first). Moves and renames move the file and fix the links in other files. Returns the file's
/// stamp, for the row.
pub(crate) fn write_page(
    conn: &Connection,
    ext: &Ext,
    old: Option<&Page>,
    disk: Option<&OnDisk>,
    new: &mut Page,
) -> Result<Stamp> {
    ext.check()?;
    let old_path = old.and_then(|p| p.path.clone());
    let same_place = old.is_some_and(|o| {
        o.category_id == new.category_id && o.group_id == new.group_id && o.path.is_some()
    });
    let dir = match (&old_path, same_place) {
        (Some(p), true) => dir_of(p).to_string(),
        _ => target_dir(conn, ext, &new.category_id, new.group_id.as_deref(), true)?,
    };
    // The file keeps its name while the title doesn't change (a name another tool gave stays).
    let name = match (old, &old_path) {
        (Some(o), Some(op)) if o.title == new.title => name_of(op).to_string(),
        _ => {
            new.title = clean_title(&new.title);
            file_name_for(&new.title, old_path.as_deref())
        }
    };
    let path = join(&dir, &name);
    let moved = old_path.as_deref() != Some(path.as_str());
    if moved {
        check_name_free(conn, ext, &dir, &name, old_path.as_deref(), NameOf::Page)?;
    }
    // A moved page's row takes its new path first: links (its own too) are found from the index.
    let shift = match (old, &old_path) {
        (Some(o), Some(op)) if moved => {
            conn.execute("UPDATE pages SET path = ?2 WHERE id = ?1", params![new.id, path])?;
            Some(Shift {
                moves: vec![(op.clone(), path.clone())],
                added: None,
                retitle: (title_of(op) != new.title).then(|| (o.title.clone(), new.title.clone())),
            })
        }
        _ => None,
    };
    let shifted = shift.as_ref().map(|s| Shifted::new(conn, s)).transpose()?;

    // The body: the file's own text when the page's text didn't change.
    let keep_body = disk.is_some() && old.is_some_and(|o| o.body == new.body);
    let mut look = DbFolder::new(conn, ext, Some(&new.category_id));
    let body = match (keep_body, disk, &old_path, &shifted) {
        (true, Some(d), Some(op), Some(s)) => {
            let bound = old.and_then(|o| convert::wiki_targets(&d.parsed.body, &o.body));
            s.retarget(&d.parsed.body_raw, op, &path, bound.as_deref())
        }
        (true, _, _, _) => None,
        _ => Some(convert::to_file(&new.body, &path, &mut look)),
    };
    let meta = Meta {
        tags: new.tags.clone(),
        source: new.source_info.clone(),
        archived: new.archived,
    };
    let text = convert::render(disk.map(|d| &d.parsed), &meta, body.as_deref());
    let abs = ext.writable(&path)?;
    let mut file_there = false;
    if moved && let Some(op) = &old_path {
        let from = ext.writable(op)?;
        if super::exists_exact(&from) {
            super::move_path(&from, &abs)?;
            file_there = true;
        }
    } else if !moved {
        file_there = super::exists_exact(&abs);
    }
    let unchanged = file_there && disk.is_some_and(|d| d.hash == hash_bytes(text.as_bytes()));
    if !file_there {
        // A new file: never over one that appeared since the name was checked.
        super::write_new(&abs, text.as_bytes())?;
    } else if !unchanged {
        super::write_atomic(&abs, text.as_bytes())?;
    }
    let stamp = stamp_of(&std::fs::metadata(&abs)?);
    new.path = Some(path.clone());
    new.file_hash = Some(hash_bytes(text.as_bytes()));
    // The index takes what the file says now (from its new place, if it moved).
    if body.is_some() || shifted.is_some() {
        let back = convert::parse(text.as_bytes())
            .map_err(|_| CoreError::Invalid("The page could not be written as text.".into()))?;
        let arrived = |id: &str| shifted.as_ref().is_some_and(|s| s.arrived(id));
        new.body = index_body(&look, &back.body, &path, &new.body, &arrived);
    }
    if let Some(s) = &shifted {
        relink_with(conn, ext, s, Some(&new.id))?;
    }
    Ok(stamp)
}

/// FR-KN-EXT-10: moves a page's file to the trash.
pub(crate) fn delete_page_file(ext: &Ext, page: &Page) -> Result<()> {
    if let Some(p) = &page.path {
        let abs = ext.writable(p)?;
        // Exactly this name: where case is ignored, `pump.md` may open `Pump.md`.
        if super::exists_exact(&abs) {
            super::trash(&abs)?;
        }
    }
    Ok(())
}

/// FR-KN-EXT-12: of `candidates` (`folder:` paths a page used), the images inside an
/// `attachments` folder that no page, page version or draft uses any more, and that no other file
/// under the root mentions by name (a file Cairn doesn't index, such as a page that isn't UTF-8,
/// one in a hidden folder, or a `.canvas`), go to the trash. Runs once the change is committed
/// (`journal`). Failures only get logged.
pub(crate) fn drop_unused_images(conn: &Connection, ext: &Ext, candidates: &[String]) {
    let mut seen = HashSet::new();
    let mut unused: Vec<(String, std::path::PathBuf)> = Vec::new();
    for path in candidates {
        if !seen.insert(path.clone())
            || !super::valid_rel(path)
            || !dir_of(path).split('/').any(|d| d.eq_ignore_ascii_case("attachments"))
        {
            continue;
        }
        let needle = format!("folder:{}", convert::encode_path(path));
        let used = [
            "SELECT 1 FROM pages WHERE instr(body, ?1) > 0",
            "SELECT 1 FROM page_versions WHERE instr(snapshot, ?1) > 0",
            "SELECT 1 FROM drafts WHERE instr(content, ?1) > 0",
        ]
        .iter()
        .any(|sql| conn.query_row(sql, [&needle], |_| Ok(())).optional().ok().flatten().is_some());
        if used {
            continue;
        }
        if let Ok(abs) = ext.writable(path)
            && abs.is_file()
            && super::exists_exact(&abs)
        {
            unused.push((path.clone(), abs));
        }
    }
    if unused.is_empty() {
        return;
    }
    let names: Vec<String> = unused.iter().map(|(p, _)| name_of(p).to_string()).collect();
    let mentioned = mentioned_in_files(ext, &names);
    for (path, abs) in unused {
        if mentioned.contains(name_of(&path)) {
            continue;
        }
        if let Err(e) = super::trash(&abs) {
            tracing::warn!(error = %e, "could not move an unused image to the trash");
        }
    }
}

/// Files a mention check never reads: media and archives (the images themselves among them).
const NOT_TEXT: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tif", "tiff", "heic", "psd", "pdf", "zip",
    "gz", "7z", "rar", "tar", "mp3", "mp4", "m4a", "mov", "avi", "mkv", "wav", "ogg", "flac", "exe",
    "dll", "so", "dmg", "iso", "sqlite", "db",
];
/// Files above this size are not read by the mention check.
const MENTION_CHECK_MAX: u64 = 8 * 1024 * 1024;

/// Which of `names` (file names) some file under the root mentions in its bytes, as written or
/// percent-encoded. Links and `.git` are not followed, nor media files or files over 8 MB.
fn mentioned_in_files(ext: &Ext, names: &[String]) -> HashSet<String> {
    let mut needles: Vec<(Vec<u8>, usize)> = Vec::new();
    for (i, n) in names.iter().enumerate() {
        needles.push((n.as_bytes().to_vec(), i));
        let encoded = crate::knowledge::export::link_encode(n);
        if encoded != *n {
            needles.push((encoded.into_bytes(), i));
        }
    }
    let Ok(finder) = aho_corasick::AhoCorasick::new(needles.iter().map(|(b, _)| b)) else {
        // Be safe: an image may be in use.
        return names.iter().cloned().collect();
    };
    let mut found: HashSet<String> = HashSet::new();
    let mut stack = vec![ext.root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                if name != ".git" {
                    stack.push(e.path());
                }
                continue;
            }
            let ext_lower = name.rsplit_once('.').map(|(_, x)| x.to_ascii_lowercase()).unwrap_or_default();
            if !kind.is_file()
                || NOT_TEXT.contains(&ext_lower.as_str())
                || name.ends_with(super::TEMP_SUFFIX)
                || e.metadata().is_ok_and(|m| m.len() > MENTION_CHECK_MAX)
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            for m in finder.find_iter(&bytes) {
                found.insert(names[needles[m.pattern().as_usize()].1].clone());
            }
            if found.len() == names.len() {
                return found;
            }
        }
    }
    found
}

// ---------------------------------------------------------------- links in other files

/// Whether an index body links to one of the moved pages (`page:<id>`) or into a moved file or
/// folder (`folder:<path>`). One pass over the text, however many things moved.
fn links_into(body: &str, ids: &HashSet<&str>, moved: &dyn Fn(&str) -> Option<String>) -> bool {
    let end = |s: &str| {
        s.find(|c: char| c == ')' || c == '#' || c.is_whitespace())
            .unwrap_or(s.len())
    };
    let pages = body.match_indices("page:").any(|(i, m)| {
        let rest = &body[i + m.len()..];
        ids.contains(&rest[..end(rest)])
    });
    pages
        || body.match_indices("folder:").any(|(i, m)| {
            let rest = &body[i + m.len()..];
            moved(&crate::knowledge::folders::percent_decode(&rest[..end(rest)])).is_some()
        })
}

/// What one of Cairn's own changes did to the pages' places, for [`relink`].
#[derive(Debug, Clone, Default)]
pub(crate) struct Shift {
    /// Files or folders that moved: their old and new root-relative paths.
    pub moves: Vec<(String, String)>,
    /// The path of a page that is new.
    pub added: Option<String>,
    /// A renamed page's old and new title: Markdown links whose text was the old title get the new.
    pub retitle: Option<(String, String)>,
}

impl Shift {
    fn moved(&self, path: &str) -> Option<String> {
        self.moves.iter().find_map(|(f, t)| rebase(path, f, t))
    }

    fn back(&self, path: &str) -> Option<String> {
        self.moves.iter().find_map(|(f, t)| rebase(path, t, f))
    }
}

/// Which page is where at one moment, for finding what a link found then.
#[derive(Default)]
struct Names {
    at: HashMap<String, String>,
    of: HashMap<String, String>,
    stems: HashMap<String, Vec<String>>,
}

impl Names {
    fn add(&mut self, path: String, id: String) {
        add_stem(&mut self.stems, &path);
        self.of.insert(id.clone(), path.clone());
        self.at.insert(path, id);
    }

    /// The page a `[[name]]` link in the page at `here` finds.
    fn page(&self, name: &str, here: &str) -> Option<&str> {
        wiki_page_path(&self.stems, name, here).and_then(|p| self.at.get(&p)).map(String::as_str)
    }
}

/// The pages' places before and after a [`Shift`] (the index rows already have the new ones).
struct Shifted<'a> {
    shift: &'a Shift,
    before: Names,
    now: Names,
}

impl<'a> Shifted<'a> {
    fn new(conn: &Connection, shift: &'a Shift) -> Result<Self> {
        let mut stmt = conn.prepare("SELECT id, path FROM pages WHERE path IS NOT NULL")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let (mut before, mut now) = (Names::default(), Names::default());
        for (id, path) in rows {
            if shift.added.as_ref() != Some(&path) {
                before.add(shift.back(&path).unwrap_or_else(|| path.clone()), id.clone());
            }
            now.add(path, id);
        }
        Ok(Shifted { shift, before, now })
    }

    /// Whether the change added the page `id` or moved it.
    fn arrived(&self, id: &str) -> bool {
        self.before.of.get(id) != self.now.of.get(id)
    }

    /// The text of a file (at `old_here` before the change, `new_here` after) with only the links
    /// changed that must change: those into moved files and folders, the file's own relative links
    /// when it moved, and `[[…]]` links that would now find another page than they did (they name
    /// their page's place instead, `[[Far/Guide]]`, FR-KN-EXT-16). The page a `[[…]]` link found
    /// is the index's (`bound`, from [`convert::wiki_targets`]) where it has one, since the text may
    /// still name a page renamed outside Cairn; else what the name found before. `None`: none.
    fn retarget(
        &self,
        text: &str,
        old_here: &str,
        new_here: &str,
        bound: Option<&[Option<String>]>,
    ) -> Option<String> {
        let wiki = |nth: usize, name: &str| {
            let id = match bound.and_then(|b| b.get(nth)?.as_deref()) {
                Some(id) => id,
                None => self.before.page(name, old_here)?,
            };
            if self.now.page(name, new_here) == Some(id) {
                return None;
            }
            let to = self.now.of.get(id)?;
            wiki_name(name, to, new_here, &|n| self.now.page(n, new_here) == Some(id))
        };
        let retitle = |p: &str, text: &str| {
            let (old, new) = self.shift.retitle.as_ref()?;
            (self.shift.moves.iter().any(|(f, _)| f == p) && text == old).then(|| new.clone())
        };
        convert::retarget(text, old_here, new_here, &|p| self.shift.moved(p), &retitle, &wiki)
    }
}

/// How a `[[…]]` link written as `name` in the page at `here` names the page at `to` now (`finds`:
/// whether a name finds it): by the page's title when the link used a title and that finds it (a
/// renamed page), else by its path from the root, else from the linking page's folder
/// (`[[../Guide]]`, as Obsidian writes relative links: a page in the root has no other path), each
/// with the extension when the link had one or only that tells `.md` and `.markdown` apart.
/// `None` when no name finds it.
fn wiki_name(name: &str, to: &str, here: &str, finds: &dyn Fn(&str) -> bool) -> Option<String> {
    let path = if page_extension(name_of(name)).is_some() { to } else { without_extension(to) };
    let plain = (!name.contains('/')).then(|| name_of(path).to_string());
    let relative = [relative_name(here, path), relative_name(here, to)];
    [plain, Some(path.to_string()), Some(to.to_string())]
        .into_iter()
        .chain(relative.map(Some))
        .flatten()
        .find(|n| finds(n))
}

/// The path from the page at `here` to `to`, as a `[[…]]` link writes it (`./` or `../` first).
fn relative_name(here: &str, to: &str) -> String {
    let from: Vec<&str> = dir_of(here).split('/').filter(|p| !p.is_empty()).collect();
    let to: Vec<&str> = to.split('/').collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count().min(to.len() - 1);
    let mut parts = vec![".."; from.len() - common];
    if parts.is_empty() {
        parts.push(".");
    }
    parts.extend(&to[common..]);
    parts.join("/")
}

/// `%XX` escapes decoded where they are valid, the rest as it is (to find names in links).
fn decode_lossy(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let (Some(hi), Some(lo)) = (b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c)))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether an index body may hold a link that found no page and could find one with a title in
/// `titles` ([`title_key`]s), written plainly or percent-encoded. False positives cost a file read.
pub(super) fn may_name(body: &str, titles: &aho_corasick::AhoCorasick) -> bool {
    titles.is_match(&fold(body)) || (body.contains('%') && titles.is_match(&fold(&decode_lossy(body))))
}

/// The SQL condition for index bodies that may hold a link that found nothing (`[[…]]`, or a path
/// to a page file, also percent-encoded): only those can find a page that arrives.
pub(super) const UNRESOLVED_LINKS: &str =
    "(instr(body, '[[') > 0 OR body LIKE '%.md%' OR body LIKE '%.markdown%' OR instr(body, '%') > 0)";

/// The pages whose index text links to one of `ids`.
pub(crate) fn referrers(conn: &Connection, ids: &[String]) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT source_id FROM mentions
         WHERE source_kind = 'page' AND target_kind = 'page' AND target_id = ?1",
    )?;
    let mut out = BTreeSet::new();
    for id in ids {
        for source in stmt.query_map([id], |r| r.get::<_, String>(0))? {
            out.insert(source?);
        }
    }
    Ok(out)
}

/// The pages whose index text links to the page `id` at `path`: by its id, or to a heading in it
/// (`folder:<path>#…`), for reading them again when the page goes.
pub(crate) fn linking_to(conn: &Connection, id: &str, path: &str) -> Result<BTreeSet<String>> {
    let mut out = referrers(conn, &[id.to_string()])?;
    let needle = format!("folder:{}#", convert::encode_path(path));
    let mut stmt = conn.prepare("SELECT id FROM pages WHERE path IS NOT NULL AND instr(body, ?1) > 0")?;
    for row in stmt.query_map([&needle], |r| r.get::<_, String>(0))? {
        out.insert(row?);
    }
    out.remove(id);
    Ok(out)
}

/// The pages with an index text that may name one of `titles` ([`title_key`]s) in a link that
/// found nothing ([`may_name`]).
fn unresolved_referrers(conn: &Connection, titles: &HashSet<String>) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    if titles.is_empty() {
        return Ok(out);
    }
    let finder = aho_corasick::AhoCorasick::new(titles)
        .map_err(|e| CoreError::Invalid(format!("Cannot look for links to the pages: {e}")))?;
    let mut stmt = conn.prepare(&format!(
        "SELECT id, body FROM pages WHERE path IS NOT NULL AND {UNRESOLVED_LINKS}"
    ))?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (id, body) = row?;
        if may_name(&body, &finder) {
            out.insert(id);
        }
    }
    Ok(out)
}

/// A page whose file changed outside Cairn since the index read it, read into the index as a
/// change of its own (a daily version, activity), before Cairn changes anything (FR-KN-EXT-11).
pub(crate) fn take_from_disk(conn: &Connection, ext: &Ext, page: &Page, disk: &OnDisk, st: &crate::tasks::Stamp) -> Result<Page> {
    let mut fresh = page_from_disk(conn, ext, page, disk);
    crate::knowledge::commit_change(conn, &mut fresh, st)?;
    if let Some(path) = &page.path {
        set_file_stamp(conn, &page.id, &stamp_of(&std::fs::metadata(ext.path(path)?)?))?;
    }
    Ok(fresh)
}

/// After one of Cairn's own changes (FR-KN-EXT-10, -16), the links in other page files that must
/// change: into moved files and folders, the moved pages' own relative links, and `[[…]]` links
/// that would now find another page than they did (a new or renamed page with the same title
/// nearer to them), which then name their page's place. Pages whose links found nothing may find
/// a page now: their index text is read again. `skip`: a page whose file was just written.
pub(crate) fn relink(conn: &Connection, ext: &Ext, shift: &Shift, skip: Option<&str>) -> Result<()> {
    if shift.moves.is_empty() && shift.added.is_none() {
        return Ok(());
    }
    relink_with(conn, ext, &Shifted::new(conn, shift)?, skip)
}

fn relink_with(conn: &Connection, ext: &Ext, shifted: &Shifted, skip: Option<&str>) -> Result<()> {
    let shift = shifted.shift;
    // The pages that moved or are new, and the titles they had and have now.
    let mut moved_ids: HashSet<&str> = HashSet::new();
    let (mut titles, mut arrived) = (HashSet::new(), HashSet::new());
    for (id, path) in &shifted.now.of {
        let old = shift.back(path);
        if old.is_none() && shift.added.as_ref() != Some(path) {
            continue;
        }
        if old.is_some() {
            moved_ids.insert(id.as_str());
        }
        titles.extend(old.map(|o| title_key(&o)));
        titles.insert(title_key(path));
        arrived.insert(title_key(path));
    }
    // Their own links, links to pages with those titles (a nearer one may take them), links into
    // moved folders, and links that found nothing.
    let mut ids: BTreeSet<String> = moved_ids.iter().map(|id| id.to_string()).collect();
    let same: Vec<String> =
        shifted.now.of.iter().filter(|(_, p)| titles.contains(&title_key(p))).map(|(id, _)| id.clone()).collect();
    ids.extend(referrers(conn, &same)?);
    if !shift.moves.is_empty() {
        let mut stmt = conn.prepare("SELECT id, body FROM pages WHERE path IS NOT NULL AND instr(body, 'folder:') > 0")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
            let (id, body) = row?;
            if links_into(&body, &HashSet::new(), &|p| shift.moved(p)) {
                ids.insert(id);
            }
        }
    }
    ids.extend(unresolved_referrers(conn, &arrived)?);
    if let Some(s) = skip {
        ids.remove(s);
    }
    relink_pages(conn, ext, shifted, ids)
}

/// Reads the files of the pages `ids` again after their links' targets changed, and changes the
/// links that must change ([`Shifted::retarget`]); the index takes what the files then say.
fn relink_pages(conn: &Connection, ext: &Ext, shifted: &Shifted, ids: BTreeSet<String>) -> Result<()> {
    let look = DbFolder::new(conn, ext, None);
    let st = crate::tasks::Stamp::now();
    for id in ids {
        let Some(path) = shifted.now.of.get(&id) else { continue };
        let Some(disk) = read_page_file(ext, path).ok().flatten() else { continue };
        let (hash, old): (Option<String>, String) =
            conn.query_row("SELECT file_hash, body FROM pages WHERE id = ?1", [&id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        // A file changed outside since the index read it: its links are what its text says.
        let outside = hash.as_deref() != Some(disk.hash.as_str());
        let bound = if outside { None } else { convert::wiki_targets(&disk.parsed.body, &old) };
        let old_here = shifted.shift.back(path).unwrap_or_else(|| path.clone());
        let written = match shifted.retarget(&disk.parsed.body_raw, &old_here, path, bound.as_deref()) {
            Some(new_body) => {
                let mut text = String::new();
                if disk.parsed.bom {
                    text.push('\u{feff}');
                }
                text.push_str(&disk.parsed.front_raw);
                text.push_str(&disk.parsed.sep_raw);
                text.push_str(&new_body);
                let abs = ext.writable(path)?;
                super::write_atomic(&abs, text.as_bytes())?;
                let stamp = stamp_of(&std::fs::metadata(&abs)?);
                let parsed = convert::parse(text.as_bytes()).map_err(|_| CoreError::Invalid("not text".into()))?;
                Some((OnDisk { hash: hash_bytes(text.as_bytes()), parsed }, stamp))
            }
            None => None,
        };
        let file = written.as_ref().map_or(&disk, |(d, _)| d);
        if outside {
            // Read in whole as a change of its own (its tags too), as the file is now.
            take_from_disk(conn, ext, &crate::knowledge::get_page(conn, &id)?, file, &st)?;
            continue;
        }
        let body = index_body(&look, &file.parsed.body, path, &old, &|p| shifted.arrived(p));
        if let Some((d, stamp)) = &written {
            conn.execute(
                "UPDATE pages SET body = ?2, file_hash = ?3, file_size = ?4, file_mtime = ?5 WHERE id = ?1",
                params![id, body, d.hash, stamp.size, stamp.mtime],
            )?;
        } else if body != old {
            conn.execute("UPDATE pages SET body = ?2 WHERE id = ?1", params![id, body])?;
        }
        if body != old {
            hooks::page_saved(conn, &crate::knowledge::get_page(conn, &id)?)?;
        }
    }
    Ok(())
}

/// After Cairn deleted a page: the pages that linked to it (`ids`, from [`referrers`] before) read
/// again from their files, so their links find what the files say now.
pub(crate) fn reread(conn: &Connection, ext: &Ext, ids: BTreeSet<String>) -> Result<()> {
    let shift = Shift::default();
    relink_pages(conn, ext, &Shifted::new(conn, &shift)?, ids)
}

// ---------------------------------------------------------------- folders

/// Moves (or renames) the folder `from` to `to` (root-relative) and updates the paths of the
/// groups and pages inside it, then the links in other files. The caller checked the name.
pub(crate) fn move_dir(conn: &Connection, ext: &Ext, from: &str, to: &str) -> Result<()> {
    ext.check()?;
    let (a, b) = (ext.writable(from)?, ext.writable(to)?);
    if a.exists() {
        super::move_path(&a, &b)?;
    } else {
        make_dir(&b)?;
    }
    rebase_rows(conn, from, to)?;
    let shift = Shift {
        moves: vec![(from.to_string(), to.to_string())],
        ..Default::default()
    };
    relink(conn, ext, &shift, None)
}

/// Rewrites the paths of every group and page at or under `from` to be under `to`.
pub(crate) fn rebase_rows(conn: &Connection, from: &str, to: &str) -> Result<()> {
    for table in ["groups", "pages"] {
        conn.execute(
            &format!(
                "UPDATE {table} SET path = ?2 || substr(path, length(?1) + 1)
                 WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'"
            ),
            params![from, to],
        )?;
    }
    Ok(())
}

/// FR-KN-EXT-5: renaming a category renames its folder (when it has one).
pub(crate) fn rename_category(conn: &Connection, ext: &Ext, category_id: &str, new_name: &str) -> Result<()> {
    let c = categories::get(conn, category_id)?;
    let Some(old) = c.folder else { return Ok(()) };
    let new = super::clean_name(new_name, "Category");
    if new == old {
        return Ok(());
    }
    if new.eq_ignore_ascii_case("attachments") {
        return Err(CoreError::Invalid("A category can't be named \"attachments\" in a knowledge folder.".into()));
    }
    let used: Option<String> = conn
        .query_row(
            "SELECT id FROM categories WHERE folder IS NOT NULL AND lower(folder) = lower(?1) AND id <> ?2",
            params![new, category_id],
            |r| r.get(0),
        )
        .optional()?;
    if used.is_some() {
        return Err(CoreError::Conflict(format!(
            "Another category already uses the folder \"{new}\". Choose another name."
        )));
    }
    check_name_free(conn, ext, "", &new, Some(&old), NameOf::Folder)?;
    move_dir(conn, ext, &old, &new)?;
    conn.execute(
        "UPDATE categories SET folder = ?2 WHERE id = ?1",
        params![category_id, new],
    )?;
    Ok(())
}

/// The folder for a new group: in its parent group's folder, or its category's.
pub(crate) fn new_group_dir(
    conn: &Connection,
    ext: &Ext,
    category_id: &str,
    parent: Option<&Group>,
    name: &str,
) -> Result<String> {
    ext.check()?;
    let dir = match parent {
        Some(g) => g
            .path
            .clone()
            .ok_or_else(|| CoreError::Invalid("This group has no folder in the knowledge folder.".into()))?,
        None => category_dir(conn, ext, category_id, true)?,
    };
    if name.eq_ignore_ascii_case("attachments") {
        return Err(CoreError::Invalid(
            "\"attachments\" is the folder for a category's images. Choose another name.".into(),
        ));
    }
    check_name_free(conn, ext, &dir, name, None, NameOf::Folder)?;
    let path = join(&dir, name);
    make_dir(&ext.writable(&path)?)?;
    Ok(path)
}

/// FR-KN-EXT-3: stores a pasted image in `<category folder>/attachments/` (named by its content,
/// as in internal mode) and returns its path.
pub(crate) fn save_image(conn: &Connection, ext: &Ext, category_id: &str, bytes: &[u8], extension: &str) -> Result<String> {
    ext.check()?;
    let dir = category_dir(conn, ext, category_id, true)?;
    let digest = hash_bytes(bytes);
    let path = join(&join(&dir, "attachments"), &format!("{}.{extension}", &digest[..16]));
    let abs = ext.writable(&path)?;
    if !abs.exists() {
        super::write_atomic(&abs, bytes)?;
    }
    Ok(path)
}

/// For tests and tools: the absolute file of a page.
pub fn page_file(ext: &Ext, page: &Page) -> Option<std::path::PathBuf> {
    page.path.as_deref().and_then(|p| ext.abs(p))
}
