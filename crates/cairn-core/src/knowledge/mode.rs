//! Where knowledge lives, and changing it (spec §5.11, FR-KN-EXT-7, -8, -9, -14; D-72).
//!
//! - **Choosing** ([`choose_mode`]): a database made in this run has no mode until the user makes
//!   the full-screen choice; an older one is internal (`settings`). Development builds can preset
//!   it with `CAIRN_DEV_KNOWLEDGE_MODE` ([`dev_preset`]).
//! - **Switching** (Settings › Data), both ways, exactly in this order:
//!   1. a full backup (the same zip as a daily backup) goes to `<data>/switch-backups/`, named
//!      with the local date and time, the mode and the page count; Cairn never deletes these;
//!   2. the old mode's knowledge is cleared: pages with their tags, history and task links,
//!      groups, the mention, search, vector and glossary-use rows of pages, page drafts and
//!      activity, and the images only pages used. Tasks, categories, the glossary and
//!      conversations stay;
//!   3. the new mode is recorded. Going **external**, the index of that folder is first put back
//!      from its newest switch backup (matched by the root; pages keep their ids and Cairn-only
//!      data by path), then a scan brings it up to the folder. Going **internal**, the knowledge
//!      part of the chosen switch backup is loaded (categories that are missing, groups, pages,
//!      tags, history, task links whose task still exists, and the images their text uses), or
//!      nothing (start with no pages). Only a switch backup made in internal mode can be chosen
//!      (the owner's choice): one made in a folder is refused before anything happens;
//!   4. the derived indexes are rebuilt and every page piece waits to be embedded again.
//!
//!   Switching never changes the folder. Changing to another folder while in external mode
//!   ([`switch_external_root`]) is one switch with one backup, as going internal and back to the
//!   new folder would be.
//! - **Switch backups** ([`switch_backups`]): listed with date, mode, page count and size; their
//!   total is compared with `switchBackupWarnGb` (FR-KN-EXT-9). They hold **knowledge only**
//!   (`export::export_knowledge`: categories, groups, pages with tags, history and task links, and
//!   the images they use), so they are loaded by switching, never imported. Older full ones load
//!   the same way. **Only the newest switch backup of each folder is kept** (the owner's choice):
//!   when Cairn leaves a folder (for internal mode or another folder), the older switch backups of
//!   that folder (matched by the folder itself) are deleted for good once the new one is written —
//!   unless putting that folder's Cairn-only data back failed when Cairn last entered it
//!   (recorded in the settings, [`ENTRY_KEY`]), so nothing is lost twice. Switch backups made in
//!   internal mode are never deleted by Cairn.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::external::{self, scan};
use crate::export::{self, archive_info};
use crate::settings::{self, KnowledgeMode};
use crate::{CoreError, Db, Result, attachments, hooks};

/// Where knowledge lives now (Settings › Data's *Knowledge storage* card, the mode chooser).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeStorage {
    /// `None` until the user chooses (a database made in this run).
    pub mode: Option<KnowledgeMode>,
    /// External mode: the root folder.
    pub root: Option<String>,
    /// External mode: whether the root folder is there now.
    pub root_exists: bool,
    /// External mode: why changes made by other programs aren't picked up live (the folder can't
    /// be watched); `None` when it is watched. Filled in by the app shell, which owns the watcher.
    pub watch_error: Option<String>,
}

pub fn storage(conn: &Connection) -> Result<KnowledgeStorage> {
    let (mode, root) = settings::knowledge_mode(conn)?;
    let root = root.filter(|_| mode == Some(KnowledgeMode::External));
    Ok(KnowledgeStorage {
        root_exists: root.as_deref().is_some_and(|r| Path::new(r).is_dir()),
        mode,
        root,
        watch_error: None,
    })
}

fn word(mode: KnowledgeMode) -> &'static str {
    match mode {
        KnowledgeMode::Internal => "internal",
        KnowledgeMode::External => "external",
    }
}

/// A folder chosen as the root: a full path to a folder Cairn can write to, apart from Cairn's own
/// data folder. Returns it as it will be recorded.
pub fn check_root(db: &Db, root: &str) -> Result<String> {
    let root = root.trim();
    if root.is_empty() {
        return Err(CoreError::Invalid("Choose a folder for the knowledge.".into()));
    }
    let path = Path::new(root);
    if !path.is_absolute() {
        return Err(CoreError::Invalid(
            "The knowledge folder must be a full path, for example /home/you/Notes.".into(),
        ));
    }
    if !path.is_dir() {
        return Err(CoreError::Invalid(format!("The folder \"{root}\" can't be found.")));
    }
    let data = std::fs::canonicalize(&db.paths().root).unwrap_or_else(|_| db.paths().root.clone());
    let chosen = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if chosen.starts_with(&data) || data.starts_with(&chosen) {
        return Err(CoreError::Invalid(
            "The knowledge folder can't be Cairn's data folder, inside it, or around it. Choose another folder.".into(),
        ));
    }
    let probe = path.join(".cairn-write-test");
    std::fs::write(&probe, b"")
        .map_err(|e| CoreError::Invalid(format!("Cairn can't write to that folder: {e}")))?;
    let _ = std::fs::remove_file(probe);
    Ok(recorded_root(root))
}

/// A root as it is recorded: without a separator at the end, except for the top of a disk
/// (`/`, `E:\`), where the separator is the path.
fn recorded_root(root: &str) -> String {
    let trimmed = root.trim_end_matches(['/', '\\']);
    let top_of_disk = trimmed.is_empty()
        || trimmed.ends_with(':')
        || Path::new(trimmed).parent().is_none();
    if top_of_disk { root.to_string() } else { trimmed.to_string() }
}

/// Whether two recorded roots name the same folder.
fn same_root(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let t = s.trim().trim_end_matches(['/', '\\']).replace('\\', "/");
        if cfg!(windows) { t.to_lowercase() } else { t }
    };
    norm(a) == norm(b)
}

/// FR-KN-EXT-7: the first-start choice. Refused once a mode is recorded (switching is in
/// Settings). In external mode the folder still needs its first scan (`external::scan`).
pub fn choose_mode(db: &mut Db, mode: KnowledgeMode, root: Option<&str>) -> Result<KnowledgeStorage> {
    if settings::knowledge_mode(db.conn())?.0.is_some() {
        return Err(CoreError::Invalid(
            "Where knowledge is kept was chosen already. Change it in Settings › Data.".into(),
        ));
    }
    let root = match mode {
        KnowledgeMode::Internal => None,
        KnowledgeMode::External => {
            let pages: i64 = db.conn().query_row("SELECT COUNT(*) FROM pages", [], |r| r.get(0))?;
            if pages > 0 {
                return Err(CoreError::Invalid(
                    "This data already has pages in Cairn. Choose internal, then switch in Settings › Data.".into(),
                ));
            }
            Some(check_root(db, root.unwrap_or(""))?)
        }
    };
    db.tx(|tx| settings::set_knowledge_mode(tx, mode, root.as_deref()))?;
    tracing::info!(mode = word(mode), "knowledge storage chosen");
    storage(db.conn())
}

/// Development builds: `CAIRN_DEV_KNOWLEDGE_MODE` = `internal`, `external=<folder>` or
/// `external=sample` presets the mode of a database that has none yet (a fresh data folder), so
/// probes skip the chooser. The folder is made if it is missing; `sample` is a copy of the sample
/// folder ([`dev_copy_sample`]). Returns whether it set the mode.
pub fn dev_preset(db: &mut Db, value: &str) -> Result<bool> {
    if settings::knowledge_mode(db.conn())?.0.is_some() {
        return Ok(false);
    }
    let value = value.trim();
    if value == "internal" {
        choose_mode(db, KnowledgeMode::Internal, None)?;
        return Ok(true);
    }
    let Some(folder) = value.strip_prefix("external=") else {
        return Err(CoreError::Invalid(format!(
            "CAIRN_DEV_KNOWLEDGE_MODE must be \"internal\", \"external=<folder>\" or \"external=sample\", not \"{value}\"."
        )));
    };
    let folder = if folder == "sample" {
        dev_copy_sample(db)?.to_string_lossy().into_owned()
    } else {
        std::fs::create_dir_all(folder)?;
        folder.to_string()
    };
    choose_mode(db, KnowledgeMode::External, Some(&folder))?;
    Ok(true)
}

/// The editable external sample folder for development (FR-KN-EXT-7): categories, nested
/// groups, front matter, both link styles, images, a Chinese page, a non-UTF-8 file and a hidden
/// folder. In the source tree, so only development builds use it.
pub const SAMPLE_FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/external-sample");

/// Development builds: a copy of the sample folder next to the data folder
/// (`<data folder>-sample-knowledge`, since a root can't be inside the data folder), made once.
pub fn dev_copy_sample(db: &Db) -> Result<PathBuf> {
    fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            let target = to.join(e.file_name());
            if e.file_type()?.is_dir() {
                copy(&e.path(), &target)?;
            } else if !target.exists() {
                std::fs::copy(e.path(), target)?;
            }
        }
        Ok(())
    }
    #[cfg(not(test))]
    let source = Path::new(SAMPLE_FOLDER);
    // Unit tests must not read the developer's editable, ignored sample folder.
    #[cfg(test)]
    let source = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/external-tests"));
    if !source.is_dir() {
        return Err(CoreError::Invalid(
            "The sample knowledge folder is only in development builds.".into(),
        ));
    }
    let mut name = db.paths().root.as_os_str().to_owned();
    name.push("-sample-knowledge");
    let target = PathBuf::from(name);
    copy(source, &target)?;
    Ok(target)
}

// ---------------------------------------------------------------- switch backups

/// The folder of the switch backups, inside the data folder.
pub fn switch_backups_dir(db: &Db) -> PathBuf {
    db.paths().root.join("switch-backups")
}

/// One switch backup (FR-KN-EXT-9's list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SwitchBackup {
    pub file_name: String,
    pub path: String,
    /// UTC, RFC 3339, from the manifest.
    pub exported_at: String,
    /// The mode the data was in when the backup was made.
    pub mode: KnowledgeMode,
    /// External mode: the root folder then.
    pub root: Option<String>,
    pub pages: u32,
    /// Bytes.
    pub size: u64,
    /// Knowledge only (made by 0.13.0 or later); `false`: a full export made by an earlier switch,
    /// which also imports.
    pub knowledge_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SwitchBackups {
    pub folder: String,
    /// Newest first.
    pub backups: Vec<SwitchBackup>,
    /// Bytes, all of them together.
    pub total_size: u64,
    /// `switchBackupWarnGb` in bytes (1 GB = 1024³ bytes).
    pub warn_size: u64,
    /// FR-KN-EXT-9: past the warning size, every start shows the clean-up pop-up.
    pub over_warning: bool,
}

fn describe(path: &Path) -> Option<SwitchBackup> {
    let info = archive_info(path).ok()?;
    Some(SwitchBackup {
        file_name: path.file_name()?.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        exported_at: info.exported_at,
        mode: info.mode.unwrap_or(KnowledgeMode::Internal),
        root: info.root,
        pages: info.pages as u32,
        size: std::fs::metadata(path).ok()?.len(),
        knowledge_only: info.knowledge_only,
    })
}

fn list_backups(dir: &Path) -> Vec<SwitchBackup> {
    let mut out: Vec<SwitchBackup> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.file_type().is_ok_and(|t| t.is_file())
                        && e.file_name().to_string_lossy().ends_with(".zip")
                })
                .filter_map(|e| describe(&e.path()))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.exported_at.cmp(&a.exported_at).then(b.file_name.cmp(&a.file_name)));
    out
}

/// The switch backups, their total size and whether it is past the warning size (FR-KN-EXT-9).
pub fn switch_backups(db: &Db) -> Result<SwitchBackups> {
    let dir = switch_backups_dir(db);
    let backups = list_backups(&dir);
    let total_size: u64 = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);
    let gb = settings::get(db.conn())?.switch_backup_warn_gb.max(0.0) as f64;
    let warn_size = (gb * 1024.0 * 1024.0 * 1024.0) as u64;
    Ok(SwitchBackups {
        folder: dir.to_string_lossy().into_owned(),
        over_warning: total_size > warn_size,
        backups,
        total_size,
        warn_size,
    })
}

/// The switch backup `file_name` (a name in the folder, nothing else).
fn backup_file(db: &Db, file_name: &str) -> Result<PathBuf> {
    let bad = file_name.is_empty()
        || file_name.contains(['/', '\\'])
        || file_name.starts_with('.')
        || !file_name.ends_with(".zip");
    let path = switch_backups_dir(db).join(file_name);
    if bad || !path.is_file() {
        return Err(CoreError::NotFound {
            entity: "switch backup",
            id: file_name.to_string(),
        });
    }
    Ok(path)
}

/// *Delete* in the switch backup list (FR-KN-EXT-9). Only the user deletes these.
pub fn delete_switch_backup(db: &Db, file_name: &str) -> Result<()> {
    let path = backup_file(db, file_name)?;
    std::fs::remove_file(path)?;
    Ok(())
}

/// Step 1 of a switch: a knowledge-only backup in `switch-backups`, named with the local date and
/// time, the mode and the page count, e.g. `2026-10-20-1530-internal-42 pages.zip`.
fn make_switch_backup(db: &Db) -> Result<SwitchBackup> {
    let conn = db.conn();
    let mode = settings::knowledge_mode(conn)?.0.unwrap_or(KnowledgeMode::Internal);
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM pages", [], |r| r.get(0))?;
    let dir = switch_backups_dir(db);
    std::fs::create_dir_all(&dir)?;
    let stem = format!(
        "{}-{}-{n} {}",
        chrono::Local::now().format("%Y-%m-%d-%H%M"),
        word(mode),
        if n == 1 { "page" } else { "pages" }
    );
    let mut path = dir.join(format!("{stem}.zip"));
    let mut k = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({k}).zip"));
        k += 1;
    }
    export::export_knowledge(db, &path)?;
    tracing::info!(pages = n, mode = word(mode), "switch backup made");
    describe(&path).ok_or_else(|| CoreError::Invalid("The switch backup could not be read back.".into()))
}

// ---------------------------------------------------------------- clearing and loading

/// Step 2 of a switch: the old mode's knowledge goes (see the module comment).
fn clear_knowledge(db: &mut Db) -> Result<()> {
    let candidates = db.tx(|tx| {
        let mut names = std::collections::BTreeSet::new();
        let mut stmt = tx.prepare("SELECT body FROM pages UNION ALL SELECT snapshot FROM page_versions")?;
        let texts = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(stmt);
        for t in texts {
            names.extend(attachments::referenced_names(&t));
        }
        tx.execute_batch(
            "DELETE FROM mentions WHERE source_kind = 'page' OR target_kind = 'page';
             DELETE FROM glossary_uses WHERE kind = 'page';
             DELETE FROM search_index WHERE rowid IN (SELECT rowid FROM search_ref WHERE kind = 'page');
             DELETE FROM search_ref WHERE kind = 'page';
             DELETE FROM chunks WHERE kind = 'page';
             DELETE FROM activity WHERE ref_id IN (SELECT id FROM pages);
             DELETE FROM drafts WHERE kind = 'page';
             DELETE FROM pages;
             DELETE FROM groups;
             DELETE FROM removed_pages;
             DELETE FROM removed_groups;
             UPDATE categories SET folder = NULL;",
        )?;
        Ok(names)
    })?;
    if let Err(e) = attachments::collect_among(db, &candidates) {
        tracing::warn!(error = %e, "could not remove the images of the cleared pages");
    }
    Ok(())
}

/// Runs `f` with the staged archive attached as `sb`.
fn with_attached<T>(db: &mut Db, staged: &export::Staged, f: impl FnOnce(&rusqlite::Transaction) -> Result<T>) -> Result<T> {
    db.conn().execute(
        "ATTACH DATABASE ?1 AS sb",
        [staged.db_file().to_string_lossy()],
    )?;
    let out = db.tx(f);
    db.conn().execute("DETACH DATABASE sb", [])?;
    out
}

fn exists(conn: &Connection, sql: &str, args: impl rusqlite::Params) -> Result<bool> {
    Ok(conn.query_row(sql, args, |_| Ok(())).optional()?.is_some())
}

/// A group of the attached switch backup.
struct GroupRow {
    id: String,
    category_id: String,
    parent_id: Option<String>,
    name: String,
    description: String,
    path: Option<String>,
    created_at: String,
    updated_at: String,
}

/// The attached backup's groups, parents before their groups (shorter paths first).
fn staged_groups(tx: &Connection) -> Result<Vec<GroupRow>> {
    let mut stmt = tx.prepare(
        "SELECT id, category_id, parent_id, name, description, path, created_at, updated_at
         FROM sb.groups ORDER BY length(path), path, id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(GroupRow {
                id: r.get(0)?,
                category_id: r.get(1)?,
                parent_id: r.get(2)?,
                name: r.get(3)?,
                description: r.get(4)?,
                path: r.get(5)?,
                created_at: r.get(6)?,
                updated_at: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// What loading a page of the attached switch backup needs to decide.
struct PageRow {
    id: String,
    category_id: String,
    group_id: Option<String>,
    origin_task_id: Option<String>,
    path: Option<String>,
}

/// The attached backup's pages, oldest first.
fn staged_pages(tx: &Connection) -> Result<Vec<PageRow>> {
    let mut stmt = tx.prepare(
        "SELECT id, category_id, group_id, origin_task_id, path FROM sb.pages ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(PageRow {
                id: r.get(0)?,
                category_id: r.get(1)?,
                group_id: r.get(2)?,
                origin_task_id: r.get(3)?,
                path: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Where a folder's earlier Cairn-only data comes from when Cairn enters it.
enum RestoreSource {
    /// No switch backup of that folder.
    Nothing,
    Staged(export::Staged),
    /// Its newest switch backup can't be read.
    Unreadable,
}

/// FR-KN-EXT-8: the newest switch backup made in external mode with `root`, built and checked
/// before anything is cleared. One that can't be read is skipped with a warning: the switch goes
/// on without it.
fn stage_restore_source(db: &Db, root: &str) -> RestoreSource {
    let Some(backup) = list_backups(&switch_backups_dir(db))
        .into_iter()
        .find(|b| b.mode == KnowledgeMode::External && b.root.as_deref().is_some_and(|r| same_folder(r, root)))
    else {
        return RestoreSource::Nothing;
    };
    match export::stage(db, Path::new(&backup.path)) {
        Ok(staged) => RestoreSource::Staged(staged),
        Err(e) => {
            tracing::warn!(error = %e, file = %backup.file_name, "the folder's earlier switch backup can't be read; its Cairn-only data is not put back");
            RestoreSource::Unreadable
        }
    }
}

/// The settings row that records, for the folder Cairn entered last, whether its Cairn-only data
/// was put back (`{"root": …, "ok": …}`). Older switch backups of a folder are deleted only after
/// an entry that worked.
pub const ENTRY_KEY: &str = "externalEntry";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    root: String,
    ok: bool,
}

/// Steps 3–4 of entering a folder: its earlier index from `source`, and a record of whether that
/// worked ("nothing to put back" counts as worked). Returns the pages put back.
fn enter_folder(db: &mut Db, root: &str, source: RestoreSource) -> Result<u32> {
    let (restored, ok) = match source {
        RestoreSource::Nothing => (0, true),
        RestoreSource::Unreadable => (0, false),
        RestoreSource::Staged(staged) => match restore_index(db, &staged) {
            Ok(n) => (n, true),
            Err(e) => {
                tracing::warn!(error = %e, "the folder's Cairn-only data could not be put back");
                (0, false)
            }
        },
    };
    let entry = serde_json::to_string(&Entry {
        root: root.to_string(),
        ok,
    })?;
    db.tx(|tx| {
        tx.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ENTRY_KEY, entry],
        )?;
        Ok(())
    })?;
    Ok(restored)
}

/// Whether Cairn's last entry into `root` put its Cairn-only data back (or had none to put back).
/// No record (a database from before this rule, or one about another folder) counts as yes.
fn entry_worked(conn: &Connection, root: &str) -> Result<bool> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [ENTRY_KEY], |r| r.get(0))
        .optional()?;
    Ok(match raw.and_then(|v| serde_json::from_str::<Entry>(&v).ok()) {
        Some(e) if same_folder(&e.root, root) => e.ok,
        _ => true,
    })
}

/// Only the newest switch backup per folder (the owner's choice): after `newest` (a backup of the
/// folder `root` Cairn is leaving) is written, that folder's other switch backups are deleted for
/// good — unless entering the folder last time failed to put its data back. Internal-mode switch
/// backups and other folders' are never touched. Failures only get logged.
fn keep_newest_of_folder(db: &Db, root: &str, newest: &SwitchBackup) -> Result<()> {
    if !entry_worked(db.conn(), root)? {
        tracing::info!("older switch backups of the folder are kept: its data was not put back when Cairn entered it");
        return Ok(());
    }
    for b in list_backups(&switch_backups_dir(db)) {
        let same = b.mode == KnowledgeMode::External
            && b.file_name != newest.file_name
            && b.root.as_deref().is_some_and(|r| same_folder(r, root));
        if same && let Err(e) = std::fs::remove_file(&b.path) {
            tracing::warn!(error = %e, file = %b.file_name, "could not delete an older switch backup");
        }
    }
    Ok(())
}

/// FR-KN-EXT-8: returning to a folder used before puts its index back from its newest switch
/// backup (staged by [`stage_restore_source`]), so pages keep their ids and Cairn-only data; the
/// scan that follows brings it up to the folder. Returns the pages put back.
fn restore_index(db: &mut Db, staged: &export::Staged) -> Result<u32> {
    let restored = with_attached(db, staged, |tx| {
        let folders: Vec<(String, String)> = tx
            .prepare("SELECT id, folder FROM sb.categories WHERE folder IS NOT NULL")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, folder) in folders {
            tx.execute(
                "UPDATE main.categories SET folder = ?2 WHERE id = ?1 AND folder IS NULL
                   AND NOT EXISTS (SELECT 1 FROM main.categories WHERE folder = ?2)",
                params![id, folder],
            )?;
        }
        for g in staged_groups(tx)? {
            let Some(path) = &g.path else { continue };
            if !exists(tx, "SELECT 1 FROM main.categories WHERE id = ?1", [&g.category_id])?
                || exists(tx, "SELECT 1 FROM main.groups WHERE id = ?1 OR path = ?2", [&g.id, path])?
            {
                continue;
            }
            let parent = match g.parent_id {
                Some(p) if exists(tx, "SELECT 1 FROM main.groups WHERE id = ?1", [&p])? => Some(p),
                _ => None,
            };
            tx.execute(
                "INSERT INTO main.groups (id, category_id, parent_id, name, description, path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![g.id, g.category_id, parent, g.name, g.description, path, g.created_at, g.updated_at],
            )?;
        }
        let mut n = 0;
        for p in staged_pages(tx)? {
            let Some(path) = &p.path else { continue };
            if !exists(tx, "SELECT 1 FROM main.categories WHERE id = ?1", [&p.category_id])?
                || exists(tx, "SELECT 1 FROM main.pages WHERE id = ?1 OR path = ?2", [&p.id, path])?
            {
                continue;
            }
            let group = p.group_id.filter(|g| exists(tx, "SELECT 1 FROM main.groups WHERE id = ?1", [g]).unwrap_or(false));
            let task = p.origin_task_id.filter(|t| exists(tx, "SELECT 1 FROM main.tasks WHERE id = ?1", [t]).unwrap_or(false));
            tx.execute(
                "INSERT INTO main.pages (id, category_id, group_id, title, body, source_info, origin, origin_task_id,
                     check_status, archived, created_at, updated_at, path, file_hash, file_mtime, file_size)
                 SELECT id, category_id, ?2, title, body, source_info, origin, ?3,
                     check_status, archived, created_at, updated_at, path, file_hash, file_mtime, file_size
                 FROM sb.pages WHERE id = ?1",
                params![p.id, group, task],
            )?;
            n += 1;
        }
        copy_page_rows(tx)?;
        Ok(n)
    })?;
    tracing::info!(pages = restored, "index put back from a switch backup");
    Ok(restored)
}

/// Tags, history and task links of the pages now in `main`, from the attached archive.
fn copy_page_rows(tx: &Connection) -> Result<()> {
    tx.execute_batch(
        "INSERT OR IGNORE INTO main.page_tags (page_id, tag)
             SELECT t.page_id, t.tag FROM sb.page_tags t
             WHERE EXISTS (SELECT 1 FROM main.pages p WHERE p.id = t.page_id);
         INSERT OR IGNORE INTO main.page_versions (page_id, day, snapshot)
             SELECT v.page_id, v.day, v.snapshot FROM sb.page_versions v
             WHERE EXISTS (SELECT 1 FROM main.pages p WHERE p.id = v.page_id);
         INSERT OR IGNORE INTO main.task_page_links (id, task_id, page_id, created_at)
             SELECT l.id, l.task_id, l.page_id, l.created_at FROM sb.task_page_links l
             WHERE EXISTS (SELECT 1 FROM main.pages p WHERE p.id = l.page_id)
               AND EXISTS (SELECT 1 FROM main.tasks t WHERE t.id = l.task_id);",
    )?;
    Ok(())
}

/// What going internal with a switch backup refuses: one made in external mode (the owner's
/// choice for 0.13.0; only internal-mode knowledge is loaded into internal mode).
pub const EXTERNAL_BACKUP_REFUSED: &str = "That switch backup is from a folder (external mode). Choose a switch backup made in internal mode, or start with no pages.";

/// The switch backup `file_name` a switch to internal mode may load: one made in internal mode.
pub fn check_internal_backup(db: &Db, file_name: &str) -> Result<PathBuf> {
    let path = backup_file(db, file_name)?;
    let info = archive_info(&path)?;
    if info.mode == Some(KnowledgeMode::External) {
        return Err(CoreError::Invalid(EXTERNAL_BACKUP_REFUSED.into()));
    }
    Ok(path)
}

/// FR-KN-EXT-8, going internal: the knowledge part of an internal-mode switch backup (knowledge
/// only, or an older full one). Returns the pages loaded.
fn load_knowledge(db: &mut Db, backup: &Path) -> Result<u32> {
    let staged = export::stage(db, backup)?;
    let attachments_from = staged.attachments();
    let attachments_to = db.paths().attachments_dir();
    let (n, images) = with_attached(db, &staged, |tx| {
        // Categories that are missing (by id; a key in use is left out).
        tx.execute_batch(
            "INSERT INTO main.categories (id, name, key, description, color, status, is_general,
                                          created_at, updated_at, next_task_number)
                 SELECT s.id, s.name, s.key, s.description, s.color, s.status, 0, s.created_at,
                        s.updated_at, s.next_task_number
                 FROM sb.categories s
                 WHERE s.is_general = 0
                   AND NOT EXISTS (SELECT 1 FROM main.categories c WHERE c.id = s.id OR c.key = s.key);",
        )?;
        for g in staged_groups(tx)? {
            if !exists(tx, "SELECT 1 FROM main.categories WHERE id = ?1", [&g.category_id])? {
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO main.groups (id, category_id, parent_id, name, description, path, created_at, updated_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, NULL, ?5, ?6)",
                params![g.id, g.category_id, g.name, g.description, g.created_at, g.updated_at],
            )?;
        }
        let mut n = 0;
        for p in staged_pages(tx)? {
            if !exists(tx, "SELECT 1 FROM main.categories WHERE id = ?1", [&p.category_id])? {
                continue;
            }
            let group = p.group_id.filter(|g| exists(tx, "SELECT 1 FROM main.groups WHERE id = ?1", [g]).unwrap_or(false));
            let task = p.origin_task_id.filter(|t| exists(tx, "SELECT 1 FROM main.tasks WHERE id = ?1", [t]).unwrap_or(false));
            n += tx.execute(
                "INSERT OR IGNORE INTO main.pages (id, category_id, group_id, title, body, source_info, origin,
                     origin_task_id, check_status, archived, created_at, updated_at)
                 SELECT id, category_id, ?2, title, body, source_info, origin, ?3,
                     check_status, archived, created_at, updated_at
                 FROM sb.pages WHERE id = ?1",
                params![p.id, group, task],
            )? as u32;
        }
        copy_page_rows(tx)?;
        // The images the loaded text uses.
        let texts: Vec<String> = tx
            .prepare("SELECT body FROM main.pages UNION ALL SELECT snapshot FROM main.page_versions")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let images: std::collections::BTreeSet<String> =
            texts.iter().flat_map(|t| attachments::referenced_names(t)).collect();
        for name in &images {
            tx.execute(
                "INSERT OR IGNORE INTO main.attachments (name, mime, size, created_at)
                     SELECT name, mime, size, created_at FROM sb.attachments WHERE name = ?1",
                [name],
            )?;
        }
        Ok((n, images))
    })?;
    std::fs::create_dir_all(&attachments_to)?;
    let mut copied = Vec::new();
    for name in images {
        if name.contains(['/', '\\']) || name.starts_with('.') {
            continue;
        }
        let (from, to) = (attachments_from.join(&name), attachments_to.join(&name));
        if from.is_file() && !to.exists() {
            std::fs::copy(from, to)?;
        }
        copied.push(name);
    }
    // A knowledge-only backup carries the images, not their rows.
    db.tx(|tx| {
        for name in &copied {
            attachments::record_file(tx, &attachments_to, name)?;
        }
        Ok(())
    })?;
    drop(staged);
    tracing::info!(pages = n, "knowledge loaded from a switch backup");
    Ok(n)
}

// ---------------------------------------------------------------- switching

/// What a switch did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    /// The backup made first (kept in `switch-backups`).
    pub backup: SwitchBackup,
    /// Pages put back from a switch backup (going external: the folder's earlier index; going
    /// internal: the chosen backup's pages).
    pub restored: u32,
    /// Going external: what the scan of the folder changed.
    pub scan: Option<scan::ScanSummary>,
}

/// Going external, steps 1–3 (before the scan): the backup, clearing, the mode, and the folder's
/// earlier index put back. Holds `db` throughout. The caller scans next, then calls
/// [`finish_switch`].
pub fn begin_switch_to_external(db: &mut Db, root: &str) -> Result<(SwitchBackup, u32)> {
    if external::is_external(db.conn())? {
        return Err(CoreError::Invalid("Knowledge is already kept in a folder.".into()));
    }
    let root = check_root(db, root)?;
    // The folder's earlier index is read before anything is cleared; putting it back is not
    // needed for the switch, so a failure only gets logged (and recorded).
    let source = stage_restore_source(db, &root);
    let backup = make_switch_backup(db)?;
    clear_knowledge(db)?;
    db.tx(|tx| settings::set_knowledge_mode(tx, KnowledgeMode::External, Some(&root)))?;
    let restored = enter_folder(db, &root, source)?;
    tracing::info!("knowledge now kept in a folder");
    Ok((backup, restored))
}

/// Step 4 of any switch: the derived indexes are rebuilt, and every page piece waits to be
/// embedded again (FR-KN-EXT-8).
pub fn finish_switch(db: &mut Db) -> Result<()> {
    db.tx(|tx| {
        hooks::rebuild_all(tx)?;
        tx.execute("UPDATE chunks SET vector = NULL WHERE kind = 'page' AND vector IS NOT NULL", [])?;
        Ok(())
    })
}

/// FR-KN-EXT-8, internal → external, all steps (the scan holds `db` too; the app runs the scan in
/// batches instead).
pub fn switch_to_external(db: &mut Db, root: &str) -> Result<SwitchResult> {
    let (backup, restored) = begin_switch_to_external(db, root)?;
    let summary = scan::scan(db, scan::Scope::All)?;
    finish_switch(db)?;
    Ok(SwitchResult {
        backup,
        restored,
        scan: Some(summary),
    })
}

/// Whether two roots name the same folder: the folders themselves when both are there
/// (symbolic links and `..` resolved), else as written.
fn same_folder(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => same_root(a, b),
    }
}

/// The checks of a change of knowledge folder: external mode now, a folder that passes the same
/// checks as switching to a folder ([`check_root`]), and not the folder in use. Returns the new
/// root as it will be recorded.
pub fn check_new_root(db: &Db, root: &str) -> Result<String> {
    let current = match settings::knowledge_mode(db.conn())? {
        (Some(KnowledgeMode::External), Some(current)) => current,
        _ => return Err(CoreError::Invalid("Knowledge is not kept in a folder.".into())),
    };
    let new = check_root(db, root)?;
    if same_folder(&current, &new) {
        return Err(CoreError::Invalid(
            "That is the knowledge folder in use already. Choose another folder.".into(),
        ));
    }
    Ok(new)
}

/// Another knowledge folder, steps 1–3 (before the scan): as switching to internal mode and back
/// to the new folder, in one switch with one backup. The switch backup records the old folder
/// and its pages (made before anything is cleared; a failure changes nothing); the old folder's
/// index is cleared; the new root is recorded and its earlier index put back from its newest
/// switch backup (if any; a failure only gets logged). The folder itself is never changed. The
/// caller scans next, then calls [`finish_switch`].
pub fn begin_switch_external_root(db: &mut Db, root: &str) -> Result<(SwitchBackup, u32)> {
    let root = check_new_root(db, root)?;
    let old_root = settings::knowledge_mode(db.conn())?.1.unwrap_or_default();
    let source = stage_restore_source(db, &root);
    let backup = make_switch_backup(db)?;
    keep_newest_of_folder(db, &old_root, &backup)?;
    clear_knowledge(db)?;
    db.tx(|tx| settings::set_knowledge_mode(tx, KnowledgeMode::External, Some(&root)))?;
    let restored = enter_folder(db, &root, source)?;
    tracing::info!("knowledge now kept in another folder");
    Ok((backup, restored))
}

/// Another knowledge folder, all steps (the scan holds `db` too; the app runs the scan in batches
/// instead).
pub fn switch_external_root(db: &mut Db, root: &str) -> Result<SwitchResult> {
    let (backup, restored) = begin_switch_external_root(db, root)?;
    let summary = scan::scan(db, scan::Scope::All)?;
    finish_switch(db)?;
    Ok(SwitchResult {
        backup,
        restored,
        scan: Some(summary),
    })
}

/// FR-KN-EXT-8, external → internal: the backup, clearing, the mode, then the knowledge part of
/// the switch backup `from` (a file name in `switch-backups`, made in internal mode: one made in
/// external mode is refused before anything happens, [`check_internal_backup`]), or no pages.
pub fn switch_to_internal(db: &mut Db, from: Option<&str>) -> Result<SwitchResult> {
    if !external::is_external(db.conn())? {
        return Err(CoreError::Invalid("Knowledge is already kept in Cairn.".into()));
    }
    let from = match from.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => Some(check_internal_backup(db, f)?),
        None => None,
    };
    let old_root = settings::knowledge_mode(db.conn())?.1.unwrap_or_default();
    let backup = make_switch_backup(db)?;
    clear_knowledge(db)?;
    db.tx(|tx| settings::set_knowledge_mode(tx, KnowledgeMode::Internal, None))?;
    let restored = match from {
        Some(path) => load_knowledge(db, &path)?,
        None => 0,
    };
    // After loading: the backup chosen may be an older one of the folder just left.
    keep_newest_of_folder(db, &old_root, &backup)?;
    finish_switch(db)?;
    tracing::info!("knowledge now kept in Cairn");
    Ok(SwitchResult {
        backup,
        restored,
        scan: None,
    })
}

// ---------------------------------------------------------------- importing (FR-KN-EXT-14)

/// What the import dialog needs to know about a backup before importing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// `None`: made before 0.13.0 (internal).
    pub mode: Option<KnowledgeMode>,
    /// External mode: the root folder the backup's index belongs to.
    pub root: Option<String>,
    /// Whether that folder is on this computer.
    pub root_exists: bool,
    pub exported_at: String,
    pub app_version: String,
    pub schema_version: u32,
    pub pages: u32,
    pub tasks: u32,
}

/// Refused for a knowledge-only switch backup, which is loaded by switching, not imported
/// (`export::SWITCH_BACKUP_REFUSED`).
pub fn backup_info(archive: &Path) -> Result<BackupInfo> {
    let info = archive_info(archive)?;
    if info.knowledge_only {
        return Err(CoreError::Invalid(export::SWITCH_BACKUP_REFUSED.into()));
    }
    Ok(BackupInfo {
        root_exists: info.root.as_deref().is_some_and(|r| Path::new(r).is_dir()),
        mode: info.mode,
        root: info.root,
        exported_at: info.exported_at,
        app_version: info.app_version,
        schema_version: info.schema_version,
        pages: info.pages as u32,
        tasks: info.tasks as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::recorded_root;

    #[test]
    fn a_root_keeps_its_separator_only_at_the_top_of_a_disk() {
        assert_eq!(recorded_root("/home/me/Notes/"), "/home/me/Notes");
        assert_eq!(recorded_root("/home/me/Notes"), "/home/me/Notes");
        assert_eq!(recorded_root("/"), "/");
        assert_eq!(recorded_root("E:\\"), "E:\\");
        assert_eq!(recorded_root("E:\\Notes\\"), "E:\\Notes");
    }
}
