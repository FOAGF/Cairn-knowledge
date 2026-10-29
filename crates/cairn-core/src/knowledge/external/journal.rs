//! What one database transaction did to the folder (D-72), so the folder and the index never
//! drift apart unnoticed.
//!
//! External mode writes, moves and trashes files inside the transaction of the change, before
//! its rows are written. Every such step notes the path as Cairn's own change (`note_own_write`),
//! which also records it here, in a journal of the thread's outermost transaction (`Db::tx` opens
//! it). Then:
//! - **Commit:** the work that had to wait for it runs: images no page uses any more are moved to
//!   the trash (FR-KN-EXT-12) only now, when the rows that stopped using them are saved.
//! - **Rollback** (also of one item of a bulk change, through a savepoint [`mark`]): the files are
//!   as the change left them while the rows are not. Cairn's notes on those paths are forgotten,
//!   and the paths go to the watcher (or wait for it) as changes to scan, so the index is brought
//!   up to the folder instead of a later edit recreating a file at an old path.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::Ext;

#[derive(Default)]
struct Journal {
    depth: usize,
    touched: Vec<PathBuf>,
    /// Image paths a change stopped using, per root, to check once it is committed.
    images: Vec<(Ext, Vec<String>)>,
}

thread_local! {
    static JOURNAL: RefCell<Option<Journal>> = const { RefCell::new(None) };
}

/// A transaction's place in the journal; closing it without [`Guard::committed`] is a rollback.
pub(crate) struct Guard {
    done: bool,
}

/// Opens the journal of this thread's transaction (only the outermost one acts on it).
pub(crate) fn open() -> Guard {
    JOURNAL.with(|j| {
        j.borrow_mut().get_or_insert_with(Journal::default).depth += 1;
    });
    Guard { done: false }
}

impl Guard {
    /// The transaction committed: runs what waited for it.
    pub(crate) fn committed(mut self, conn: &Connection) {
        self.done = true;
        if let Some(j) = close() {
            for (ext, paths) in j.images {
                super::store::drop_unused_images(conn, &ext, &paths);
            }
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if !self.done
            && let Some(j) = close()
        {
            take_back(j.touched);
        }
    }
}

/// Leaves one level; returns the journal when the outermost transaction ends.
fn close() -> Option<Journal> {
    JOURNAL.with(|j| {
        let mut slot = j.borrow_mut();
        let journal = slot.as_mut()?;
        journal.depth = journal.depth.saturating_sub(1);
        if journal.depth == 0 { slot.take() } else { None }
    })
}

/// Changes that were rolled back: the watcher must see their paths again.
fn take_back(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    for p in &paths {
        super::forget_own_write(p);
    }
    tracing::info!(paths = paths.len(), "a change to the knowledge folder was rolled back; its files will be read again");
    super::watch::rescan_later(paths);
}

/// Records a path the current transaction changed on disk (nothing outside a transaction).
pub(crate) fn touched(abs: &Path) {
    JOURNAL.with(|j| {
        if let Some(journal) = j.borrow_mut().as_mut() {
            journal.touched.push(abs.to_path_buf());
        }
    });
}

/// After commit, moves to the trash those of `paths` (`folder:` paths) no page uses any more.
/// Outside a transaction it is done at once.
pub(crate) fn drop_images_after_commit(conn: &Connection, ext: &Ext, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let deferred = JOURNAL.with(|j| match j.borrow_mut().as_mut() {
        Some(journal) => {
            journal.images.push((ext.clone(), paths.clone()));
            true
        }
        None => false,
    });
    if !deferred {
        super::store::drop_unused_images(conn, ext, &paths);
    }
}

/// A point in the journal, for a savepoint.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    touched: usize,
    images: usize,
}

/// Where the journal is now (`bulk::each` takes one per item).
pub(crate) fn mark() -> Mark {
    JOURNAL.with(|j| match j.borrow().as_ref() {
        Some(journal) => Mark {
            touched: journal.touched.len(),
            images: journal.images.len(),
        },
        None => Mark { touched: 0, images: 0 },
    })
}

/// The changes since `mark` were rolled back (to a savepoint).
pub(crate) fn rollback_to(mark: Mark) {
    let back = JOURNAL.with(|j| {
        let mut slot = j.borrow_mut();
        let journal = slot.as_mut()?;
        journal.images.truncate(mark.images.min(journal.images.len()));
        let from = mark.touched.min(journal.touched.len());
        Some(journal.touched.split_off(from))
    });
    if let Some(paths) = back {
        take_back(paths);
    }
}
