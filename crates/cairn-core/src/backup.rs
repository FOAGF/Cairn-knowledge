//! Automatic and manual backups (NFR-REL-2, UI-SET-4, D-45, D-52).
//!
//! A backup is a full export (FR-DATA-1, [`crate::export::export_to`]) written to the backup
//! folder: the setting `backupFolder`, or the data folder's `backups` folder while it is empty.
//!
//! - **One daily backup per local calendar day**, made on start and while Cairn is open. "Today"
//!   is the local date in the computer's current time zone; the time of the last daily backup is
//!   stored in UTC and compared as a local date.
//! - **File names** carry the computer's name and the local date and time (D-58):
//!   `cairn-daily-LAPTOP-2026-10-16-0912.zip`, `cairn-manual-LAPTOP-2026-10-16-1530.zip`. A name
//!   that is taken gets `-2`, `-3`, …; nothing is overwritten. The computer part is read from the
//!   end of the name, so hyphens in it never clash with the date ([`parse_name`]). Names without a
//!   computer (before M8) still list, as "an earlier backup".
//! - **Retention:** the newest N daily backups *of this computer* are kept (`backupKeep`). Another
//!   computer's files and old-form files without a computer name are never deleted (a backup
//!   folder may be shared, e.g. in OneDrive). Manual ones are kept until the user deletes them. The safety copies in the data folder's `backups` folder
//!   (`pre-import-*.zip`, `pre-migration-v*-*.sqlite`) also keep the newest N of each kind. The
//!   order comes from the export time inside each file (the manifest's `exportedAt`, UTC), not
//!   from names or file dates; only `pre-migration-*` files have no manifest (they are plain
//!   SQLite copies), so they are ordered by the UTC stamp in their name
//!   (`pre-migration-v{N}-{YYYYMMDD}-{HHMMSS}.sqlite`). Cairn deletes only its own backup files
//!   and never a file it can't read or whose name it can't parse. Retention runs after each
//!   daily backup.
//! - **Database check (D-52):** before each daily backup SQLite's `PRAGMA quick_check` runs. If
//!   it fails, the backup is still written (it may help), the status says so, and no old backup
//!   is deleted until a check passes.
//! - **Status** (last daily time, last failure, check result) is local to this data folder and is
//!   not exported: it lives in `backup-state.json` in the data folder, not in the database, so
//!   an import never brings in another computer's status and never loses this one.
//! - **The database lock is not held during an export.** [`prepare`] reads what it needs from
//!   the open database and returns a [`BackupJob`]; [`BackupJob::run`] works from its own
//!   read-only connection (one read transaction, a consistent WAL snapshot), so the UI keeps
//!   working while a large export runs. [`make_backup`] does both steps for callers that hold a
//!   [`Db`] anyway.

use std::fmt::Display;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CoreError, DataPaths, Db, Result, export, settings};

/// Why a backup was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BackupKind {
    /// The automatic once-a-day backup; the newest N are kept.
    Daily,
    /// *Back up now*; kept until the user deletes it.
    Manual,
}

impl BackupKind {
    fn word(self) -> &'static str {
        match self {
            BackupKind::Daily => "daily",
            BackupKind::Manual => "manual",
        }
    }
}

/// One backup file in the backup folder (UI-SET-4's list).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub path: String,
    pub file_name: String,
    pub kind: BackupKind,
    /// From the manifest inside the file (UTC, RFC 3339).
    pub exported_at: String,
    /// Bytes.
    pub size: u64,
    /// The Cairn version that made it.
    pub app_version: String,
    /// The computer that made it, from the file name (D-58); `None` for an older backup whose
    /// name has no computer.
    pub computer: Option<String>,
}

/// What *Settings › Backup & data* shows on the *Last backup* line.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    /// The backup folder in use (the setting, or the data folder's `backups`).
    pub folder: String,
    /// This computer's name as used in backup file names (D-58).
    pub computer: String,
    /// Whether `folder` is the default one inside the data folder.
    pub is_default_folder: bool,
    /// The last successful backup of either kind made by this computer, if any.
    pub last: Option<BackupFile>,
    /// The last failure, if it came after the last success: when (UTC) and why, in plain words.
    pub error: Option<String>,
    pub error_at: Option<String>,
    /// The last database check failed (D-52): old backups are not deleted until one passes.
    pub check_failed: bool,
}

/// After a failed backup, the next automatic try waits this long (NFR-REL-2).
const RETRY_AFTER: chrono::Duration = chrono::Duration::hours(1);

// ---------------------------------------------------------------------------------------------
// Local status

const STATE_FILE: &str = "backup-state.json";

/// The status that is local to this data folder (not exported).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct State {
    /// When the last daily backup was made (UTC, RFC 3339).
    last_daily_at: Option<String>,
    /// The last failure and when it happened (UTC); cleared by the next success.
    error: Option<String>,
    error_at: Option<String>,
    /// The last daily database check failed (D-52).
    check_failed: bool,
}

fn state_path(paths: &DataPaths) -> PathBuf {
    paths.root.join(STATE_FILE)
}

/// A missing or damaged file means "no status yet": a daily backup is simply due.
fn load_state(paths: &DataPaths) -> State {
    std::fs::read_to_string(state_path(paths))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(paths: &DataPaths, state: &State) -> Result<()> {
    let file = state_path(paths);
    let part = file.with_extension("json.part");
    std::fs::write(&part, serde_json::to_vec_pretty(state)?)?;
    std::fs::rename(part, file)?;
    Ok(())
}

fn parse_utc(text: &Option<String>) -> Option<DateTime<Utc>> {
    text.as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc))
}

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

// ---------------------------------------------------------------------------------------------
// Folder and status

/// The folder backups go to: the `backupFolder` setting, or `<data>/backups` while it is empty.
pub fn backup_folder(db: &Db) -> Result<PathBuf> {
    let folder = settings::get(db.conn())?.backup_folder;
    Ok(folder_from_setting(db.paths(), &folder))
}

fn folder_from_setting(paths: &DataPaths, setting: &str) -> PathBuf {
    let setting = setting.trim();
    if setting.is_empty() {
        paths.backups_dir()
    } else {
        PathBuf::from(setting)
    }
}

/// The status line for Settings.
pub fn status(db: &Db) -> Result<BackupStatus> {
    status_as(db, computer_name())
}

fn status_as(db: &Db, computer: String) -> Result<BackupStatus> {
    let folder = backup_folder(db)?;
    let state = load_state(db.paths());
    Ok(BackupStatus {
        is_default_folder: folder == db.paths().backups_dir(),
        last: list_folder(&folder)
            .into_iter()
            .find(|f| f.computer.as_deref() == Some(computer.as_str())),
        computer,
        error: state.error,
        error_at: state.error_at,
        check_failed: state.check_failed,
        folder: folder.to_string_lossy().into_owned(),
    })
}

/// The backup folder was changed: the next daily backup goes to the new folder soon instead of
/// waiting for tomorrow.
pub fn folder_changed(db: &Db) -> Result<()> {
    let mut state = load_state(db.paths());
    state.last_daily_at = None;
    state.error = None;
    state.error_at = None;
    save_state(db.paths(), &state)
}

/// Checks that Cairn can create and remove a file in `folder` (*Choose folder…*). The folder is
/// created if it doesn't exist.
pub fn check_folder(folder: &Path) -> Result<()> {
    if !folder.is_absolute() {
        return Err(CoreError::Invalid(
            "The folder must be a full path, for example /home/you/OneDrive/cairn.".into(),
        ));
    }
    std::fs::create_dir_all(folder)
        .map_err(|e| CoreError::Invalid(format!("That folder cannot be created: {e}")))?;
    let probe = folder.join(".cairn-write-test");
    std::fs::write(&probe, b"")
        .map_err(|e| CoreError::Invalid(format!("Cairn cannot write to that folder: {e}")))?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// When

/// Whether a daily backup is due at `now`: backups are on and no daily backup was made on the
/// current local date; after a failure, only once an hour has passed since it.
pub fn is_due(db: &Db, now: DateTime<Utc>) -> Result<bool> {
    is_due_in(db, now, &Local)
}

/// [`is_due`] at the current time.
pub fn is_due_now(db: &Db) -> Result<bool> {
    is_due(db, Utc::now())
}

/// [`is_due`] in a given time zone (tests use fixed offsets; the app uses the computer's zone).
fn is_due_in<Tz: TimeZone>(db: &Db, now: DateTime<Utc>, tz: &Tz) -> Result<bool> {
    let enabled = settings::get(db.conn())?.backup_enabled;
    let state = load_state(db.paths());
    Ok(due(
        enabled,
        parse_utc(&state.last_daily_at),
        parse_utc(&state.error_at),
        now,
        tz,
    ))
}

/// The rule behind [`is_due`] (NFR-REL-2): the last daily backup's UTC time is turned into a local
/// date in `tz` (the zone *now*) and compared with today's. A zone change or daylight saving
/// can therefore cause one extra backup, never a gap of much more than a day.
fn due<Tz: TimeZone>(
    enabled: bool,
    last_daily: Option<DateTime<Utc>>,
    error_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    tz: &Tz,
) -> bool {
    if !enabled {
        return false;
    }
    if let Some(failed) = error_at {
        let since = now - failed;
        if since >= chrono::Duration::zero() && since < RETRY_AFTER {
            return false;
        }
    }
    match last_daily {
        Some(last) => last.with_timezone(tz).date_naive() != now.with_timezone(tz).date_naive(),
        None => true,
    }
}

// ---------------------------------------------------------------------------------------------
// Making a backup

/// The database check before a daily backup; `true` means it passed.
type Checker<'a> = &'a dyn Fn(&Connection) -> Result<bool>;

/// SQLite's quick integrity check (D-52).
fn quick_check(conn: &Connection) -> Result<bool> {
    let rows = conn
        .prepare("PRAGMA quick_check")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows.len() == 1 && rows[0] == "ok")
}

/// A backup ready to run: everything it needs from the open database, so [`BackupJob::run`]
/// doesn't need the database lock.
#[derive(Debug, Clone)]
pub struct BackupJob {
    paths: DataPaths,
    folder: PathBuf,
    keep: u32,
    kind: BackupKind,
    /// This computer's name for file names and retention (D-58).
    computer: String,
}

/// Reads the settings for a backup of `kind` (quick; hold the database lock only for this).
pub fn prepare(db: &Db, kind: BackupKind) -> Result<BackupJob> {
    let s = settings::get(db.conn())?;
    Ok(BackupJob {
        paths: db.paths().clone(),
        folder: folder_from_setting(db.paths(), &s.backup_folder),
        keep: s.backup_keep.max(1),
        kind,
        computer: computer_name(),
    })
}

/// Makes a backup of `kind` now: the database check first (daily only), then the export, then
/// retention (daily only). Records the outcome in the status, also on failure (the error is
/// returned too). Holds the caller's `Db` for the whole export; the app uses [`prepare`] and
/// [`BackupJob::run`] instead.
pub fn make_backup(db: &mut Db, kind: BackupKind, now: DateTime<Utc>) -> Result<BackupFile> {
    prepare(db, kind)?.run(now)
}

impl BackupJob {
    /// [`BackupJob::run`] at the current time.
    pub fn run_now(&self) -> Result<BackupFile> {
        self.run(Utc::now())
    }

    /// Runs the backup from its own read-only connection.
    pub fn run(&self, now: DateTime<Utc>) -> Result<BackupFile> {
        self.run_in(now, &Local, &quick_check)
    }

    fn run_in<Tz: TimeZone>(
        &self,
        now: DateTime<Utc>,
        tz: &Tz,
        check: Checker,
    ) -> Result<BackupFile>
    where
        Tz::Offset: Display,
    {
        let result = self.try_run(now, tz, check);
        if let Err(e) = &result {
            tracing::warn!(error = %e, kind = self.kind.word(), "backup failed");
            let mut state = load_state(&self.paths);
            state.error = Some(failure_message(e));
            state.error_at = Some(iso(now));
            if let Err(e) = save_state(&self.paths, &state) {
                tracing::warn!(error = %e, "could not save the backup status");
            }
        }
        result
    }

    fn try_run<Tz: TimeZone>(
        &self,
        now: DateTime<Utc>,
        tz: &Tz,
        check: Checker,
    ) -> Result<BackupFile>
    where
        Tz::Offset: Display,
    {
        let conn = Connection::open_with_flags(
            self.paths.db_file(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        // D-52: a failed check doesn't stop the backup (it may help), but old backups stay.
        let mut check_passed = true;
        if self.kind == BackupKind::Daily {
            check_passed = check(&conn).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "the database check could not run");
                false
            });
            let mut state = load_state(&self.paths);
            if state.check_failed == check_passed {
                state.check_failed = !check_passed;
                save_state(&self.paths, &state)?;
            }
            if !check_passed {
                tracing::error!("the database check failed; no old backup will be deleted");
            }
        }

        std::fs::create_dir_all(&self.folder)?;
        let dest = free_name(&self.folder, self.kind, &self.computer, now, tz);
        export::export_from_conn(&conn, &self.paths.attachments_dir(), &dest, &iso(now))?;
        let file = describe(&dest)?;

        let mut state = load_state(&self.paths);
        state.error = None;
        state.error_at = None;
        if self.kind == BackupKind::Daily {
            state.last_daily_at = Some(file.exported_at.clone());
        }
        save_state(&self.paths, &state)?;
        tracing::info!(kind = self.kind.word(), size = file.size, "backup made");

        if self.kind == BackupKind::Daily && check_passed {
            self.prune();
        }
        Ok(file)
    }

    /// Retention (NFR-REL-2): the newest N daily backups, and the newest N of each kind of
    /// safety copy. A failure to delete only gets logged.
    fn prune(&self) {
        let keep = self.keep as usize;
        let mut dailies: Vec<_> = list_folder(&self.folder)
            .into_iter()
            // D-58: only this computer's own daily backups; never another computer's or an
            // old-form file without a computer name.
            .filter(|f| f.kind == BackupKind::Daily && same_computer(&f.computer, &self.computer))
            .map(|f| (f.exported_at, PathBuf::from(f.path)))
            .collect();
        delete_beyond(&mut dailies, keep);

        let mut pre_import = Vec::new();
        let mut pre_migration = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.paths.backups_dir()) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("pre-import-") && name.ends_with(".zip") {
                    if let Ok((at, _)) = export::peek_manifest(&path) {
                        pre_import.push((at, path));
                    }
                } else if let Some(stamp) = pre_migration_stamp(&name) {
                    pre_migration.push((stamp, path));
                }
            }
        }
        delete_beyond(&mut pre_import, keep);
        delete_beyond(&mut pre_migration, keep);
    }
}

/// A short reason for the *Last backup* line.
fn failure_message(e: &CoreError) -> String {
    match e {
        CoreError::Io(e) => format!("The backup folder could not be written to ({e})."),
        CoreError::Invalid(m) => m.clone(),
        other => other.to_string(),
    }
}

/// `cairn-daily-LAPTOP-2026-10-16-0912.zip` in the zone `tz`, or with `-2`, `-3`, … while taken.
fn free_name<Tz: TimeZone>(
    folder: &Path,
    kind: BackupKind,
    computer: &str,
    now: DateTime<Utc>,
    tz: &Tz,
) -> PathBuf
where
    Tz::Offset: Display,
{
    let stem = format!(
        "cairn-{}-{}-{}",
        kind.word(),
        computer,
        now.with_timezone(tz).format("%Y-%m-%d-%H%M")
    );
    let mut path = folder.join(format!("{stem}.zip"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{stem}-{n}.zip"));
        n += 1;
    }
    path
}

// ---------------------------------------------------------------------------------------------
// The computer's name (D-58)

/// The longest computer name used in a file name.
const COMPUTER_MAX: usize = 24;

/// Keeps letters, digits and hyphens of a host name, at most [`COMPUTER_MAX`] characters, without
/// hyphens at the ends; `computer` if nothing is left. Letters of any script count, so a Chinese
/// computer name stays readable (and is a valid file name on Windows and Linux).
pub fn clean_computer_name(raw: &str) -> String {
    let kept: String = raw
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .take(COMPUTER_MAX)
        .collect();
    let kept = kept.trim_matches('-');
    if kept.is_empty() {
        "computer".into()
    } else {
        kept.into()
    }
}

/// This computer's name for backup file names: `COMPUTERNAME` on Windows; elsewhere the host name
/// from `/proc/sys/kernel/hostname` or `/etc/hostname`.
pub fn computer_name() -> String {
    let raw = if cfg!(windows) {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    } else {
        ["/proc/sys/kernel/hostname", "/etc/hostname"]
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .map(|t| t.trim().to_string())
            .find(|t| !t.is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_default()
    };
    clean_computer_name(&raw)
}

/// Whether a file's computer is this one (the same name in any letter case).
fn same_computer(file: &Option<String>, this: &str) -> bool {
    file.as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case(this))
}

/// What a backup file name says: the kind and the computer (`None` for an old-form name).
/// The date part `YYYY-MM-DD-HHMM` with an optional `-N` is matched from the end, so a computer
/// name with hyphens or digits is never confused with it. `None` if the name is not Cairn's.
pub fn parse_name(name: &str) -> Option<(BackupKind, Option<String>)> {
    let (kind, rest) = if let Some(r) = name.strip_prefix("cairn-daily-") {
        (BackupKind::Daily, r)
    } else {
        (BackupKind::Manual, name.strip_prefix("cairn-manual-")?)
    };
    let rest = rest.strip_suffix(".zip")?;
    let parts: Vec<&str> = rest.split('-').collect();
    let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
    // The number of parts the date takes: without and with the `-N` suffix.
    for tail in [4, 5] {
        if parts.len() < tail {
            continue;
        }
        let (head, date) = parts.split_at(parts.len() - tail);
        let (date, n_ok) = if tail == 5 {
            (
                &date[..4],
                !date[4].is_empty() && date[4].bytes().all(|b| b.is_ascii_digit()),
            )
        } else {
            (date, true)
        };
        if n_ok
            && digits(date[0], 4)
            && digits(date[1], 2)
            && digits(date[2], 2)
            && digits(date[3], 4)
        {
            let computer = head.join("-");
            return Some((kind, (!computer.is_empty()).then_some(computer)));
        }
    }
    None
}

/// Deletes all but the newest `keep` of `files` (sort key: UTC time as RFC 3339 or a stamp that
/// sorts the same way; an unparseable time never gets here).
fn delete_beyond(files: &mut Vec<(String, PathBuf)>, keep: usize) {
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    for (_, path) in files.iter().skip(keep) {
        match std::fs::remove_file(path) {
            Ok(()) => tracing::info!("deleted an old backup"),
            Err(e) => tracing::warn!(error = %e, "could not delete an old backup"),
        }
    }
}

/// The UTC time in `pre-migration-v3-20261016-091200.sqlite`, as an RFC 3339 string, or `None`
/// if the name isn't of that form.
fn pre_migration_stamp(name: &str) -> Option<String> {
    let rest = name
        .strip_prefix("pre-migration-v")?
        .strip_suffix(".sqlite")?;
    let (version, stamp) = rest.split_once('-')?;
    version.parse::<u32>().ok()?;
    let at = NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%S").ok()?;
    Some(iso(at.and_utc()))
}

// ---------------------------------------------------------------------------------------------
// Listing

/// The facts about the backup at `path`; fails if it isn't a readable Cairn archive.
fn describe(path: &Path) -> Result<BackupFile> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A name that isn't Cairn's form (e.g. `cairn-daily-old.zip`) keeps the old reading: daily by
    // prefix, no computer.
    let (kind, computer) = parse_name(&file_name).unwrap_or_else(|| {
        let kind = if file_name.starts_with("cairn-daily-") {
            BackupKind::Daily
        } else {
            BackupKind::Manual
        };
        (kind, None)
    });
    let (exported_at, app_version) = export::peek_manifest(path)?;
    // Only a time that can be ordered counts as readable.
    DateTime::parse_from_rfc3339(&exported_at)
        .map_err(|_| CoreError::Invalid("The backup's export time cannot be read.".into()))?;
    Ok(BackupFile {
        path: path.to_string_lossy().into_owned(),
        file_name,
        kind,
        exported_at,
        size: std::fs::metadata(path)?.len(),
        app_version,
        computer,
    })
}

/// Cairn's backups in `folder`, newest first; files that aren't readable are left out.
fn list_folder(folder: &Path) -> Vec<BackupFile> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<(DateTime<Utc>, BackupFile)> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.starts_with("cairn-daily-") || name.starts_with("cairn-manual-"))
                && name.ends_with(".zip")
                && e.file_type().is_ok_and(|t| t.is_file())
        })
        .filter_map(|e| describe(&e.path()).ok())
        .filter_map(|f| {
            let at = DateTime::parse_from_rfc3339(&f.exported_at).ok()?;
            Some((at.with_timezone(&Utc), f))
        })
        .collect();
    files.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.file_name.cmp(&a.1.file_name))
    });
    files.into_iter().map(|(_, f)| f).collect()
}

/// Cairn's backups in the backup folder, newest first. Files Cairn can't read are left out.
pub fn list(db: &Db) -> Result<Vec<BackupFile>> {
    Ok(list_folder(&backup_folder(db)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn open() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        (dir, db)
    }

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn zone(hours: i32) -> FixedOffset {
        FixedOffset::east_opt(hours * 3600).unwrap()
    }

    fn ok(_: &Connection) -> Result<bool> {
        Ok(true)
    }

    fn broken(_: &Connection) -> Result<bool> {
        Ok(false)
    }

    /// Runs a backup as the computer `TESTPC`.
    fn run(db: &Db, kind: BackupKind, now: &str, tz: &FixedOffset, check: Checker) -> BackupFile {
        run_as("TESTPC", db, kind, now, tz, check)
    }

    fn run_as(
        computer: &str,
        db: &Db,
        kind: BackupKind,
        now: &str,
        tz: &FixedOffset,
        check: Checker,
    ) -> BackupFile {
        let mut job = prepare(db, kind).unwrap();
        job.computer = computer.into();
        job.run_in(utc(now), tz, check).unwrap()
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut n: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        n.sort();
        n
    }

    fn keep(db: &mut Db, n: u32) {
        settings::update(
            db,
            settings::SettingsPatch {
                backup_keep: Some(n),
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn backup_is_a_readable_export_with_a_local_name_and_a_status() {
        let (_d, db) = open();
        let tz = zone(10);
        let f = run(&db, BackupKind::Daily, "2026-10-16T01:12:30Z", &tz, &ok);
        // 01:12 UTC is 11:12 at +10.
        assert_eq!(f.file_name, "cairn-daily-TESTPC-2026-10-16-1112.zip");
        assert_eq!(f.kind, BackupKind::Daily);
        assert_eq!(f.exported_at, "2026-10-16T01:12:30.000Z");
        assert!(f.size > 0);
        assert_eq!(
            Path::new(&f.path).parent().unwrap(),
            db.paths().backups_dir()
        );
        // The file is a normal export: it imports.
        let (_d2, mut other) = open();
        crate::export::import_from(&mut other, Path::new(&f.path)).unwrap();

        let st = status_as(&db, "TESTPC".into()).unwrap();
        assert!(st.is_default_folder && !st.check_failed && st.error.is_none());
        assert_eq!(st.computer, "TESTPC");
        assert_eq!(f.computer.as_deref(), Some("TESTPC"));
        assert_eq!(st.last.unwrap().file_name, f.file_name);
        assert_eq!(list(&db).unwrap().len(), 1);
        // No half-written file is left.
        assert!(
            names(&db.paths().backups_dir())
                .iter()
                .all(|n| !n.ends_with(".part"))
        );
    }

    #[test]
    fn a_taken_name_gets_a_number_and_nothing_is_overwritten() {
        let (_d, db) = open();
        let tz = zone(0);
        let a = run(&db, BackupKind::Manual, "2026-10-16T15:30:05Z", &tz, &ok);
        let b = run(&db, BackupKind::Manual, "2026-10-16T15:30:50Z", &tz, &ok);
        let c = run(&db, BackupKind::Manual, "2026-10-16T15:30:59Z", &tz, &ok);
        assert_eq!(a.file_name, "cairn-manual-TESTPC-2026-10-16-1530.zip");
        assert_eq!(b.file_name, "cairn-manual-TESTPC-2026-10-16-1530-2.zip");
        assert_eq!(c.file_name, "cairn-manual-TESTPC-2026-10-16-1530-3.zip");
        assert_eq!(list(&db).unwrap().len(), 3);
    }

    #[test]
    fn export_does_not_hold_the_app_connection() {
        // The job works from its own read-only connection: the app's write goes through while
        // the job exists, and the backup sees the data from when it read.
        let (_d, mut db) = open();
        let job = prepare(&db, BackupKind::Manual).unwrap();
        db.tx(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES ('userName', '\"Kim\"')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let f = job
            .run_in(utc("2026-10-16T00:00:00Z"), &zone(0), &ok)
            .unwrap();
        let (_d2, mut other) = open();
        crate::export::import_from(&mut other, Path::new(&f.path)).unwrap();
        assert_eq!(settings::get(other.conn()).unwrap().user_name, "Kim");
    }

    // ----- when

    #[test]
    fn due_once_per_local_day_and_only_when_on() {
        let tz = zone(2);
        let last = Some(utc("2026-10-16T09:00:00Z")); // 11:00 local on the 16th
        // Same local day, even late in the evening (21:59 UTC is 23:59 local).
        assert!(!due(true, last, None, utc("2026-10-16T21:59:00Z"), &tz));
        // Local midnight (22:00 UTC) starts a new day.
        assert!(due(true, last, None, utc("2026-10-16T22:00:00Z"), &tz));
        assert!(due(true, None, None, utc("2026-10-16T22:00:00Z"), &tz));
        assert!(!due(false, None, None, utc("2026-10-16T22:00:00Z"), &tz));
    }

    #[test]
    fn a_failure_is_retried_after_an_hour() {
        let tz = zone(0);
        let failed = Some(utc("2026-10-16T08:00:00Z"));
        assert!(!due(true, None, failed, utc("2026-10-16T08:59:00Z"), &tz));
        assert!(due(true, None, failed, utc("2026-10-16T09:00:00Z"), &tz));
        // A clock that went back does not block backups for long.
        assert!(due(true, None, failed, utc("2026-10-16T07:00:00Z"), &tz));
    }

    #[test]
    fn is_due_reads_the_settings_and_the_status() {
        let (_d, mut db) = open();
        let tz = zone(0);
        let now = "2026-10-16T09:12:00Z";
        assert!(is_due_in(&db, utc(now), &tz).unwrap());
        run(&db, BackupKind::Daily, now, &tz, &ok);
        assert!(!is_due_in(&db, utc("2026-10-16T23:59:00Z"), &tz).unwrap());
        assert!(is_due_in(&db, utc("2026-10-17T00:00:00Z"), &tz).unwrap());
        // A manual backup doesn't count as the daily one.
        let (_d2, db2) = open();
        run(&db2, BackupKind::Manual, now, &tz, &ok);
        assert!(is_due_in(&db2, utc(now), &tz).unwrap());
        // Off.
        settings::update(
            &mut db,
            settings::SettingsPatch {
                backup_enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!is_due_in(&db, utc("2026-10-17T00:00:00Z"), &tz).unwrap());
    }

    #[test]
    fn a_failed_backup_is_reported_and_retried_after_an_hour() {
        let (_d, mut db) = open();
        // A file where the folder should be.
        let blocked = db.paths().root.join("blocked");
        std::fs::write(&blocked, b"x").unwrap();
        settings::update(
            &mut db,
            settings::SettingsPatch {
                backup_folder: Some(blocked.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let tz = zone(0);
        let err = prepare(&db, BackupKind::Daily)
            .unwrap()
            .run_in(utc("2026-10-16T08:00:00Z"), &tz, &ok)
            .unwrap_err();
        assert_eq!(err.kind(), "io");
        let st = status(&db).unwrap();
        assert!(st.error.is_some() && st.last.is_none());
        assert_eq!(st.error_at.as_deref(), Some("2026-10-16T08:00:00.000Z"));
        assert!(!is_due_in(&db, utc("2026-10-16T08:30:00Z"), &tz).unwrap());
        assert!(is_due_in(&db, utc("2026-10-16T09:00:00Z"), &tz).unwrap());
        // Folder changed: the error goes and a backup is due at once.
        folder_changed(&db).unwrap();
        assert!(status(&db).unwrap().error.is_none());
        assert!(is_due_in(&db, utc("2026-10-16T08:30:00Z"), &tz).unwrap());
    }

    /// Replays days with zone changes: `schedule(t)` gives the computer's zone at `t`. Returns
    /// the times of the daily backups.
    fn simulate(
        start: &str,
        hours: i64,
        schedule: impl Fn(DateTime<Utc>) -> FixedOffset,
    ) -> Vec<DateTime<Utc>> {
        let mut last = None;
        let mut made = Vec::new();
        let mut t = utc(start);
        for _ in 0..hours {
            if due(true, last, None, t, &schedule(t)) {
                made.push(t);
                last = Some(t);
            }
            t += chrono::Duration::hours(1);
        }
        made
    }

    /// How many different local dates the replayed span touches.
    fn days_covered(
        start: &str,
        hours: i64,
        schedule: impl Fn(DateTime<Utc>) -> FixedOffset,
    ) -> usize {
        let mut days = std::collections::BTreeSet::new();
        let mut t = utc(start);
        for _ in 0..hours {
            days.insert(t.with_timezone(&schedule(t)).date_naive());
            t += chrono::Duration::hours(1);
        }
        days.len()
    }

    fn assert_no_long_gap(made: &[DateTime<Utc>]) {
        for w in made.windows(2) {
            // Checked hourly here (the app checks every minute): a day plus one check.
            assert!(w[1] - w[0] <= chrono::Duration::hours(25), "gap {:?}", w);
        }
    }

    #[test]
    fn steady_zone_gives_one_backup_per_day() {
        let made = simulate("2026-10-01T00:30:00Z", 24 * 10, |_| zone(9));
        assert_eq!(
            made.len(),
            days_covered("2026-10-01T00:30:00Z", 24 * 10, |_| zone(9))
        );
        assert_no_long_gap(&made);
    }

    #[test]
    fn a_zone_change_east_across_midnight_gives_at_most_one_extra() {
        // Travels from UTC-5 to UTC+8 on the 5th at noon UTC.
        let change = utc("2026-10-05T12:00:00Z");
        let schedule = |t: DateTime<Utc>| if t < change { zone(-5) } else { zone(8) };
        let made = simulate("2026-10-01T00:30:00Z", 24 * 10, schedule);
        // One per calendar day touched, plus at most one for the jump.
        let days = days_covered("2026-10-01T00:30:00Z", 24 * 10, schedule);
        assert!(made.len() <= days + 1, "{} for {days} days", made.len());
        assert_no_long_gap(&made);
    }

    #[test]
    fn a_zone_change_west_across_midnight_gives_at_most_one_extra() {
        let change = utc("2026-10-05T01:00:00Z");
        let schedule = |t: DateTime<Utc>| if t < change { zone(12) } else { zone(-8) };
        let made = simulate("2026-10-01T00:30:00Z", 24 * 10, schedule);
        let days = days_covered("2026-10-01T00:30:00Z", 24 * 10, schedule);
        assert!(made.len() <= days + 1, "{} for {days} days", made.len());
        assert_no_long_gap(&made);
    }

    #[test]
    fn daylight_saving_shifts_make_no_gap_and_no_extra_backup() {
        // Spring forward and fall back by one hour, with the backup made near midnight.
        for (before, after) in [(0, 1), (1, 0), (10, 11), (11, 10)] {
            let change = utc("2026-10-05T14:00:00Z");
            let schedule = |t: DateTime<Utc>| {
                if t < change {
                    zone(before)
                } else {
                    zone(after)
                }
            };
            let made = simulate("2026-10-01T22:30:00Z", 24 * 10, schedule);
            let days = days_covered("2026-10-01T22:30:00Z", 24 * 10, schedule);
            assert!(
                made.len() <= days + 1,
                "{before}->{after}: {} for {days} days",
                made.len()
            );
            assert_no_long_gap(&made);
        }
    }

    #[test]
    fn many_zone_changes_never_leave_a_long_gap() {
        // The zone jumps every 17 hours between far-apart offsets.
        let zones = [-12, 14, 0, 5, -9, 13, 2];
        let start = utc("2026-10-01T00:00:00Z");
        let schedule =
            |t: DateTime<Utc>| zone(zones[((t - start).num_hours() / 17) as usize % zones.len()]);
        let made = simulate("2026-10-01T00:00:00Z", 24 * 14, schedule);
        assert_no_long_gap(&made);
        // At most one extra per zone change (about 20 of them) on top of the days touched.
        let days = days_covered("2026-10-01T00:00:00Z", 24 * 14, schedule);
        assert!(made.len() <= days + 20, "{} for {days} days", made.len());
    }

    // ----- retention

    #[test]
    fn daily_retention_keeps_the_newest_by_export_time_and_never_touches_other_files() {
        let (_d, mut db) = open();
        keep(&mut db, 2);
        let dir = db.paths().backups_dir();
        let tz = zone(0);
        // A manual one, a file Cairn can't read, a stranger's file and a folder.
        let manual = run(&db, BackupKind::Manual, "2026-10-01T10:00:00Z", &tz, &ok);
        std::fs::write(
            dir.join("cairn-daily-TESTPC-2026-01-01-0000.zip"),
            b"not a zip",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), b"mine").unwrap();
        std::fs::write(dir.join("cairn-daily-old.zip.part"), b"x").unwrap();
        std::fs::create_dir(dir.join("cairn-daily-folder.zip")).unwrap();
        for day in ["05", "06", "07", "08"] {
            run(
                &db,
                BackupKind::Daily,
                &format!("2026-10-{day}T09:00:00Z"),
                &tz,
                &ok,
            );
        }
        let left = names(&dir);
        for gone in [
            "cairn-daily-TESTPC-2026-10-05-0900.zip",
            "cairn-daily-TESTPC-2026-10-06-0900.zip",
        ] {
            assert!(!left.iter().any(|n| n == gone), "{gone} should be deleted");
        }
        for kept in [
            "cairn-daily-TESTPC-2026-10-07-0900.zip",
            "cairn-daily-TESTPC-2026-10-08-0900.zip",
            "cairn-daily-TESTPC-2026-01-01-0000.zip",
            "notes.txt",
            "cairn-daily-folder.zip",
            manual.file_name.as_str(),
        ] {
            assert!(left.iter().any(|n| n == kept), "{kept} missing: {left:?}");
        }
    }

    #[test]
    fn retention_orders_by_the_export_time_inside_the_file_not_by_name() {
        let (_d, mut db) = open();
        keep(&mut db, 1);
        // By name the later backup looks older (named in another zone): the order must come
        // from the export time inside the files.
        let newer = run(
            &db,
            BackupKind::Daily,
            "2026-10-09T09:00:00Z",
            &zone(14),
            &ok,
        );
        let latest = run(
            &db,
            BackupKind::Daily,
            "2026-10-10T09:00:00Z",
            &zone(-12),
            &ok,
        );
        assert!(latest.file_name < newer.file_name);
        let left = list(&db).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].exported_at, latest.exported_at);
    }

    #[test]
    fn safety_copies_keep_the_newest_n_of_each_kind() {
        let (_d, mut db) = open();
        keep(&mut db, 2);
        let dir = db.paths().backups_dir();
        let tz = zone(0);
        // pre-import: made by real exports with different export times.
        let mut pre_imports = Vec::new();
        for day in ["01", "02", "03"] {
            let f = run(
                &db,
                BackupKind::Manual,
                &format!("2026-09-{day}T00:00:00Z"),
                &tz,
                &ok,
            );
            let target = dir.join(format!("pre-import-2026090{}-000000-000.zip", &day[1..]));
            std::fs::rename(&f.path, &target).unwrap();
            pre_imports.push(target);
        }
        // pre-migration: ordered by the UTC stamp in the name; one with a stamp that can't be read.
        for name in [
            "pre-migration-v2-20260901-000000.sqlite",
            "pre-migration-v3-20260902-000000.sqlite",
            "pre-migration-v8-20260903-000000.sqlite",
            "pre-migration-vX-garbage.sqlite",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        run(&db, BackupKind::Daily, "2026-10-01T09:00:00Z", &tz, &ok);
        let left = names(&dir);
        assert!(!pre_imports[0].exists());
        assert!(pre_imports[1].exists() && pre_imports[2].exists());
        assert!(!left.contains(&"pre-migration-v2-20260901-000000.sqlite".to_string()));
        assert!(left.contains(&"pre-migration-v3-20260902-000000.sqlite".to_string()));
        assert!(left.contains(&"pre-migration-v8-20260903-000000.sqlite".to_string()));
        assert!(left.contains(&"pre-migration-vX-garbage.sqlite".to_string()));
    }

    #[test]
    fn a_custom_folder_gets_the_backups_and_the_safety_copies_stay_in_the_data_folder() {
        let (_d, mut db) = open();
        let custom = tempfile::tempdir().unwrap();
        let target = custom.path().join("cairn");
        check_folder(&target).unwrap();
        settings::update(
            &mut db,
            settings::SettingsPatch {
                backup_folder: Some(target.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        folder_changed(&db).unwrap();
        let f = run(
            &db,
            BackupKind::Daily,
            "2026-10-16T09:00:00Z",
            &zone(0),
            &ok,
        );
        assert_eq!(Path::new(&f.path).parent().unwrap(), target);
        let st = status(&db).unwrap();
        assert!(!st.is_default_folder);
        assert_eq!(list(&db).unwrap().len(), 1);
        assert!(names(&db.paths().backups_dir()).is_empty());
    }

    #[test]
    fn folder_check_needs_a_writable_full_path() {
        assert!(check_folder(Path::new("relative")).is_err());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        std::fs::write(&file, b"x").unwrap();
        assert!(check_folder(&file.join("sub")).is_err());
        check_folder(dir.path()).unwrap();
        assert!(names(dir.path()) == vec!["a-file".to_string()]);
    }

    // ----- the computer's name (D-58)

    #[test]
    fn computer_names_are_cleaned_for_file_names() {
        assert_eq!(clean_computer_name("LAPTOP-7F3K"), "LAPTOP-7F3K");
        assert_eq!(clean_computer_name("my.host name_1\n"), "myhostname1");
        assert_eq!(clean_computer_name("--odd--"), "odd");
        assert_eq!(clean_computer_name("我的电脑"), "我的电脑");
        assert_eq!(clean_computer_name(" ._ "), "computer");
        assert_eq!(clean_computer_name(""), "computer");
        assert_eq!(clean_computer_name(&"a".repeat(60)).len(), 24);
        // The cut never leaves a hyphen at the end.
        assert_eq!(
            clean_computer_name(&format!("{}-b", "a".repeat(23))),
            "a".repeat(23)
        );
        assert!(!computer_name().is_empty());
    }

    #[test]
    fn names_parse_from_the_end_in_old_and_new_forms() {
        let p = |n: &str| parse_name(n);
        assert_eq!(
            p("cairn-daily-LAPTOP-2026-10-17-0912.zip"),
            Some((BackupKind::Daily, Some("LAPTOP".into())))
        );
        assert_eq!(
            p("cairn-manual-LAPTOP-2026-10-17-0912-3.zip"),
            Some((BackupKind::Manual, Some("LAPTOP".into())))
        );
        // Hyphens and digits in the computer part.
        assert_eq!(
            p("cairn-daily-DESKTOP-AB-12-2026-10-17-0912-2.zip"),
            Some((BackupKind::Daily, Some("DESKTOP-AB-12".into())))
        );
        assert_eq!(
            p("cairn-daily-2026-2026-10-17-0912.zip"),
            Some((BackupKind::Daily, Some("2026".into())))
        );
        assert_eq!(
            p("cairn-daily-1234-2026-10-17-0912-2.zip"),
            Some((BackupKind::Daily, Some("1234".into())))
        );
        // Old forms: no computer.
        assert_eq!(
            p("cairn-daily-2026-10-16-0912.zip"),
            Some((BackupKind::Daily, None))
        );
        assert_eq!(
            p("cairn-manual-2026-10-16-1530-2.zip"),
            Some((BackupKind::Manual, None))
        );
        // Not Cairn's form.
        assert_eq!(p("cairn-daily-old.zip"), None);
        assert_eq!(p("cairn-daily-PC-2026-10-16.zip"), None);
        assert_eq!(p("notes.zip"), None);
    }

    #[test]
    fn a_new_name_carries_the_computer_and_clashes_still_count_up() {
        let dir = tempfile::tempdir().unwrap();
        let now = utc("2026-10-17T09:12:00Z");
        let a = free_name(dir.path(), BackupKind::Daily, "PC-1", now, &zone(0));
        assert_eq!(
            a.file_name().unwrap(),
            "cairn-daily-PC-1-2026-10-17-0912.zip"
        );
        std::fs::write(&a, b"x").unwrap();
        let b = free_name(dir.path(), BackupKind::Daily, "PC-1", now, &zone(0));
        assert_eq!(
            b.file_name().unwrap(),
            "cairn-daily-PC-1-2026-10-17-0912-2.zip"
        );
        // Another computer's name is not a clash.
        let c = free_name(dir.path(), BackupKind::Daily, "PC-2", now, &zone(0));
        assert_eq!(
            c.file_name().unwrap(),
            "cairn-daily-PC-2-2026-10-17-0912.zip"
        );
    }

    #[test]
    fn retention_deletes_only_this_computers_dailies() {
        let (_d, mut db) = open();
        keep(&mut db, 10);
        let dir = db.paths().backups_dir();
        let tz = zone(0);
        // An old-form file (made by an older Cairn) and another computer's files.
        let old = run(&db, BackupKind::Manual, "2026-09-01T09:00:00Z", &tz, &ok);
        let old_form = dir.join("cairn-daily-2026-09-01-0900.zip");
        std::fs::rename(&old.path, &old_form).unwrap();
        for day in ["02", "03", "04"] {
            let at = format!("2026-09-{day}T09:00:00Z");
            run_as("OTHERPC", &db, BackupKind::Daily, &at, &tz, &ok);
        }
        keep(&mut db, 1);
        for day in ["05", "06", "07"] {
            let at = format!("2026-09-{day}T09:00:00Z");
            run_as("TESTPC", &db, BackupKind::Daily, &at, &tz, &ok);
        }
        let left = names(&dir);
        // This computer keeps one; the others lose nothing.
        assert_eq!(
            left.iter().filter(|n| n.contains("TESTPC")).count(),
            1,
            "{left:?}"
        );
        assert!(left.contains(&"cairn-daily-TESTPC-2026-09-07-0900.zip".to_string()));
        assert_eq!(left.iter().filter(|n| n.contains("OTHERPC")).count(), 3);
        assert!(old_form.exists());
        // The list shows each file's computer; the old one has none.
        let all = list(&db).unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all.iter().filter(|f| f.computer.is_none()).count(), 1);
        // *Last backup* is this computer's, not the newest of the folder.
        keep(&mut db, 10);
        run_as(
            "OTHERPC",
            &db,
            BackupKind::Daily,
            "2026-09-20T09:00:00Z",
            &tz,
            &ok,
        );
        let st = status_as(&db, "TESTPC".into()).unwrap();
        assert_eq!(
            st.last.unwrap().file_name,
            "cairn-daily-TESTPC-2026-09-07-0900.zip"
        );
    }

    // ----- the database check (D-52)

    #[test]
    fn a_failed_check_still_backs_up_but_deletes_nothing_until_a_check_passes() {
        let (_d, mut db) = open();
        keep(&mut db, 1);
        let tz = zone(0);
        run(&db, BackupKind::Daily, "2026-10-01T09:00:00Z", &tz, &ok);
        run(&db, BackupKind::Daily, "2026-10-02T09:00:00Z", &tz, &ok);
        assert_eq!(list(&db).unwrap().len(), 1);

        // Checks fail on the 3rd and 4th: the backups are written, nothing is deleted.
        run(&db, BackupKind::Daily, "2026-10-03T09:00:00Z", &tz, &broken);
        assert!(status(&db).unwrap().check_failed);
        run(&db, BackupKind::Daily, "2026-10-04T09:00:00Z", &tz, &broken);
        assert_eq!(list(&db).unwrap().len(), 3);
        // A manual backup doesn't change the check result.
        run(&db, BackupKind::Manual, "2026-10-04T10:00:00Z", &tz, &ok);
        assert!(status(&db).unwrap().check_failed);

        // A passing check clears the warning and retention catches up.
        run(&db, BackupKind::Daily, "2026-10-05T09:00:00Z", &tz, &ok);
        assert!(!status(&db).unwrap().check_failed);
        let left = list(&db).unwrap();
        assert_eq!(
            left.iter().filter(|f| f.kind == BackupKind::Daily).count(),
            1
        );
        assert_eq!(
            left.iter().filter(|f| f.kind == BackupKind::Manual).count(),
            1
        );
    }

    #[test]
    fn quick_check_passes_on_a_healthy_database_and_fails_on_a_damaged_one() {
        let (_d, db) = open();
        assert!(quick_check(db.conn()).unwrap());
        // A page-level problem: a table whose index doesn't match its rows.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.sqlite");
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "PRAGMA journal_mode = DELETE;
                 CREATE TABLE t (a INTEGER PRIMARY KEY, b TEXT);
                 CREATE INDEX t_b ON t (b);",
            )
            .unwrap();
            for i in 0..2000 {
                c.execute("INSERT INTO t (b) VALUES (?1)", [format!("row {i}")])
                    .unwrap();
            }
        }
        // Zero out one page in the middle of the file.
        let mut bytes = std::fs::read(&path).unwrap();
        let page = 4096;
        let at = bytes.len() / page / 2 * page;
        bytes[at..at + page].fill(0xFF);
        std::fs::write(&path, bytes).unwrap();
        let c = Connection::open(&path).unwrap();
        assert!(!matches!(quick_check(&c), Ok(true)));
    }

    fn set_folder(db: &mut Db, folder: &str) {
        settings::update(
            db,
            settings::SettingsPatch {
                backup_folder: Some(folder.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn an_import_keeps_this_computers_backup_folder_and_takes_the_rest_from_the_file() {
        let tz = zone(0);
        // The file comes from a computer with its own folder and another setting.
        let (_d2, mut other) = open();
        let elsewhere = tempfile::tempdir().unwrap();
        set_folder(&mut other, &elsewhere.path().to_string_lossy());
        keep(&mut other, 7);
        let archive = run(&other, BackupKind::Manual, "2026-10-17T09:00:00Z", &tz, &ok);
        let (_d3, empty_default) = open();
        let default_archive = run(
            &empty_default,
            BackupKind::Manual,
            "2026-10-17T09:00:00Z",
            &tz,
            &ok,
        );

        // A custom folder here stays, whatever the file says.
        let (_d, mut db) = open();
        let mine = tempfile::tempdir().unwrap();
        let mine_path = mine.path().to_string_lossy().into_owned();
        set_folder(&mut db, &mine_path);
        crate::export::import_from(&mut db, Path::new(&archive.path)).unwrap();
        let s = settings::get(db.conn()).unwrap();
        assert_eq!(s.backup_folder, mine_path);
        assert_eq!(s.backup_keep, 7, "other settings come from the file");
        assert_eq!(backup_folder(&db).unwrap(), mine.path());
        // Restoring a backup of this very folder is the same import.
        crate::export::import_from(&mut db, Path::new(&default_archive.path)).unwrap();
        assert_eq!(settings::get(db.conn()).unwrap().backup_folder, mine_path);

        // The default (empty) stays empty although the file had a custom folder.
        let (_d4, mut db2) = open();
        crate::export::import_from(&mut db2, Path::new(&archive.path)).unwrap();
        assert_eq!(settings::get(db2.conn()).unwrap().backup_folder, "");
        assert_eq!(backup_folder(&db2).unwrap(), db2.paths().backups_dir());
    }

    #[test]
    fn status_is_local_and_an_import_does_not_bring_in_or_lose_it() {
        let (_d, mut db) = open();
        let tz = zone(0);
        let f = run(&db, BackupKind::Daily, "2026-10-16T09:00:00Z", &tz, &ok);
        // An archive from "another computer" that has its own (fake) backup status.
        let (_d2, other) = open();
        let archive = run(&other, BackupKind::Manual, "2026-10-17T09:00:00Z", &tz, &ok);
        crate::export::import_from(&mut db, Path::new(&archive.path)).unwrap();
        // This computer's daily backup is still today's.
        assert!(!is_due_in(&db, utc("2026-10-16T20:00:00Z"), &tz).unwrap());
        assert!(
            list(&db)
                .unwrap()
                .iter()
                .any(|b| b.file_name == f.file_name)
        );
    }
}
