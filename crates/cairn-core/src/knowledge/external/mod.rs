//! External knowledge mode (spec §5.11, D-72): pages are Markdown files in one root folder, and
//! `pages` / `groups` are an **index** of that folder. Every feature keeps reading the same tables;
//! only the writing and the reading of files are new.
//!
//! # The folder
//! - Each folder directly in the root is a **category**, matched to Cairn's categories by name
//!   (`categories.folder` records the match). `.md` files directly in the root belong to *General*
//!   and stay there; new ungrouped General pages go into a `General` folder. A top-level folder
//!   named `attachments` is never a category (it holds images, FR-KN-EXT-3).
//! - A folder inside a category folder is a **group** when Markdown is somewhere below it; groups
//!   hold groups without a depth limit (`groups.parent_id`). A folder with only images is not one.
//! - Pages are `.md` and `.markdown` files; the **title is the file name** without the extension.
//!   Hidden files and folders (a name starting with a dot) and links (symbolic links, junctions)
//!   are skipped. Files that aren't UTF-8 text are left out of the index and reported.
//!
//! # The index's own forms ([`convert`])
//! The database keeps Cairn's forms, so mentions, backlinks, chips and the Assistant work as in
//! internal mode, and moving a file never breaks a link inside the database:
//! - a link to another page is `[text](page:<id>)`;
//! - a link to any other file in the folder (images, PDFs, a page with a `#heading`) is
//!   `folder:<path from the root>`, each part percent-encoded where Markdown needs it;
//! - links that don't resolve, and web links, stay as written; `#KEY` task mentions are text.
//!
//! Reading a file turns its relative links (and Obsidian `[[Page]]` links) into these; writing
//! turns them back into paths relative to the file. Front matter carries `tags`, `source` and
//! `archived` (written only when set); other keys are kept as they are, in their order.
//!
//! # Identity
//! External rows are unique by `path` (root-relative, `/`-separated, exactly as the file system
//! has it). Cairn-only data (check status, the source task, the daily history, task↔page links,
//! group descriptions) lives in the index, matched to the file by its path. `file_hash`,
//! `file_mtime` and `file_size` record the file as Cairn last read or wrote it: a scan reads only
//! files whose size or time changed, and a file that disappears while one with the same hash
//! appears is the same page (renamed outside Cairn).
//!
//! # Reading ([`scan`], [`watch`])
//! A scan walks the folder (all of it, or the paths a watcher batch names) and plans the changes
//! without the database lock; the plan is applied in batches of a few hundred files, one short
//! transaction each, every item checked again against the index when it is applied. The watcher
//! collects file-system events and delivers at most one batch a second, or a request for a full
//! scan when the system reports missed events. Cairn's own writes are recognised by their hash
//! (here and when a batch is applied), so they cause no work.
//!
//! # Writing ([`store`])
//! Every page and group change in external mode goes through the single-item functions of
//! `knowledge` (`create_page_as`, `update_page_in`, …), which write the file first and then the
//! row, so every caller (the save pipeline, merge, the Assistant, the bulk bar, history restore)
//! works unchanged. A file is written to a temporary file and swapped in with a rename (retried
//! for a few seconds while Windows reports it busy); its line endings are kept. A file whose hash
//! differs from the index is read into the index first, never overwritten blindly. Renames and
//! moves move files and folders and replace just the affected links in other files. Deleting
//! moves the file to the system trash (debug builds: `CAIRN_DEV_TRASH_DIR` if set).

pub mod convert;
pub(crate) mod journal;
pub(crate) mod pages;
pub mod scan;
pub mod store;
pub mod watch;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::settings::{self, KnowledgeMode};
use crate::{CoreError, Result};

/// The root folder of an external-mode database.
#[derive(Debug, Clone)]
pub struct Ext {
    pub root: PathBuf,
    /// Cairn's data folder (for images in `attachments` that pages bring along); `None` for a
    /// database without a file.
    pub(crate) data: Option<PathBuf>,
}

impl Ext {
    pub fn new(root: impl Into<PathBuf>) -> Ext {
        Ext {
            root: root.into(),
            data: None,
        }
    }

    /// The absolute path of a root-relative one (`""` is the root itself). `None` when `rel` isn't
    /// a path inside the root ([`valid_rel`]): nothing Cairn reads, writes or deletes can lie
    /// outside the root, whatever a link in a page says.
    pub fn abs(&self, rel: &str) -> Option<PathBuf> {
        if rel.is_empty() {
            return Some(self.root.clone());
        }
        if !valid_rel(rel) {
            return None;
        }
        let mut p = self.root.clone();
        for part in rel.split('/') {
            p.push(part);
        }
        Some(p)
    }

    /// [`Self::abs`], or a plain error.
    pub(crate) fn path(&self, rel: &str) -> Result<PathBuf> {
        self.abs(rel).ok_or_else(|| {
            CoreError::Invalid(format!("\"{rel}\" is not a path inside the knowledge folder."))
        })
    }

    /// [`Self::path`] for a write: no part of it below the root may be a link (a symbolic link or
    /// junction), so a write never lands outside the root.
    pub(crate) fn writable(&self, rel: &str) -> Result<PathBuf> {
        let abs = self.path(rel)?;
        let mut at = self.root.clone();
        for part in rel.split('/').filter(|p| !p.is_empty()) {
            at.push(part);
            match std::fs::symlink_metadata(&at) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(CoreError::Invalid(format!(
                        "\"{rel}\" goes through a link to another place. Cairn only writes inside the knowledge folder."
                    )));
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Ok(abs)
    }

    /// The root-relative form (`/`-separated) of a path inside the root.
    pub fn rel(&self, abs: &Path) -> Option<String> {
        let rest = abs.strip_prefix(&self.root).ok()?;
        let parts: Vec<String> = rest
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let rel = parts.join("/");
        valid_rel(&rel).then_some(rel)
    }

    /// Fails with a plain message when the root folder isn't there (a drive not connected).
    pub(crate) fn check(&self) -> Result<()> {
        if self.root.is_dir() {
            Ok(())
        } else {
            Err(CoreError::Invalid(format!(
                "The knowledge folder \"{}\" can't be found. Check that it is there, or change the knowledge storage in Settings › Data.",
                self.root.display()
            )))
        }
    }
}

/// The external root when the database is in external mode, else `None` (internal, or not chosen).
pub(crate) fn active(conn: &Connection) -> Result<Option<Ext>> {
    match settings::knowledge_mode(conn)? {
        (Some(KnowledgeMode::External), Some(root)) => {
            let data = conn
                .path()
                .filter(|p| !p.is_empty())
                .and_then(|p| Path::new(p).parent().map(Path::to_path_buf));
            Ok(Some(Ext {
                root: PathBuf::from(root),
                data,
            }))
        }
        (Some(KnowledgeMode::External), None) => Err(CoreError::Invalid(
            "No knowledge folder is set. Choose one in Settings › Data.".into(),
        )),
        _ => Ok(None),
    }
}

/// Whether the database is in external mode.
pub fn is_external(conn: &Connection) -> Result<bool> {
    Ok(settings::knowledge_mode(conn)?.0 == Some(KnowledgeMode::External))
}

// ---------------------------------------------------------------- paths

/// One part of a root-relative path: not empty, `.` or `..`, no `\` (a separator on Windows,
/// so a folder synced there could hold none), on Windows no `:` (drives, streams), and one plain
/// name to the system (no drive or root prefix).
fn valid_part(part: &str) -> bool {
    use std::path::Component;
    if part.is_empty() || part == "." || part == ".." || part.contains(['\\', '\0']) {
        return false;
    }
    if cfg!(windows) && part.contains(':') {
        return false;
    }
    let mut c = Path::new(part).components();
    matches!((c.next(), c.next()), (Some(Component::Normal(_)), None))
}

/// Whether `rel` is a root-relative path (`/`-separated) that stays inside the root.
pub(crate) fn valid_rel(rel: &str) -> bool {
    !rel.is_empty() && rel.split('/').all(valid_part)
}

/// Looks at paths below the root exactly as written: each part must be in its folder's listing
/// with exactly that name (a file system that ignores case also opens `pump.md` after a rename
/// to `Pump.md`), and none may be a link. Listings are kept for one pass.
#[derive(Default)]
pub(crate) struct Exact {
    listings: std::cell::RefCell<HashMap<PathBuf, HashMap<std::ffi::OsString, std::fs::FileType>>>,
}

impl Exact {
    /// The type of `name` in `dir`'s listing (read once per pass).
    fn entry(&self, dir: &Path, name: &str) -> Option<std::fs::FileType> {
        let mut listings = self.listings.borrow_mut();
        let listing = listings.entry(dir.to_path_buf()).or_insert_with(|| {
            std::fs::read_dir(dir)
                .map(|d| {
                    d.flatten()
                        .filter_map(|e| Some((e.file_name(), e.file_type().ok()?)))
                        .collect()
                })
                .unwrap_or_default()
        });
        listing.get(std::ffi::OsStr::new(name)).copied()
    }

    /// What is at `rel` (a folder or a file), if it is there exactly so and not through a link.
    pub(crate) fn kind(&self, ext: &Ext, rel: &str) -> Option<std::fs::FileType> {
        if !valid_rel(rel) {
            return None;
        }
        let parts: Vec<&str> = rel.split('/').collect();
        let mut dir = ext.root.clone();
        for (i, part) in parts.iter().enumerate() {
            let kind = self.entry(&dir, part)?;
            if kind.is_symlink() {
                return None;
            }
            if i + 1 == parts.len() {
                return Some(kind);
            }
            if !kind.is_dir() {
                return None;
            }
            dir.push(part);
        }
        None
    }

    pub(crate) fn is_file(&self, ext: &Ext, rel: &str) -> bool {
        self.kind(ext, rel).is_some_and(|k| k.is_file())
    }

    pub(crate) fn is_dir(&self, ext: &Ext, rel: &str) -> bool {
        self.kind(ext, rel).is_some_and(|k| k.is_dir())
    }

    /// The file's metadata when it is there exactly so ([`Self::kind`]).
    pub(crate) fn file_meta(&self, ext: &Ext, rel: &str) -> Option<std::fs::Metadata> {
        if !self.is_file(ext, rel) {
            return None;
        }
        std::fs::symlink_metadata(ext.abs(rel)?).ok()
    }
}

/// Whether `abs` is there with exactly this name (its last part, compared with its folder's
/// listing), for paths outside a pass ([`Exact`]).
pub(crate) fn exists_exact(abs: &Path) -> bool {
    if std::fs::symlink_metadata(abs).is_err() {
        return false;
    }
    let (Some(dir), Some(name)) = (abs.parent(), abs.file_name()) else {
        return true;
    };
    std::fs::read_dir(dir)
        .map(|d| d.flatten().any(|e| e.file_name() == name))
        .unwrap_or(false)
}

/// The folder part of a root-relative path (`""` for the root).
pub(crate) fn dir_of(rel: &str) -> &str {
    rel.rfind('/').map_or("", |i| &rel[..i])
}

/// The last part of a root-relative path.
pub(crate) fn name_of(rel: &str) -> &str {
    rel.rfind('/').map_or(rel, |i| &rel[i + 1..])
}

/// `dir/name`, or `name` in the root.
pub(crate) fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Whether `path` is `dir` itself or inside it.
pub(crate) fn is_within(path: &str, dir: &str) -> bool {
    path == dir || (path.len() > dir.len() && path.starts_with(dir) && path.as_bytes()[dir.len()] == b'/')
}

/// `path` with its leading `from` folder replaced by `to` (when it is inside `from`).
pub(crate) fn rebase(path: &str, from: &str, to: &str) -> Option<String> {
    if path == from {
        Some(to.to_string())
    } else if is_within(path, from) {
        Some(format!("{to}{}", &path[from.len()..]))
    } else {
        None
    }
}

/// The extension of a page file (`md` or `markdown`, any case), if it is one.
pub(crate) fn page_extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && (ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")))
        .then_some(ext)
}

/// A page's title from its file name: the name without `.md` / `.markdown` (FR-KN-EXT-2).
pub(crate) fn title_of(rel: &str) -> String {
    let name = name_of(rel);
    match page_extension(name) {
        Some(ext) => name[..name.len() - ext.len() - 1].to_string(),
        None => name.to_string(),
    }
}

/// Text compared ignoring case, as links name pages: lowercase, with Greek's final sigma (whose
/// lowercase form depends on what follows it, `ΟΣ` alone or in `ΟΣ.md`) as a plain sigma.
pub(crate) fn fold(s: &str) -> String {
    let lower = s.to_lowercase();
    if lower.contains('ς') {
        lower.replace('ς', "σ")
    } else {
        lower
    }
}

/// The key a page file is found by from a `[[…]]` link: its title, ignoring case ([`fold`]).
pub(crate) fn title_key(rel: &str) -> String {
    fold(&title_of(rel))
}

/// Hidden names (`.git`, `.obsidian`, Cairn's own temporary files) are never read.
pub(crate) fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// Every file under the root that isn't a page, by its name ignoring case ([`fold`]), for
/// `![[name]]` embeds, which may name a file anywhere in the folder. Hidden files and folders and
/// links are skipped.
pub(crate) fn files_by_name(ext: &Ext) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut stack = vec![String::new()];
    while let Some(rel) = stack.pop() {
        let Some(Ok(entries)) = ext.abs(&rel).map(std::fs::read_dir) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = join(&rel, &name);
            let Ok(kind) = e.file_type() else { continue };
            if is_hidden(&name) || !valid_rel(&path) {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && page_extension(&name).is_none() {
                out.entry(fold(&name)).or_default().push(path);
            }
        }
    }
    out
}

/// FR-KN-EXT-15: a file or folder name Cairn makes in external mode: Windows' rules on every
/// system (`util::safe_file_name`), and no leading dots or spaces, since a name starting with a
/// dot is hidden and never read (a category named `.git` would take over the repository's
/// folder). `fallback` when nothing is left.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let mut out = crate::util::safe_file_name(name.trim(), fallback);
    loop {
        let stripped = out.trim_start_matches(['.', ' ']);
        if stripped.len() == out.len() {
            return out;
        }
        out = crate::util::safe_file_name(stripped, fallback);
    }
}

/// The title Cairn uses for a page file it makes ([`clean_name`] with `page`).
pub fn clean_title(title: &str) -> String {
    clean_name(title, "page")
}

// ---------------------------------------------------------------- files

/// SHA-256 of a file's bytes, as lowercase hex.
pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A file's size and modification time (nanoseconds since 1970), as the index keeps them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub size: i64,
    pub mtime: i64,
}

pub(crate) fn nanos(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos().min(i64::MAX as u128) as i64,
        Err(e) => -(e.duration().as_nanos().min(i64::MAX as u128) as i64),
    }
}

pub(crate) fn stamp_of(meta: &std::fs::Metadata) -> Stamp {
    Stamp {
        size: meta.len() as i64,
        mtime: meta.modified().map(nanos).unwrap_or(0),
    }
}

/// An ISO timestamp like `now_iso` for a file time.
pub(crate) fn iso_of(t: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(t)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Whether an error is Windows saying the file is in use (another program has it open).
fn busy(e: &std::io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

/// Runs `f`, retrying for a few seconds while the file is busy (FR-KN-EXT-10).
pub(crate) fn retry<T>(mut f: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match f() {
            Err(e) if busy(&e) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            other => return other,
        }
    }
}

/// The hidden temporary file a write of `abs` goes through.
fn temp_for(abs: &Path) -> Result<PathBuf> {
    let dir = abs
        .parent()
        .ok_or_else(|| CoreError::Invalid("A file can't be written at the top of the disk.".into()))?;
    std::fs::create_dir_all(dir)?;
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(dir.join(format!(".{name}{TEMP_SUFFIX}")))
}

/// The end of the names of Cairn's temporary files (hidden: they start with a dot).
pub(crate) const TEMP_SUFFIX: &str = ".cairn-tmp";

fn write_temp(tmp: &Path, bytes: &[u8], permissions: Option<std::fs::Permissions>) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    if let Some(p) = permissions {
        std::fs::set_permissions(tmp, p)?;
    }
    Ok(())
}

/// Writes `bytes` to `abs` through a hidden temporary file in the same folder, swapped in with a
/// rename, so another program never sees half a file. The old file's permissions are kept.
/// Missing folders are made.
pub(crate) fn write_atomic(abs: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = temp_for(abs)?;
    let permissions = std::fs::metadata(abs).ok().map(|m| m.permissions());
    // Noted first: the watcher may see the rename before it returns. A failed write leaves a note
    // that matches nothing.
    note_own_write(abs, Some(hash_bytes(bytes)));
    let written = write_temp(&tmp, bytes, permissions).and_then(|()| retry(|| std::fs::rename(&tmp, abs)));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

fn already_there(abs: &Path) -> CoreError {
    CoreError::Conflict(format!("\"{}\" is already there.", abs.display()))
}

/// Writes a new file at `abs`, refused when something is there already, also when it appeared
/// after the caller looked: the temporary file is linked in under its name, which fails when the
/// name is taken (no check-then-rename race). Where links can't be made (some network and USB
/// drives) the file is made with `create_new`, which fails the same way.
pub(crate) fn write_new(abs: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = temp_for(abs)?;
    note_own_write(abs, Some(hash_bytes(bytes)));
    if let Err(e) = write_temp(&tmp, bytes, None) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    let linked = retry(|| std::fs::hard_link(&tmp, abs));
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(already_there(abs)),
        Err(_) => {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(abs)
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        already_there(abs)
                    } else {
                        e.into()
                    }
                })?;
            f.write_all(bytes)?;
            f.sync_all()?;
            Ok(())
        }
    }
}

/// Moves a file or folder, retrying while it is busy. Refuses to replace something.
pub(crate) fn move_path(from: &Path, to: &Path) -> Result<()> {
    if to.exists() && !same_file(from, to) {
        return Err(already_there(to));
    }
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    note_own_write(from, None);
    note_moved(from, to);
    retry(|| std::fs::rename(from, to))?;
    Ok(())
}

/// Whether two paths are the same file: on a file system that ignores case, `a.md` → `A.md`
/// finds the target "there" already.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

/// Notes what will be at `to` once `from` is moved there (before the move, for the watcher).
fn note_moved(from: &Path, to: &Path) {
    if from.is_dir() {
        note_own_write(to, Some(DIR_MARK.to_string()));
    } else if let Ok(bytes) = std::fs::read(from) {
        note_own_write(to, Some(hash_bytes(&bytes)));
    }
}

/// Makes a folder (and its parents) and notes it as Cairn's own change. Something else of that
/// name (a file, a link) is refused.
pub(crate) fn make_dir(abs: &Path) -> Result<()> {
    match std::fs::symlink_metadata(abs) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(CoreError::Conflict(format!(
            "\"{}\" is there already and is not a folder.",
            abs.display()
        ))),
        Err(_) => {
            note_own_write(abs, Some(DIR_MARK.to_string()));
            std::fs::create_dir_all(abs)?;
            Ok(())
        }
    }
}

/// Removes a folder if nothing is in it any more (hidden files count as something).
pub(crate) fn remove_dir_if_empty(abs: &Path) -> Result<bool> {
    let empty = std::fs::read_dir(abs).map(|mut d| d.next().is_none()).unwrap_or(false);
    if empty {
        note_own_write(abs, None);
        retry(|| std::fs::remove_dir(abs))?;
    }
    Ok(empty)
}

// ---------------------------------------------------------------- the trash

#[cfg(test)]
thread_local! {
    static TEST_TRASH: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Tests move deleted files into `dir` (never the system trash). Per test thread.
#[cfg(test)]
pub(crate) fn set_test_trash(dir: &Path) {
    TEST_TRASH.with(|t| *t.borrow_mut() = Some(dir.to_path_buf()));
}

/// Where deleted files go instead of the system trash: tests always have one; debug builds when
/// `CAIRN_DEV_TRASH_DIR` is set, so probes never fill the owner's trash.
fn stand_in_trash() -> Result<Option<PathBuf>> {
    #[cfg(test)]
    {
        TEST_TRASH
            .with(|t| t.borrow().clone())
            .map(Some)
            .ok_or_else(|| CoreError::Invalid("tests must call set_test_trash before deleting".into()))
    }
    #[cfg(not(test))]
    {
        if cfg!(debug_assertions)
            && let Some(dir) = std::env::var_os("CAIRN_DEV_TRASH_DIR")
        {
            return Ok(Some(PathBuf::from(dir)));
        }
        Ok(None)
    }
}

/// Moves a file (or folder) to the system trash (FR-KN-EXT-10).
pub(crate) fn trash(abs: &Path) -> Result<()> {
    note_own_write(abs, None);
    match stand_in_trash()? {
        Some(dir) => {
            std::fs::create_dir_all(&dir)?;
            let name = abs
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into());
            let target = dir.join(format!("{}-{name}", nanos(SystemTime::now())));
            if retry(|| std::fs::rename(abs, &target)).is_err() {
                // Another file system: copy, then remove.
                std::fs::copy(abs, &target)?;
                std::fs::remove_file(abs)?;
            }
        }
        None => trash::delete(abs).map_err(|e| {
            CoreError::Io(std::io::Error::other(format!(
                "\"{}\" could not be moved to the trash: {e}",
                abs.display()
            )))
        })?,
    }
    Ok(())
}

// ---------------------------------------------------------------- Cairn's own writes

/// Marks a folder in the registry of Cairn's own writes.
const DIR_MARK: &str = "<dir>";
/// How long a write of Cairn's is remembered (events arrive within a second or two).
const OWN_WRITE_TTL: Duration = Duration::from_secs(30);

/// Per path: the hash Cairn wrote ([`DIR_MARK`] for a folder it made, `None` when it removed the
/// path) and when.
type OwnWrites = HashMap<PathBuf, (Option<String>, Instant)>;

/// What Cairn last did to each path it changed.
static OWN_WRITES: LazyLock<Mutex<OwnWrites>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Records a change Cairn made, so the watcher can recognise its event as an echo, and in the
/// transaction's journal, so a rollback can take it back ([`journal`]).
pub(crate) fn note_own_write(abs: &Path, hash: Option<String>) {
    {
        let mut map = OWN_WRITES.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, (_, at)| now.duration_since(*at) < OWN_WRITE_TTL);
        map.insert(abs.to_path_buf(), (hash, now));
    }
    journal::touched(abs);
}

/// Forgets Cairn's note on `abs` (its change was rolled back), so the watcher sees it again.
pub(crate) fn forget_own_write(abs: &Path) {
    OWN_WRITES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(abs);
}

/// Whether the path is exactly as Cairn's own last change left it (an echo of that change). Names
/// are compared exactly: where case is ignored, `pump.md` still "exists" after Cairn renamed it
/// to `Pump.md`, and must not count as there.
pub(crate) fn is_own_write(abs: &Path) -> bool {
    let expected = {
        let map = OWN_WRITES.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(abs) {
            Some((h, at)) if at.elapsed() < OWN_WRITE_TTL => h.clone(),
            _ => return false,
        }
    };
    match expected.as_deref() {
        None => !exists_exact(abs),
        Some(DIR_MARK) => abs.is_dir() && exists_exact(abs),
        Some(h) => exists_exact(abs) && std::fs::read(abs).is_ok_and(|b| hash_bytes(&b) == h),
    }
}

#[cfg(test)]
mod regression_tests;
