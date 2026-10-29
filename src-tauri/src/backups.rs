//! The daily backup while Cairn runs (NFR-REL-2; M7 part A): on start, then checked regularly
//! (hourly, and soon after the computer wakes from sleep). The work itself is
//! `cairn_core::backup`.
//!
//! `is_due` is a cheap look at the settings and the local status, so the thread asks every minute.
//! That covers both "hourly" and "soon after wake" (after sleep, the next tick sees a new local
//! day), and a failed backup is retried an hour later because `is_due` says so.

use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use cairn_core::Db;
use cairn_core::backup::{self, BackupFile, BackupKind};
use tauri::{AppHandle, Manager};

use crate::events::{self, Entity};
use crate::state::AppState;

/// A short wait after start, so start-up itself isn't slowed by the first backup.
const START_DELAY: Duration = Duration::from_secs(5);
/// How often the thread looks at whether a daily backup is due.
const CHECK_EVERY: Duration = Duration::from_secs(60);

/// One backup at a time, whoever asks (the thread or *Back up now*).
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Holds backups off while the data files are swapped (an import or the dev reset, NFR-REL-5), and
/// waits for one in progress. Take it before the database lock, as [`make`] does.
pub fn exclusive() -> MutexGuard<'static, ()> {
    lock(&ONE_AT_A_TIME)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not stop backups for the rest of the run.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Makes a backup of `kind` now and tells the UI. Blocking: call from a blocking thread. The
/// database lock is held only to read the settings, not while the export runs.
pub fn make(
    app: &AppHandle,
    db: &Arc<Mutex<Db>>,
    kind: BackupKind,
) -> cairn_core::Result<BackupFile> {
    let _one = lock(&ONE_AT_A_TIME);
    let job = backup::prepare(&lock(db), kind)?;
    let result = job.run_now();
    // Also after a failure: the *Last backup* line shows it.
    events::emit_changed(app, &[Entity::Backups]);
    result
}

/// Starts the background check. Never blocks start-up and never panics.
pub fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("backups".into())
        .spawn(move || {
            std::thread::sleep(START_DELAY);
            loop {
                // Whatever goes wrong in one round must not end the loop.
                if std::panic::catch_unwind(AssertUnwindSafe(|| tick(&app))).is_err() {
                    tracing::error!("the backup check panicked");
                }
                // FR-LLM-AS-13: the same minute's look also deletes expired archived conversations.
                let expire = || crate::commands::conversations::delete_expired(&app);
                if std::panic::catch_unwind(AssertUnwindSafe(expire)).is_err() {
                    tracing::error!("deleting expired conversations panicked");
                }
                std::thread::sleep(CHECK_EVERY);
            }
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "could not start the backup thread");
    }
}

/// One look: if a daily backup is due, make it.
fn tick(app: &AppHandle) {
    let Some(db) = app.state::<AppState>().db_handle() else {
        return;
    };
    let due = backup::is_due_now(&lock(&db));
    match due {
        Ok(true) => {
            // The outcome is logged and recorded in the status by the core; nothing else to do.
            let _ = make(app, &db, BackupKind::Daily);
        }
        Ok(false) => {}
        Err(e) => tracing::warn!(error = %e, "could not tell whether a backup is due"),
    }
}
