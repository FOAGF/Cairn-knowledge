//! Reading the folder into the index (FR-KN-EXT-2, -4, -5, -6; D-72 contract).
//!
//! A scan runs in three steps, so the database is never held for long:
//! 1. [`prepare`] (database lock, quick): what the index knows (paths, hashes, sizes, times of the
//!    pages; the groups; the categories and their folders).
//! 2. [`ScanJob::read`] (no lock): walks the folder (all of it, or only the paths a watcher batch
//!    names), compares each page file's size and time with the index and reads only the files
//!    that changed, then plans the changes. A file that disappeared while a new one with the same
//!    hash appeared is the same page, renamed (it keeps its id and Cairn-only data). Links are
//!    turned into the index's forms against the planned state, so pages found in one scan can
//!    link to each other.
//! 3. [`ScanPlan::apply_some`] (lock, one short transaction per call): applies the plan in
//!    batches of about [`BATCH_TIME`] each: folders first (new categories and groups), then
//!    removed, renamed, changed and new pages, then groups whose folders are gone. Every item is
//!    checked against the index again when it is applied: a page whose row already has the file's
//!    hash is skipped (Cairn wrote it itself), and a file whose size or time changed again since it
//!    was read is left for the next pass. Last, unchanged pages whose links may find another page
//!    now (after a page they link to, or a page with such a title, was added, removed or renamed)
//!    are read again, found from what was applied (`Relink`); links to a page renamed outside
//!    Cairn keep it (FR-KN-EXT-16).
//!
//! **Files that disappear** leave their Cairn-only data behind for 30 days (`removed_pages`,
//! `removed_groups`): a file that comes back at the same path (or, for a page, with the same
//! hash) gets its old id, history, task links, check status and source task again, and a folder
//! its group's id and description. A full scan that finds no Markdown at all while the index has
//! pages treats the folder as unreachable (an unmounted drive, an offline cloud folder) and
//! changes nothing. Paths are compared exactly as written, also where the file system ignores case
//! ([`super::Exact`]), and links below the root are never followed.
//!
//! Folders: each folder directly in the root (except `attachments`) is a category, matched by its
//! recorded folder, then by name; a new one becomes a category with a key from
//! `categories::suggest_key` and the next colour. A category whose folder disappeared while most of
//! its files reappeared under a new folder takes that folder's name (renamed outside Cairn);
//! otherwise it stays (tasks use it). A folder inside a category with Markdown below it is a group
//! (the same rename rule keeps a group's id and description); a group is removed only when its
//! folder is gone. Pages found in the folder are `origin = folder` and checked; their changes
//! write a daily version and activity like any change.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::time::SystemTime;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::convert::{self, Lookup, Parsed};
use super::store;
use super::{
    Exact, Ext, Stamp, TEMP_SUFFIX, dir_of, fold, hash_bytes, is_hidden, iso_of, join, name_of,
    page_extension, stamp_of, title_key, title_of, valid_rel,
};
use crate::knowledge::{self, CheckStatus, Page, PageOrigin};
use crate::tasks::Stamp as Clock;
use crate::util::new_id;
use crate::{CoreError, Db, Result};

/// The most plan items one transaction applies (D-72: a few hundred files at a time).
pub const BATCH: usize = 250;

/// How long one transaction of a scan may hold the database: it applies items until this much
/// time has passed (at least one), then commits, so a command waits at most about this long.
pub const BATCH_TIME: std::time::Duration = std::time::Duration::from_millis(100);

/// How long the Cairn-only data of a page or group whose file disappeared is kept.
const TOMBSTONE_DAYS: i64 = 30;

/// Cairn's temporary files older than this are left over from a write that stopped; a scan
/// removes them.
const STALE_TEMP: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// What a scan covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The whole folder (start-up, *Rescan*, a watcher that missed events).
    All,
    /// Root-relative paths that changed (files or folders), from a watcher batch.
    Paths(Vec<String>),
}

/// Where a scan is, for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    /// Walking the folder and reading changed files; `total` is the files found so far.
    Reading,
    /// Writing the changes into the index.
    Applying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: ScanPhase,
    pub done: u32,
    pub total: u32,
}

/// A file left out of the index, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanSkip {
    pub path: String,
    pub reason: String,
}

/// What a scan changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub added: u32,
    pub changed: u32,
    pub renamed: u32,
    pub removed: u32,
    pub categories_added: u32,
    pub groups_added: u32,
    pub groups_removed: u32,
    /// Pages whose links were read again (their files unchanged) because a page they link to, or
    /// could link to, was added, removed or renamed.
    pub relinked: u32,
    /// Files that are not in the index (not UTF-8 text, …).
    pub skipped: Vec<ScanSkip>,
}

impl ScanSummary {
    /// Whether the index changed (the UI refreshes, the indexer wakes).
    pub fn changed_anything(&self) -> bool {
        self.added + self.changed + self.renamed + self.removed + self.categories_added
            + self.groups_added
            + self.groups_removed
            + self.relinked
            > 0
    }
}

// ---------------------------------------------------------------- step 1: the index

#[derive(Debug, Clone)]
struct IndexedPage {
    id: String,
    hash: Option<String>,
    stamp: Option<Stamp>,
    category_id: String,
}

#[derive(Debug, Clone)]
struct IndexedGroup {
    id: String,
}

#[derive(Debug, Clone)]
struct IndexedCategory {
    id: String,
    name: String,
    folder: Option<String>,
}

/// A page whose file disappeared, kept to give its data back ([`TOMBSTONE_DAYS`]).
#[derive(Debug, Clone)]
struct Tombstone {
    id: String,
    path: String,
    hash: Option<String>,
}

/// A scan, ready to read the folder (no lock needed from here on).
#[derive(Debug, Clone)]
pub struct ScanJob {
    ext: Ext,
    scope: Scope,
    pages: HashMap<String, IndexedPage>,
    groups: HashMap<String, IndexedGroup>,
    categories: Vec<IndexedCategory>,
    /// Newest first.
    removed: Vec<Tombstone>,
}

/// Step 1 (under the database lock): what the index knows. Fails outside external mode or when
/// the root folder isn't there.
pub fn prepare(conn: &Connection, scope: Scope) -> Result<ScanJob> {
    let ext = super::active(conn)?
        .ok_or_else(|| CoreError::Invalid("Knowledge is not kept in a folder.".into()))?;
    ext.check()?;
    let mut stmt = conn.prepare(
        "SELECT id, path, file_hash, file_size, file_mtime, category_id FROM pages WHERE path IS NOT NULL",
    )?;
    let pages = stmt
        .query_map([], |r| {
            let size: Option<i64> = r.get(3)?;
            let mtime: Option<i64> = r.get(4)?;
            Ok((
                r.get::<_, String>(1)?,
                IndexedPage {
                    id: r.get(0)?,
                    hash: r.get(2)?,
                    stamp: size.zip(mtime).map(|(size, mtime)| Stamp { size, mtime }),
                    category_id: r.get(5)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    let mut stmt = conn.prepare("SELECT id, path FROM groups WHERE path IS NOT NULL")?;
    let groups = stmt
        .query_map([], |r| Ok((r.get::<_, String>(1)?, IndexedGroup { id: r.get(0)? })))?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    let mut stmt = conn.prepare("SELECT id, name, folder FROM categories ORDER BY is_general DESC, created_at")?;
    let categories = stmt
        .query_map([], |r| {
            Ok(IndexedCategory {
                id: r.get(0)?,
                name: r.get(1)?,
                folder: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut stmt = conn.prepare("SELECT id, path, file_hash FROM removed_pages ORDER BY removed_at DESC, id")?;
    let removed = stmt
        .query_map([], |r| {
            Ok(Tombstone {
                id: r.get(0)?,
                path: r.get(1)?,
                hash: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ScanJob {
        ext,
        scope,
        pages,
        groups,
        categories,
        removed,
    })
}

// ---------------------------------------------------------------- step 2: reading

/// A page file as read for the plan.
#[derive(Debug, Clone)]
struct Read {
    stamp: Stamp,
    hash: String,
    parsed: Parsed,
    created: Option<String>,
    modified: String,
}

#[derive(Debug, Clone)]
enum FolderChange {
    NewCategory { folder: String },
    LinkCategory { id: String, folder: String },
    RenameCategory { id: String, folder: String },
    NewGroup { path: String },
    RenameGroup { id: String, path: String },
}

#[derive(Debug, Clone)]
enum PageChange {
    Remove { id: String, path: String },
    Rename { id: String, from: String, to: String, read: Box<Read>, body: String },
    Change { id: String, path: String, read: Box<Read>, body: String },
    Touch { id: String, path: String, stamp: Stamp },
    Add { id: String, path: String, read: Box<Read>, body: String },
    RemoveGroup { path: String },
}

/// The planned changes of one scan; apply them with [`ScanPlan::apply_some`].
#[derive(Debug, Clone)]
pub struct ScanPlan {
    folders: Vec<FolderChange>,
    pages: Vec<PageChange>,
    /// The next folder change, then the next page change, to apply.
    folders_next: usize,
    next: usize,
    summary: ScanSummary,
    /// Pages that link to pages of this plan not yet in the index when they were applied: their
    /// mentions are indexed again at the end (from `links_next`).
    forward_links: Vec<String>,
    links_next: usize,
    /// Old removed-page data was dropped (once per plan).
    expired: bool,
    folded: Folded,
    relink: Relink,
}

/// The pages whose links are read again once the plan's page changes are applied: those linking
/// to a page it removed, or to a page with the title of one it added or renamed (a nearer page may
/// take the link), and those with a link that found nothing (it may find a page now). Found from
/// what was applied, never from the plan: a change skipped when applied changes no links.
#[derive(Debug, Clone, Default)]
struct Relink {
    /// The pages added or renamed, and their titles ([`super::title_key`]).
    arrived: HashSet<String>,
    titles: BTreeSet<String>,
    /// Pages that linked to a removed page.
    found: BTreeSet<String>,
    /// Pages this plan changed or added: their links were found against the planned folder
    /// already. A renamed page isn't one: its links were kept or followed as the change was
    /// applied, before the pages the plan adds after it arrived.
    written: HashSet<String>,
    /// How far the search for links that found nothing got (a `rowid` of `pages`).
    searched_to: Option<i64>,
    /// The pages to read again, once found (`None` until then), and the next one.
    pages: Option<Vec<String>>,
    next: usize,
}

/// The rows of `pages` one step of the search for links that found nothing looks at.
const SEARCH_ROWS: i64 = 500;

/// The index's page paths by their case-folded form, for the check before an `Add` that the same
/// file isn't indexed under another case. Read once per pass and kept up to date by it, so the
/// check costs the same for the last file as for the first; read again when something else was
/// written between two of the pass's transactions (the connection's change count moved), or after
/// one failed.
#[derive(Debug, Clone, Default)]
struct Folded {
    paths: HashMap<String, Vec<String>>,
    /// The connection's `total_changes` when the map was last right; `None`: read it again.
    at: Option<u64>,
}

impl Folded {
    fn refresh(&mut self, conn: &Connection) -> Result<()> {
        if self.at == Some(conn.total_changes()) {
            return Ok(());
        }
        self.paths.clear();
        let mut stmt = conn.prepare("SELECT path FROM pages WHERE path IS NOT NULL")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for p in rows {
            self.add(&p);
        }
        Ok(())
    }

    /// The map is right as of now (end of a transaction of the pass).
    fn settle(&mut self, conn: &Connection) {
        self.at = Some(conn.total_changes());
    }

    fn add(&mut self, path: &str) {
        self.paths.entry(path.to_lowercase()).or_default().push(path.to_string());
    }

    fn remove(&mut self, path: &str) {
        let key = path.to_lowercase();
        if let Some(list) = self.paths.get_mut(&key) {
            list.retain(|p| p != path);
            if list.is_empty() {
                self.paths.remove(&key);
            }
        }
    }

    /// Indexed paths that differ from `path` only in case.
    fn others<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a String> + 'a {
        self.paths
            .get(&path.to_lowercase())
            .into_iter()
            .flatten()
            .filter(move |p| p.as_str() != path)
    }
}

/// The folder on disk, as far as the scope reaches.
#[derive(Default)]
struct Disk {
    /// Page files with their metadata.
    pages: HashMap<String, std::fs::Metadata>,
    /// Every other file (for `folder:` links).
    files: HashSet<String>,
    /// Folders walked (not hidden).
    dirs: HashSet<String>,
    /// Scope paths that are gone.
    gone: Vec<String>,
    /// Whether everything was walked (else `exists` asks the disk).
    complete: bool,
    /// Page files whose names can't be used on every system.
    bad_names: Vec<ScanSkip>,
    exact: Exact,
}

impl Disk {
    fn walk(&mut self, ext: &Ext, rel: &str, progress: &mut dyn FnMut(ScanProgress)) -> Result<()> {
        let Some(abs) = ext.abs(rel) else { return Ok(()) };
        let entries = match std::fs::read_dir(&abs) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_hidden(&name) {
                // Cairn's temporary file of a write that stopped half way.
                if name.ends_with(TEMP_SUFFIX)
                    && entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age > STALE_TEMP))
                {
                    let _ = std::fs::remove_file(entry.path());
                }
                continue;
            }
            let kind = entry.file_type()?;
            let path = join(rel, &name);
            if !valid_rel(&path) {
                if kind.is_file() && page_extension(&name).is_some() {
                    self.bad_names.push(ScanSkip {
                        path,
                        reason: "The file name can't be used on every system (it has a \\ in it).".into(),
                    });
                }
                continue;
            }
            if kind.is_dir() {
                self.dirs.insert(path.clone());
                self.walk(ext, &path, progress)?;
            } else if kind.is_file() {
                if page_extension(&name).is_some() {
                    let meta = entry.metadata()?;
                    self.pages.insert(path, meta);
                    if self.pages.len().is_multiple_of(500) {
                        progress(ScanProgress {
                            phase: ScanPhase::Reading,
                            done: 0,
                            total: self.pages.len() as u32,
                        });
                    }
                } else {
                    self.files.insert(path);
                }
            }
            // Links (symbolic links, junctions) are not followed.
        }
        Ok(())
    }

    /// One path of a watcher batch, exactly as named: a folder is walked, a file noted; a path
    /// that isn't there exactly so (or only through a link) is gone.
    fn look(&mut self, ext: &Ext, rel: &str, progress: &mut dyn FnMut(ScanProgress)) -> Result<()> {
        if rel.split('/').any(is_hidden) || !valid_rel(rel) {
            return Ok(());
        }
        match self.exact.kind(ext, rel) {
            Some(k) if k.is_dir() => {
                self.dirs.insert(rel.to_string());
                // Its parents are folders too (a group made by a new file in a new folder).
                self.walk(ext, rel, progress)
            }
            Some(k) if k.is_file() => {
                let meta = ext.abs(rel).and_then(|a| std::fs::symlink_metadata(a).ok());
                match meta {
                    Some(m) if page_extension(name_of(rel)).is_some() => {
                        self.pages.insert(rel.to_string(), m);
                    }
                    Some(_) => {
                        self.files.insert(rel.to_string());
                    }
                    None => self.gone.push(rel.to_string()),
                }
                Ok(())
            }
            Some(_) => Ok(()),
            None => {
                self.gone.push(rel.to_string());
                Ok(())
            }
        }
    }

    /// Whether a folder is there (exactly so), as far as this scan knows.
    fn has_dir(&self, ext: &Ext, rel: &str) -> bool {
        if self.complete {
            self.dirs.contains(rel)
        } else {
            self.dirs.contains(rel) || self.exact.is_dir(ext, rel)
        }
    }
}

/// The planned state of the folder, for turning links into the index's forms.
struct Planned<'a> {
    ext: &'a Ext,
    disk: &'a Disk,
    by_path: HashMap<String, String>,
    by_stem: HashMap<String, Vec<String>>,
    /// The folder's other files by name, read when a watcher batch's embed is found nowhere nearer.
    files: std::cell::OnceCell<HashMap<String, Vec<String>>>,
}

impl Planned<'_> {
    fn add(&mut self, path: &str, id: &str) {
        self.by_path.insert(path.to_string(), id.to_string());
        store::add_stem(&mut self.by_stem, path);
    }
}

impl Lookup for Planned<'_> {
    fn page_at(&self, path: &str) -> Option<String> {
        self.by_path.get(path).cloned()
    }
    fn exists(&self, path: &str) -> bool {
        if self.disk.files.contains(path) || self.disk.dirs.contains(path) || self.disk.pages.contains_key(path) {
            return true;
        }
        !self.disk.complete && self.ext.abs(path).is_some_and(|a| a.exists())
    }
    fn page_named(&self, name: &str, here: &str) -> Option<String> {
        store::wiki_page_path(&self.by_stem, name, here).and_then(|p| self.by_path.get(&p).cloned())
    }
    fn file_named(&self, name: &str, here: &str) -> Option<String> {
        // A full scan saw every file; a watcher batch only its own.
        let named = |n: &str| {
            let n = fold(n);
            if self.disk.complete {
                self.disk.files.iter().filter(|f| fold(name_of(f)) == n).cloned().collect()
            } else {
                let files = self.files.get_or_init(|| super::files_by_name(self.ext));
                files.get(&n).cloned().unwrap_or_default()
            }
        };
        store::embed_path(name, here, &|p| self.exists(p), &named)
    }
}

/// Whether `path` is in the scan's reach.
fn in_scope(scope: &Scope, path: &str) -> bool {
    match scope {
        Scope::All => true,
        Scope::Paths(paths) => paths.iter().any(|p| super::is_within(path, p)),
    }
}

/// A top-level folder that can't be a category.
fn not_a_category(folder: &str) -> bool {
    folder.eq_ignore_ascii_case("attachments")
}

fn read_file(ext: &Ext, path: &str, meta: &std::fs::Metadata) -> std::result::Result<Read, String> {
    let abs = ext.abs(path).ok_or_else(|| "The path is not inside the knowledge folder.".to_string())?;
    let bytes = std::fs::read(abs).map_err(|e| format!("The file can't be read: {e}"))?;
    let parsed = convert::parse(&bytes).map_err(|_| "The file is not UTF-8 text.".to_string())?;
    Ok(Read {
        stamp: stamp_of(meta),
        hash: hash_bytes(&bytes),
        parsed,
        created: meta.created().ok().map(iso_of),
        modified: iso_of(meta.modified().unwrap_or_else(|_| SystemTime::now())),
    })
}

impl ScanJob {
    /// Step 2 (no lock): walks the folder, reads the changed files and plans the changes.
    pub fn read(self, progress: &mut dyn FnMut(ScanProgress)) -> Result<ScanPlan> {
        let ext = &self.ext;
        let mut disk = Disk::default();
        match &self.scope {
            Scope::All => {
                disk.walk(ext, "", progress)?;
                disk.complete = true;
            }
            Scope::Paths(paths) => {
                for p in paths {
                    disk.look(ext, p, progress)?;
                }
            }
        }
        // FR-KN-EXT-6: no Markdown at all while the index has pages: the folder is not reachable
        // now (a drive not mounted, an offline cloud folder), not emptied; nothing is changed.
        if disk.complete && disk.pages.is_empty() && !self.pages.is_empty() {
            return Err(CoreError::Invalid(format!(
                "The knowledge folder \"{}\" has no Markdown files right now, so Cairn changed nothing. Check that its drive or cloud folder is available, then rescan.",
                self.ext.root.display()
            )));
        }
        let scope = &self.scope;
        let mut summary = ScanSummary::default();

        // Pages: missing, new and changed files.
        let mut missing: Vec<(&String, &IndexedPage)> = self
            .pages
            .iter()
            .filter(|(p, _)| in_scope(scope, p) && !disk.pages.contains_key(*p))
            .collect();
        missing.sort_by(|a, b| a.0.cmp(b.0));
        let mut fresh: Vec<&String> = disk.pages.keys().filter(|p| !self.pages.contains_key(*p)).collect();
        fresh.sort();
        let mut stale: Vec<&String> = disk
            .pages
            .iter()
            .filter(|(p, meta)| {
                self.pages
                    .get(*p)
                    .is_some_and(|ix| ix.stamp != Some(stamp_of(meta)) || ix.hash.is_none())
            })
            .map(|(p, _)| p)
            .collect();
        stale.sort();

        // Pages under the top-level `attachments` folder are not knowledge.
        let mut skipped = std::mem::take(&mut disk.bad_names);
        fresh.retain(|p| {
            let keep = !(p.contains('/') && not_a_category(p.split('/').next().unwrap_or("")));
            if !keep {
                skipped.push(ScanSkip {
                    path: (*p).clone(),
                    reason: "Files in the top-level attachments folder are not pages.".into(),
                });
            }
            keep
        });

        let to_read = fresh.len() + stale.len();
        let mut reads: HashMap<String, Read> = HashMap::new();
        for (n, p) in fresh.iter().chain(stale.iter()).enumerate() {
            match read_file(ext, p, &disk.pages[*p]) {
                Ok(r) => {
                    reads.insert((*p).clone(), r);
                }
                Err(reason) => skipped.push(ScanSkip {
                    path: (*p).clone(),
                    reason,
                }),
            }
            if n % 100 == 99 {
                progress(ScanProgress {
                    phase: ScanPhase::Reading,
                    done: n as u32 + 1,
                    total: to_read as u32,
                });
            }
        }

        // Renames: a missing page whose hash a new file has (FR-KN-EXT-4). Same name first, then
        // same folder, then any.
        let mut renamed: Vec<(String, String, String)> = Vec::new(); // (id, from, to)
        let mut taken: HashSet<String> = HashSet::new();
        let mut matched_missing: HashSet<String> = HashSet::new();
        for (from, ix) in &missing {
            let Some(h) = &ix.hash else { continue };
            let candidates: Vec<&&String> = fresh
                .iter()
                .filter(|p| !taken.contains(**p) && reads.get(**p).is_some_and(|r| &r.hash == h))
                .collect();
            let best = candidates
                .iter()
                .min_by_key(|p| (name_of(p) != name_of(from), dir_of(p) != dir_of(from), p.to_string()))
                .map(|p| (**p).clone());
            if let Some(to) = best {
                taken.insert(to.clone());
                matched_missing.insert((*from).clone());
                renamed.push((ix.id.clone(), (*from).clone(), to));
            }
        }

        // Folders, and the groups whose folder is gone (removed after their pages moved out).
        let folders = self.plan_folders(&disk, &missing, &renamed);
        let renamed_groups: HashSet<&String> = folders
            .iter()
            .filter_map(|f| match f {
                FolderChange::RenameGroup { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        let mut removed_groups: Vec<&String> = Vec::new();
        for (p, g) in &self.groups {
            if in_scope(scope, p) && !renamed_groups.contains(&g.id) && !disk.has_dir(ext, p) {
                removed_groups.push(p);
            }
        }
        removed_groups.sort_by_key(|p| (std::cmp::Reverse(p.matches('/').count()), (*p).clone()));

        // The planned state: the index without the missing pages, with renames and new pages.
        let mut planned = Planned {
            ext,
            disk: &disk,
            by_path: HashMap::new(),
            by_stem: HashMap::new(),
            files: std::cell::OnceCell::new(),
        };
        let gone: HashSet<&String> = missing.iter().map(|(p, _)| *p).collect();
        for (p, ix) in &self.pages {
            if !gone.contains(p) {
                planned.add(p, &ix.id);
            }
        }
        for (id, _, to) in &renamed {
            planned.add(to, id);
        }
        // New files: a page whose file disappeared before (same path, then the same path in
        // another case, then the same text) comes back with its id; others get a new one.
        let in_use: HashSet<&str> = self.pages.values().map(|ix| ix.id.as_str()).collect();
        let mut revived: HashSet<String> = HashSet::new();
        let mut new_ids: HashMap<String, String> = HashMap::new();
        for p in &fresh {
            if taken.contains(*p) {
                continue;
            }
            let Some(read) = reads.get(*p) else { continue };
            let usable = |t: &&Tombstone| !revived.contains(&t.id) && !in_use.contains(t.id.as_str());
            let lower = p.to_lowercase();
            let back = self
                .removed
                .iter()
                .filter(usable)
                .find(|t| t.path == **p)
                .or_else(|| self.removed.iter().filter(usable).find(|t| t.path.to_lowercase() == lower))
                .or_else(|| {
                    self.removed
                        .iter()
                        .filter(usable)
                        .find(|t| t.hash.as_deref() == Some(read.hash.as_str()))
                });
            let id = match back {
                Some(t) => {
                    revived.insert(t.id.clone());
                    t.id.clone()
                }
                None => new_id(),
            };
            planned.add(p, &id);
            new_ids.insert((*p).clone(), id);
        }


        // Page changes, in the order they are applied.
        let mut pages: Vec<PageChange> = Vec::new();
        for (from, ix) in &missing {
            if !matched_missing.contains(*from) {
                pages.push(PageChange::Remove {
                    id: ix.id.clone(),
                    path: (*from).clone(),
                });
            }
        }
        for (id, from, to) in renamed {
            let read = reads.remove(&to).expect("renames come from read files");
            let body = convert::to_index(&read.parsed.body, &to, &planned);
            pages.push(PageChange::Rename {
                id,
                from,
                to,
                read: Box::new(read),
                body,
            });
        }
        for p in &stale {
            let ix = &self.pages[*p];
            let Some(read) = reads.remove(*p) else { continue };
            if ix.hash.as_deref() == Some(read.hash.as_str()) {
                pages.push(PageChange::Touch {
                    id: ix.id.clone(),
                    path: (*p).clone(),
                    stamp: read.stamp,
                });
            } else {
                let body = convert::to_index(&read.parsed.body, p, &planned);
                pages.push(PageChange::Change {
                    id: ix.id.clone(),
                    path: (*p).clone(),
                    read: Box::new(read),
                    body,
                });
            }
        }
        for p in &fresh {
            let Some(id) = new_ids.remove(*p) else { continue };
            let Some(read) = reads.remove(*p) else { continue };
            let body = convert::to_index(&read.parsed.body, p, &planned);
            pages.push(PageChange::Add {
                id,
                path: (*p).clone(),
                read: Box::new(read),
                body,
            });
        }
        // Groups whose folder is gone, after their pages moved out.
        for p in removed_groups {
            pages.push(PageChange::RemoveGroup { path: p.clone() });
        }

        summary.skipped = skipped;
        Ok(ScanPlan {
            folders,
            pages,
            folders_next: 0,
            next: 0,
            summary,
            forward_links: Vec::new(),
            links_next: 0,
            expired: false,
            folded: Folded::default(),
            relink: Relink::default(),
        })
    }

    /// Categories and groups from the folders (see the module comment).
    fn plan_folders(
        &self,
        disk: &Disk,
        missing: &[(&String, &IndexedPage)],
        renamed: &[(String, String, String)],
    ) -> Vec<FolderChange> {
        let ext = &self.ext;
        let scope = &self.scope;
        let mut out = Vec::new();
        // Which index folders are gone (exactly so), asked once each.
        let gone_dirs: HashSet<String> = {
            let mut gone = HashSet::new();
            let known: Vec<String> = self
                .categories
                .iter()
                .filter_map(|c| c.folder.clone())
                .chain(self.groups.keys().cloned())
                .collect();
            for d in known {
                if in_scope(scope, &d) && !disk.has_dir(ext, &d) {
                    gone.insert(d);
                }
            }
            gone
        };
        let exists_dir = |p: &str| !gone_dirs.contains(p);

        // Top-level folders on disk (in reach).
        let mut tops: Vec<&String> = disk
            .dirs
            .iter()
            .filter(|d| !d.contains('/') && !not_a_category(d))
            .collect();
        // Folders of new page files whose parent folders were not walked (a scoped scan).
        let mut extra: BTreeSet<String> = BTreeSet::new();
        for p in disk.pages.keys() {
            let mut d = dir_of(p);
            while !d.is_empty() {
                if !disk.dirs.contains(d) {
                    extra.insert(d.to_string());
                }
                d = dir_of(d);
            }
        }
        for d in &extra {
            if !d.contains('/') && !not_a_category(d) && !tops.contains(&d) {
                tops.push(d);
            }
        }
        tops.sort();
        tops.dedup();

        let mut used: HashSet<String> = self.categories.iter().filter_map(|c| c.folder.clone()).collect();
        let mut linked: HashSet<String> = HashSet::new();
        let mut unmatched: Vec<&String> = Vec::new();
        for top in tops {
            if self.categories.iter().any(|c| c.folder.as_deref() == Some(top.as_str())) {
                continue;
            }
            let by_name = self.categories.iter().find(|c| {
                c.folder.is_none()
                    && !linked.contains(&c.id)
                    && !used.contains(top.as_str())
                    && (c.name.to_lowercase() == top.to_lowercase()
                        || super::clean_name(&c.name, "Category").to_lowercase() == top.to_lowercase())
            });
            match by_name {
                Some(c) => {
                    used.insert(top.clone());
                    linked.insert(c.id.clone());
                    out.push(FolderChange::LinkCategory {
                        id: c.id.clone(),
                        folder: top.clone(),
                    });
                }
                None => unmatched.push(top),
            }
        }

        // A category whose folder is gone while most of its files reappeared under a new folder.
        let moved_to: HashMap<&str, &str> = renamed.iter().map(|(_, f, t)| (f.as_str(), t.as_str())).collect();
        for c in &self.categories {
            let Some(folder) = &c.folder else { continue };
            if !in_scope(scope, folder) || exists_dir(folder) {
                continue;
            }
            let inside: Vec<&&String> = missing
                .iter()
                .filter(|(p, ix)| ix.category_id == c.id && super::is_within(p, folder))
                .map(|(p, _)| p)
                .collect();
            if inside.is_empty() {
                continue;
            }
            let mut votes: HashMap<&str, usize> = HashMap::new();
            for p in &inside {
                if let Some(to) = moved_to.get(p.as_str()) {
                    let top = to.split('/').next().unwrap_or("");
                    if to.contains('/') {
                        *votes.entry(top).or_default() += 1;
                    }
                }
            }
            if let Some((top, n)) = votes.into_iter().max_by_key(|(t, n)| (*n, std::cmp::Reverse(*t)))
                && n * 2 > inside.len()
                && let Some(pos) = unmatched.iter().position(|u| u.as_str() == top)
            {
                let top = unmatched.remove(pos);
                out.push(FolderChange::RenameCategory {
                    id: c.id.clone(),
                    folder: top.clone(),
                });
            }
        }
        for top in unmatched {
            out.push(FolderChange::NewCategory { folder: top.clone() });
        }

        // Groups: folders inside a category with Markdown below them.
        let mut with_pages: BTreeSet<String> = BTreeSet::new();
        for p in disk.pages.keys() {
            if p.contains('/') && not_a_category(p.split('/').next().unwrap_or("")) {
                continue;
            }
            let mut d = dir_of(p);
            while d.contains('/') {
                with_pages.insert(d.to_string());
                d = dir_of(d);
            }
        }
        let new_dirs: Vec<&String> = with_pages.iter().filter(|d| !self.groups.contains_key(*d)).collect();
        // A group whose folder is gone while most of its pages reappeared directly in a new
        // folder keeps its id and description (renamed outside Cairn); its groups follow.
        let mut renamed_groups: Vec<(String, String)> = Vec::new(); // (old path, new path)
        let mut gone_groups: Vec<(&String, &IndexedGroup)> = self
            .groups
            .iter()
            .filter(|(p, _)| in_scope(scope, p) && !exists_dir(p))
            .collect();
        gone_groups.sort_by_key(|(p, _)| (p.matches('/').count(), (*p).clone()));
        let mut claimed: HashSet<String> = HashSet::new();
        let mut group_renames = Vec::new();
        for (old, g) in &gone_groups {
            if let Some(new) = renamed_groups.iter().find_map(|(o, n)| super::rebase(old, o, n)) {
                if new_dirs.iter().any(|d| **d == new) && claimed.insert(new.clone()) {
                    group_renames.push(FolderChange::RenameGroup { id: g.id.clone(), path: new.clone() });
                    renamed_groups.push(((*old).clone(), new));
                }
                continue;
            }
            let direct: Vec<&&String> = missing
                .iter()
                .filter(|(p, _)| dir_of(p) == old.as_str())
                .map(|(p, _)| p)
                .collect();
            if direct.is_empty() {
                continue;
            }
            let mut votes: HashMap<&str, usize> = HashMap::new();
            for p in &direct {
                if let Some(to) = moved_to.get(p.as_str()) {
                    *votes.entry(dir_of(to)).or_default() += 1;
                }
            }
            if let Some((dir, n)) = votes.into_iter().max_by_key(|(d, n)| (*n, std::cmp::Reverse(*d)))
                && n * 2 > direct.len()
                && new_dirs.iter().any(|d| d.as_str() == dir)
                && claimed.insert(dir.to_string())
            {
                group_renames.push(FolderChange::RenameGroup { id: g.id.clone(), path: dir.to_string() });
                renamed_groups.push(((*old).clone(), dir.to_string()));
            }
        }
        out.extend(group_renames);
        let mut fresh_groups: Vec<&String> = new_dirs.into_iter().filter(|d| !claimed.contains(*d)).collect();
        fresh_groups.sort_by_key(|d| (d.matches('/').count(), (*d).clone()));
        for d in fresh_groups {
            out.push(FolderChange::NewGroup { path: d.clone() });
        }
        out
    }
}

// ---------------------------------------------------------------- step 3: applying

impl ScanPlan {
    /// Items left to apply.
    pub fn remaining(&self) -> usize {
        self.folders.len() - self.folders_next + self.pages.len() - self.next
    }

    /// Whether the plan changes nothing.
    pub fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.pages.is_empty()
    }

    /// All items of the plan (the mention updates at the end come on top).
    pub fn total(&self) -> usize {
        self.folders.len() + self.pages.len()
    }

    pub fn is_done(&self) -> bool {
        self.folders_next >= self.folders.len()
            && self.next >= self.pages.len()
            && self.relink.is_done()
            && self.links_next >= self.forward_links.len()
    }

    /// What was applied so far, and the files left out.
    pub fn summary(&self) -> &ScanSummary {
        &self.summary
    }

    /// Step 3: applies up to `max` items in one transaction. Returns how many were applied
    /// (0 when the plan is done). A failed transaction leaves the plan where it was.
    pub fn apply_some(&mut self, db: &mut Db, max: usize) -> Result<usize> {
        let (folders_next, next, summary, forward, links_next, expired, relink) = (
            self.folders_next,
            self.next,
            self.summary.clone(),
            self.forward_links.len(),
            self.links_next,
            self.expired,
            self.relink.clone(),
        );
        let r = db.tx(|tx| self.apply_in(tx, max));
        if r.is_err() {
            self.folded.at = None;
            self.folders_next = folders_next;
            self.next = next;
            self.summary = summary;
            self.forward_links.truncate(forward);
            self.links_next = links_next;
            self.expired = expired;
            self.relink = relink;
        }
        r
    }

    /// [`Self::apply_some`] inside the caller's transaction.
    pub(crate) fn apply_in(&mut self, conn: &Connection, max: usize) -> Result<usize> {
        let ext = super::active(conn)?
            .ok_or_else(|| CoreError::Invalid("Knowledge is not kept in a folder any more.".into()))?;
        let started = std::time::Instant::now();
        // At least one item per call; then until the batch is full or its time is up.
        let more = |n: usize| n == 0 || (n < max.max(1) && started.elapsed() < BATCH_TIME);
        let mut n = 0;
        let exact = Exact::default();
        self.folded.refresh(conn)?;
        if !self.expired {
            expire_tombstones(conn)?;
            self.expired = true;
        }
        while self.folders_next < self.folders.len() && more(n) {
            apply_folder(conn, &ext, &exact, &self.folders[self.folders_next], &mut self.summary)?;
            self.folders_next += 1;
            n += 1;
        }
        let clock = Clock::now();
        while self.folders_next >= self.folders.len() && self.next < self.pages.len() && more(n) {
            let change = &self.pages[self.next];
            self.next += 1;
            n += 1;
            let applied = apply_page(
                conn,
                &ext,
                &exact,
                &mut self.folded,
                change,
                &clock,
                &mut self.summary,
                &mut self.relink,
            )?;
            if let Some(id) = applied
                && links_ahead(conn, &id)?
            {
                self.forward_links.push(id);
            }
        }
        // Then the pages whose links may find something else now ([`Relink`]), against the index
        // as it is in this transaction (read once in it, after the page changes).
        while self.next >= self.pages.len() && self.relink.pages.is_none() && more(n) {
            if self.relink.search(conn)? {
                n += 1;
            }
        }
        let look = store::DbFolder::new(conn, &ext, None);
        while let Some(pages) = &self.relink.pages
            && self.relink.next < pages.len()
            && more(n)
        {
            let id = pages[self.relink.next].clone();
            self.relink.next += 1;
            n += 1;
            if reread(conn, &ext, &exact, &look, &id, &self.relink.arrived)? {
                self.summary.relinked += 1;
                if links_ahead(conn, &id)? {
                    self.forward_links.push(id);
                }
            }
        }
        while self.next >= self.pages.len()
            && self.relink.is_done()
            && self.links_next < self.forward_links.len()
            && more(n)
        {
            let id = &self.forward_links[self.links_next];
            if let Ok(p) = knowledge::get_page(conn, id) {
                crate::mentions::index_source(conn, knowledge::MentionSource::Page, &p.id, &p.body)?;
            }
            self.links_next += 1;
            n += 1;
        }
        self.folded.settle(conn);
        Ok(n)
    }
}

impl Relink {
    fn is_done(&self) -> bool {
        self.pages.as_ref().is_some_and(|p| self.next >= p.len())
    }

    /// One step of finding the pages to read again: first the pages linking to a page with an
    /// arrived title, then [`SEARCH_ROWS`] rows of `pages` at a time for links that found nothing.
    /// Whether it looked at anything (a plan that added and renamed nothing doesn't).
    fn search(&mut self, conn: &Connection) -> Result<bool> {
        if self.titles.is_empty() {
            self.finish();
            return Ok(false);
        }
        let after = match self.searched_to {
            Some(at) => at,
            None => {
                // A plan that wrote every page (a first scan) found all their links already.
                let pages: i64 =
                    conn.query_row("SELECT COUNT(*) FROM pages WHERE path IS NOT NULL", [], |r| r.get(0))?;
                if pages as usize <= self.written.len() {
                    self.finish();
                    return Ok(false);
                }
                let mut stmt = conn.prepare("SELECT id, path FROM pages WHERE path IS NOT NULL")?;
                let rows = stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let same: Vec<String> = rows
                    .into_iter()
                    .filter(|(_, p)| self.titles.contains(&title_key(p)))
                    .map(|(id, _)| id)
                    .collect();
                self.found.extend(store::referrers(conn, &same)?);
                i64::MIN
            }
        };
        let finder = aho_corasick::AhoCorasick::new(&self.titles)
            .map_err(|e| CoreError::Invalid(format!("Cannot look for links to the pages: {e}")))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT rowid, id, body FROM pages WHERE rowid > ?1 AND path IS NOT NULL AND {}
             ORDER BY rowid LIMIT ?2",
            store::UNRESOLVED_LINKS
        ))?;
        let mut rows = 0;
        let mut last = after;
        for row in stmt.query_map(params![after, SEARCH_ROWS], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })? {
            let (rowid, id, body) = row?;
            rows += 1;
            last = rowid;
            if !self.written.contains(&id) && store::may_name(&body, &finder) {
                self.found.insert(id);
            }
        }
        if rows < SEARCH_ROWS {
            self.finish();
        } else {
            self.searched_to = Some(last);
        }
        Ok(true)
    }

    fn finish(&mut self) {
        let found = std::mem::take(&mut self.found);
        self.pages = Some(found.into_iter().filter(|id| !self.written.contains(id)).collect());
    }
}

// Count the files read again, for tests that a change reads no more than it must.
#[cfg(test)]
thread_local! {
    pub(super) static REREADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A page's file read again after pages it links to (or could) changed: its index text found
/// anew ([`store::index_body`]), while the file is the one the index read (else the watcher reads
/// it as a change of its own). Whether the text changed.
fn reread(
    conn: &Connection,
    ext: &Ext,
    exact: &Exact,
    look: &store::DbFolder,
    id: &str,
    arrived: &HashSet<String>,
) -> Result<bool> {
    let Some((path, hash)) = page_row(conn, id)? else { return Ok(false) };
    let Some(meta) = exact.file_meta(ext, &path) else { return Ok(false) };
    #[cfg(test)]
    REREADS.with(|n| n.set(n.get() + 1));
    let Ok(read) = read_file(ext, &path, &meta) else { return Ok(false) };
    if hash.as_deref() != Some(read.hash.as_str()) {
        return Ok(false);
    }
    let mut p = knowledge::get_page(conn, id)?;
    let body = store::index_body(look, &read.parsed.body, &path, &p.body, &|a| arrived.contains(a));
    if body == p.body {
        return Ok(false);
    }
    p.body = body;
    conn.execute("UPDATE pages SET body = ?2 WHERE id = ?1", params![id, p.body])?;
    crate::hooks::page_saved(conn, &p)?;
    Ok(true)
}

/// Drops the data of pages and groups whose files have been gone longer than [`TOMBSTONE_DAYS`].
fn expire_tombstones(conn: &Connection) -> Result<()> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(TOMBSTONE_DAYS))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    conn.execute("DELETE FROM removed_pages WHERE removed_at < ?1", [&cutoff])?;
    conn.execute("DELETE FROM removed_groups WHERE removed_at < ?1", [&cutoff])?;
    Ok(())
}

fn apply_folder(conn: &Connection, ext: &Ext, exact: &Exact, f: &FolderChange, summary: &mut ScanSummary) -> Result<()> {
    match f {
        FolderChange::NewCategory { folder } => {
            // A category made meanwhile (in Cairn, or development's demo data) with that name and
            // no folder yet takes the folder instead of a second category.
            if store::category_of_folder(conn, folder)?.is_none()
                && exact.is_dir(ext, folder)
                && !store::adopt_category_for_folder(conn, folder)?
            {
                store::new_category_for_folder(conn, folder)?;
                summary.categories_added += 1;
            }
        }
        FolderChange::LinkCategory { id, folder } => {
            if store::category_of_folder(conn, folder)?.is_none() {
                conn.execute(
                    "UPDATE categories SET folder = ?2 WHERE id = ?1 AND folder IS NULL",
                    params![id, folder],
                )?;
            }
        }
        FolderChange::RenameCategory { id, folder } => {
            if store::category_of_folder(conn, folder)?.is_none() {
                // Its groups follow through the group renames planned with it.
                conn.execute(
                    "UPDATE categories SET name = ?2, folder = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, folder, crate::util::now_iso()],
                )?;
            }
        }
        FolderChange::RenameGroup { id, path } => {
            let exists = conn
                .query_row("SELECT 1 FROM groups WHERE path = ?1", [path], |_| Ok(()))
                .optional()?
                .is_some();
            if !exists {
                let (category_id, parent_id) = store::place_of_dir(conn, path)?;
                if let Some(category_id) = category_id {
                    conn.execute(
                        "UPDATE groups SET path = ?2, name = ?3, category_id = ?4, parent_id = ?5, updated_at = ?6
                         WHERE id = ?1",
                        params![id, path, name_of(path), category_id, parent_id, crate::util::now_iso()],
                    )?;
                }
            }
        }
        FolderChange::NewGroup { path } => {
            if store::group_of_dir(conn, path)?.is_none() {
                // A folder that comes back gets its group's id and description again.
                let added = match revive_group(conn, path)? {
                    true => true,
                    false => store::ensure_group_rows(conn, path)?.is_some(),
                };
                if added {
                    summary.groups_added += 1;
                }
            }
        }
    }
    Ok(())
}

/// The group of a folder that was removed and is back (same path, or the same in another case):
/// made again with its old id and description. Whether it was.
fn revive_group(conn: &Connection, path: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT id, path, description, created_at FROM removed_groups ORDER BY removed_at DESC")?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    let lower = path.to_lowercase();
    let Some((id, _, description, created)) = rows
        .iter()
        .find(|r| r.1 == path)
        .or_else(|| rows.iter().find(|r| r.1.to_lowercase() == lower))
        .cloned()
    else {
        return Ok(false);
    };
    let in_use = conn
        .query_row("SELECT 1 FROM groups WHERE id = ?1", [&id], |_| Ok(()))
        .optional()?
        .is_some();
    let (category_id, parent_id) = store::place_of_dir(conn, path)?;
    let Some(category_id) = category_id.filter(|_| !in_use) else {
        return Ok(false);
    };
    conn.execute(
        "INSERT INTO groups (id, category_id, parent_id, name, description, path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, category_id, parent_id, name_of(path), description, path, created, crate::util::now_iso()],
    )?;
    conn.execute("DELETE FROM removed_groups WHERE id = ?1", [&id])?;
    Ok(true)
}

/// The file at `path` is there exactly so and still has the size and time it had when it was read.
fn still(ext: &Ext, exact: &Exact, path: &str, stamp: &Stamp) -> bool {
    exact.file_meta(ext, path).is_some_and(|m| stamp_of(&m) == *stamp)
}

/// Another row's path names the same file as `path` in another case (a file system that ignores
/// case): adding `path` would index one file twice.
fn same_file_indexed(ext: &Ext, folded: &Folded, path: &str) -> bool {
    let Some(abs) = ext.abs(path) else { return false };
    folded
        .others(path)
        .any(|p| ext.abs(p).is_some_and(|other| super::same_file(&other, &abs)))
}

/// FR-KN-EXT-4: the page's file is gone; its Cairn-only data is kept (with the rows of its history
/// and task links) to give back if the file returns, then its rows go.
fn bury(conn: &Connection, page: &Page) -> Result<()> {
    let versions: Vec<(String, String)> = conn
        .prepare("SELECT day, snapshot FROM page_versions WHERE page_id = ?1 ORDER BY day")?
        .query_map([&page.id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let links: Vec<(String, String, String)> = conn
        .prepare("SELECT id, task_id, created_at FROM task_page_links WHERE page_id = ?1")?
        .query_map([&page.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    conn.execute(
        "INSERT OR REPLACE INTO removed_pages (id, path, file_hash, origin, origin_task_id, check_status,
                                               created_at, versions, task_links, removed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            page.id,
            page.path,
            page.file_hash,
            crate::tasks::enum_str(&page.origin),
            page.origin_task_id,
            crate::tasks::enum_str(&page.check_status),
            page.created_at,
            serde_json::to_string(&versions)?,
            serde_json::to_string(&links)?,
            crate::util::now_iso(),
        ],
    )?;
    knowledge::delete_page_rows(conn, page)?;
    Ok(())
}

/// A buried page's history rows (day, snapshot) and task links (link id, task id, created).
type KeptRows = (Vec<(String, String)>, Vec<(String, String, String)>);

/// A page whose file came back: its Cairn-only data from [`bury`], if it is still kept.
fn unbury(conn: &Connection, p: &mut Page) -> Result<Option<KeptRows>> {
    let row = conn
        .query_row(
            "SELECT origin, origin_task_id, check_status, created_at, versions, task_links
             FROM removed_pages WHERE id = ?1",
            [&p.id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((origin, task, status, created, versions, links)) = row else {
        return Ok(None);
    };
    conn.execute("DELETE FROM removed_pages WHERE id = ?1", [&p.id])?;
    if let Ok(o) = crate::tasks::parse_enum(0, &origin) {
        p.origin = o;
    }
    if let Ok(c) = crate::tasks::parse_enum(0, &status) {
        p.check_status = c;
    }
    p.origin_task_id = task.filter(|t| {
        conn.query_row("SELECT 1 FROM tasks WHERE id = ?1", [t], |_| Ok(()))
            .optional()
            .ok()
            .flatten()
            .is_some()
    });
    p.created_at = created;
    Ok(Some((
        serde_json::from_str(&versions).unwrap_or_default(),
        serde_json::from_str(&links).unwrap_or_default(),
    )))
}

fn page_row(conn: &Connection, id: &str) -> Result<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row("SELECT path, file_hash FROM pages WHERE id = ?1", [id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .optional()?
        .and_then(|(p, h)| p.map(|p| (p, h))))
}

fn path_taken(conn: &Connection, path: &str) -> Result<Option<(String, Option<String>)>> {
    Ok(conn
        .query_row("SELECT id, file_hash FROM pages WHERE path = ?1", [path], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?)
}

/// The page's fields from a file read.
fn fill_from_read(p: &mut Page, read: &Read, body: &str) {
    p.body = body.to_string();
    p.tags = knowledge::normalize_tags(read.parsed.tags.clone());
    p.source_info = read.parsed.source.as_deref().and_then(knowledge::clean_source);
    p.archived = read.parsed.archived;
    p.file_hash = Some(read.hash.clone());
}

/// Whether the page's text links to a page that isn't in the index (yet).
fn links_ahead(conn: &Connection, id: &str) -> Result<bool> {
    let body: String = conn.query_row("SELECT body FROM pages WHERE id = ?1", [id], |r| r.get(0))?;
    for target in crate::mentions::parse(&body).0 {
        let known = conn
            .query_row("SELECT 1 FROM pages WHERE id = ?1", [&target], |_| Ok(()))
            .optional()?
            .is_some();
        if !known {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Applies one change; returns the page written (added, changed or renamed), if any. What it
/// added, removed or renamed goes into `relink`.
#[allow(clippy::too_many_arguments)]
fn apply_page(
    conn: &Connection,
    ext: &Ext,
    exact: &Exact,
    folded: &mut Folded,
    change: &PageChange,
    clock: &Clock,
    summary: &mut ScanSummary,
    relink: &mut Relink,
) -> Result<Option<String>> {
    let mut written = None;
    match change {
        PageChange::Remove { id, path } => {
            if page_row(conn, id)?.is_some_and(|(p, _)| p == *path) && !exact.is_file(ext, path) {
                let page = knowledge::get_page(conn, id)?;
                relink.found.extend(store::linking_to(conn, id, path)?);
                bury(conn, &page)?;
                folded.remove(path);
                summary.removed += 1;
            }
        }
        PageChange::Rename { id, from, to, read, body } => {
            let at_from = page_row(conn, id)?.is_some_and(|(p, _)| p == *from);
            if !at_from || path_taken(conn, to)?.is_some() || !still(ext, exact, to, &read.stamp) {
                return Ok(None);
            }
            let (category_id, group_id) = store::place_of_file(conn, to)?;
            let Some(category_id) = category_id else { return Ok(None) };
            let mut p = knowledge::get_page(conn, id)?;
            // The same text as before: its links to pages it no longer names (seen from its new
            // place) keep them, as links to it do (FR-KN-EXT-16).
            let exists = |page: &str| page == id || page_row(conn, page).ok().flatten().is_some();
            let arrived = |page: &str| relink.arrived.contains(page);
            let body = convert::keep_links(&p.body, body, &|was, now| store::still_meant(&exists, &arrived, was, now));
            p.category_id = category_id;
            p.group_id = group_id;
            p.title = title_of(to);
            p.path = Some(to.clone());
            fill_from_read(&mut p, read, &body);
            knowledge::commit_from_folder(conn, &mut p, &read.stamp, clock, &read.modified)?;
            folded.remove(from);
            folded.add(to);
            relink.arrived.insert(id.clone());
            relink.titles.insert(title_key(to));
            summary.renamed += 1;
            written = Some(id.clone());
        }
        PageChange::Change { id, path, read, body } => {
            let Some((at, hash)) = page_row(conn, id)? else { return Ok(None) };
            if at != *path || hash.as_deref() == Some(read.hash.as_str()) || !still(ext, exact, path, &read.stamp) {
                if at == *path && hash.as_deref() == Some(read.hash.as_str()) {
                    store::set_file_stamp(conn, id, &read.stamp)?;
                }
                return Ok(None);
            }
            let mut p = knowledge::get_page(conn, id)?;
            fill_from_read(&mut p, read, body);
            knowledge::commit_from_folder(conn, &mut p, &read.stamp, clock, &read.modified)?;
            relink.written.insert(id.clone());
            summary.changed += 1;
            written = Some(id.clone());
        }
        PageChange::Touch { id, path, stamp } => {
            if page_row(conn, id)?.is_some_and(|(p, _)| p == *path) {
                store::set_file_stamp(conn, id, stamp)?;
            }
        }
        PageChange::Add { id, path, read, body } => {
            // Taken: Cairn made it meanwhile (its own write) or another pass found it. The same
            // file under another case is indexed already. Changed again since it was read: the
            // next pass reads it.
            if path_taken(conn, path)?.is_some()
                || same_file_indexed(ext, folded, path)
                || !still(ext, exact, path, &read.stamp)
            {
                return Ok(None);
            }
            let in_use = conn
                .query_row("SELECT 1 FROM pages WHERE id = ?1", [id], |_| Ok(()))
                .optional()?
                .is_some();
            if in_use {
                return Ok(None);
            }
            let (category_id, group_id) = store::place_of_file(conn, path)?;
            let Some(category_id) = category_id else { return Ok(None) };
            let mut p = Page {
                id: id.clone(),
                category_id,
                group_id,
                title: title_of(path),
                body: body.clone(),
                tags: knowledge::normalize_tags(read.parsed.tags.clone()),
                source_info: read.parsed.source.as_deref().and_then(knowledge::clean_source),
                origin: PageOrigin::Folder,
                origin_task_id: None,
                check_status: CheckStatus::Checked,
                archived: read.parsed.archived,
                created_at: read.created.clone().unwrap_or_else(|| clock.at.clone()),
                updated_at: read.modified.clone(),
                path: Some(path.clone()),
                file_hash: Some(read.hash.clone()),
            };
            // FR-KN-EXT-4: a file that comes back gets its Cairn-only data again.
            let kept = unbury(conn, &mut p)?;
            knowledge::insert_from_folder(conn, &p, &read.stamp, clock)?;
            if let Some((versions, links)) = kept {
                for (day, snapshot) in versions {
                    conn.execute(
                        "INSERT OR IGNORE INTO page_versions (page_id, day, snapshot) VALUES (?1, ?2, ?3)",
                        params![p.id, day, snapshot],
                    )?;
                }
                for (link_id, task_id, created) in links {
                    conn.execute(
                        "INSERT OR IGNORE INTO task_page_links (id, task_id, page_id, created_at)
                         SELECT ?1, ?2, ?3, ?4 WHERE EXISTS (SELECT 1 FROM tasks WHERE id = ?2)",
                        params![link_id, task_id, p.id, created],
                    )?;
                }
            }
            folded.add(path);
            relink.written.insert(p.id.clone());
            relink.arrived.insert(p.id.clone());
            relink.titles.insert(title_key(path));
            summary.added += 1;
            written = Some(p.id);
        }
        PageChange::RemoveGroup { path } => {
            if !exact.is_dir(ext, path)
                && let Some(id) = store::group_of_dir(conn, path)?
            {
                // Its description is kept, for a folder that comes back.
                conn.execute(
                    "INSERT OR REPLACE INTO removed_groups (id, path, description, created_at, removed_at)
                     SELECT id, path, description, created_at, ?2 FROM groups WHERE id = ?1",
                    params![id, crate::util::now_iso()],
                )?;
                conn.execute("DELETE FROM groups WHERE id = ?1", [&id])?;
                summary.groups_removed += 1;
            }
        }
    }
    Ok(written)
}

/// Scans the whole folder (or `scope`) and applies everything, holding `db` throughout (switching
/// modes, tests, tools). The app uses the three steps instead.
pub fn scan(db: &mut Db, scope: Scope) -> Result<ScanSummary> {
    let job = prepare(db.conn(), scope)?;
    let mut plan = job.read(&mut |_| {})?;
    while plan.apply_some(db, BATCH)? > 0 {}
    Ok(plan.summary().clone())
}

/// Whether a path names something a scan reads (not hidden, inside the root).
pub fn relevant(rel: &str) -> bool {
    !rel.is_empty() && !rel.split('/').any(is_hidden)
}

/// The root-relative paths of absolute ones (for watcher batches); others are dropped.
pub fn relative_paths(ext: &Ext, abs: &[std::path::PathBuf]) -> Vec<String> {
    let mut out: Vec<String> = abs
        .iter()
        .filter_map(|p| ext.rel(Path::new(p)))
        .filter(|p| relevant(p))
        .collect();
    out.sort();
    out.dedup();
    out
}
