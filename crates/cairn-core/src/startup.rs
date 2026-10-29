//! Start-up housekeeping, before and after the database opens (NFR-REL-5, D-52).
//!
//! - [`recover_interrupted_import`] runs **before** the database opens. An import swaps the
//!   database and attachments by renaming the current files to `*.pre-import` first
//!   ([`crate::Db::replace_from`]). If Cairn stopped in the middle, those files are still there:
//!   the data from before the import is put back and the half-swapped files are removed.
//!   **Finished or halfway:** once the imported data is fully in place (and has opened and
//!   migrated), `replace_from` writes the marker file `import-swapped.marker`, deletes the
//!   stashes, then deletes the marker. So at start-up:
//!   - stashes and no marker: the swap itself stopped, so the data from before is put back;
//!   - stashes and the marker: the swap finished and only the clean-up was cut short, so the
//!     stashes are just deleted (the import is never undone);
//!   - only the marker: it is deleted.
//! - [`sweep_leftovers`] removes Cairn's own half-made files: unfinished imports
//!   (`import-staging-*` folders in the data folder) and half-written exports or backups
//!   (`*.part`) in the data folder, its `backups` folder and the chosen backup folder. Nothing
//!   else is touched.

use std::path::{Path, PathBuf};

use crate::db::{import_live_files, import_swapped_marker, pre_import_stash, remove_any};
use crate::{DataPaths, Result};

/// A message for the user about something done at start-up, shown once in the UI.
pub const IMPORT_INTERRUPTED: &str = "An import was interrupted. Your data from before it is back.";

/// Put in a notice when the data from before an interrupted import could not be put back.
pub const IMPORT_NOT_RESTORED: &str = "An import was interrupted and your data from before it could not be put back. The files are still in the data folder, with names ending in .pre-import.";

/// Puts back the data from before an import that stopped halfway. Returns whether it did.
pub fn recover_interrupted_import(paths: &DataPaths) -> Result<bool> {
    let live = import_live_files(paths);
    let marker = import_swapped_marker(paths);
    let stashes: Vec<&PathBuf> = live
        .iter()
        .filter(|p| pre_import_stash(p).exists())
        .collect();

    if stashes.is_empty() {
        // Nothing to undo; a marker alone is the leftover of a finished import.
        remove_any(&marker)?;
        return Ok(false);
    }
    if marker.exists() {
        // The swap finished; only the clean-up was cut short. Never undo it.
        tracing::info!("finishing the clean-up of an import that had completed");
        for p in &live {
            remove_any(&pre_import_stash(p))?;
        }
        remove_any(&marker)?;
        return Ok(false);
    }

    tracing::warn!("an import was interrupted; putting back the data from before it");
    // The half-swapped files are removed before the old ones are put back. A WAL or shared-memory
    // file that belongs to the half-swapped database must go even when the old data had none.
    let old_db_stashed = pre_import_stash(&live[0]).exists();
    for (i, p) in live.iter().enumerate() {
        let stash = pre_import_stash(p);
        if stash.exists() {
            remove_any(p)?;
            std::fs::rename(&stash, p)?;
        } else if old_db_stashed && (i == 1 || i == 2) {
            remove_any(p)?;
        }
    }
    Ok(true)
}

/// Removes Cairn's half-made files in the data folder and in `backup_folders`. Returns how many
/// files or folders were removed.
pub fn sweep_leftovers(paths: &DataPaths, backup_folders: &[PathBuf]) -> Result<usize> {
    let mut removed = 0;

    // Unfinished imports (and dev resets) in the data folder.
    for entry in read_dir_or_empty(&paths.root) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("import-staging-") && entry.file_type().is_ok_and(|t| t.is_dir()) {
            remove_any(&entry.path())?;
            removed += 1;
        }
    }

    // Half-written exports and backups.
    let mut folders = vec![paths.root.clone(), paths.backups_dir()];
    for f in backup_folders {
        if !folders.contains(f) {
            folders.push(f.clone());
        }
    }
    for folder in folders {
        for entry in read_dir_or_empty(&folder) {
            if is_partial_file(&entry.path()) && entry.file_type().is_ok_and(|t| t.is_file()) {
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!(removed, "removed leftovers of unfinished work");
    }
    Ok(removed)
}

fn read_dir_or_empty(dir: &Path) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(dir)
        .map(|e| e.flatten().collect())
        .unwrap_or_default()
}

/// Whether `path` is one of Cairn's half-written files (`*.part`).
pub fn is_partial_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "part")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;

    /// A data folder whose settings hold `name` as the user name, so the data can be told apart.
    fn data_with(root: &Path, name: &str) -> DataPaths {
        let mut db = Db::open_at(root).unwrap();
        crate::settings::update(
            &mut db,
            crate::settings::SettingsPatch {
                user_name: Some(name.into()),
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(db.paths().attachments_dir().join("img.png"), name).unwrap();
        db.paths().clone()
    }

    fn user_name(paths: &DataPaths) -> String {
        let db = Db::open(paths.clone()).unwrap();
        crate::settings::get(db.conn()).unwrap().user_name
    }

    fn image(paths: &DataPaths) -> String {
        std::fs::read_to_string(paths.attachments_dir().join("img.png")).unwrap()
    }

    /// Sets `from`'s files aside the way `replace_from` does, and puts `to`'s in place of them.
    fn stash_old_files(paths: &DataPaths) {
        for p in import_live_files(paths) {
            if p.exists() {
                std::fs::rename(&p, pre_import_stash(&p)).unwrap();
            }
        }
    }

    fn leftovers(paths: &DataPaths) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&paths.root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".pre-import") || n.ends_with(".marker"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn nothing_to_do_on_a_normal_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        assert!(!recover_interrupted_import(&paths).unwrap());
        assert_eq!(user_name(&paths), "old");
    }

    #[test]
    fn stopped_after_stashing_only_the_database_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        drop(Db::open(paths.clone()).unwrap()); // folds the WAL into the file
        std::fs::rename(paths.db_file(), pre_import_stash(&paths.db_file())).unwrap();
        // The attachments were not stashed yet.
        assert!(recover_interrupted_import(&paths).unwrap());
        assert_eq!(user_name(&paths), "old");
        assert_eq!(image(&paths), "old");
        assert!(leftovers(&paths).is_empty());
    }

    #[test]
    fn stopped_after_the_new_database_was_renamed_in_but_before_the_attachments() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        let staging = tempfile::tempdir().unwrap();
        let new = data_with(staging.path(), "new");
        drop(Db::open(paths.clone()).unwrap());
        stash_old_files(&paths);
        std::fs::rename(new.db_file(), paths.db_file()).unwrap();
        // The new database was opened once (a WAL file of its own exists), attachments not moved.
        std::fs::write(paths.root.join("cairn.sqlite-wal"), b"half-swapped").unwrap();
        assert!(recover_interrupted_import(&paths).unwrap());
        assert_eq!(user_name(&paths), "old");
        assert_eq!(image(&paths), "old");
        assert!(leftovers(&paths).is_empty());
    }

    #[test]
    fn stopped_after_the_attachments_were_swapped_too() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        let staging = tempfile::tempdir().unwrap();
        let new = data_with(staging.path(), "new");
        drop(Db::open(paths.clone()).unwrap());
        stash_old_files(&paths);
        std::fs::rename(new.db_file(), paths.db_file()).unwrap();
        std::fs::rename(new.attachments_dir(), paths.attachments_dir()).unwrap();
        // No marker yet: the new database had not opened and migrated.
        assert!(recover_interrupted_import(&paths).unwrap());
        assert_eq!(user_name(&paths), "old");
        assert_eq!(image(&paths), "old");
        assert!(leftovers(&paths).is_empty());
    }

    #[test]
    fn a_finished_swap_whose_clean_up_was_cut_is_not_undone() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        let staging = tempfile::tempdir().unwrap();
        let new = data_with(staging.path(), "new");
        drop(Db::open(paths.clone()).unwrap());
        stash_old_files(&paths);
        std::fs::rename(new.db_file(), paths.db_file()).unwrap();
        std::fs::rename(new.attachments_dir(), paths.attachments_dir()).unwrap();
        // The swap finished: the marker is there, the stashes are not yet deleted.
        std::fs::write(import_swapped_marker(&paths), b"").unwrap();
        assert!(!recover_interrupted_import(&paths).unwrap());
        assert_eq!(user_name(&paths), "new");
        assert_eq!(image(&paths), "new");
        assert!(leftovers(&paths).is_empty());
    }

    #[test]
    fn a_real_import_leaves_no_stash_and_no_marker() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        let other = tempfile::tempdir().unwrap();
        let other_paths = data_with(other.path(), "new");
        let archive = other.path().join("a.zip");
        let src = Db::open(other_paths).unwrap();
        crate::export::export_to(&src, &archive).unwrap();
        let mut db = Db::open(paths.clone()).unwrap();
        crate::export::import_from(&mut db, &archive).unwrap();
        drop(db);
        assert!(leftovers(&paths).is_empty());
        assert!(!recover_interrupted_import(&paths).unwrap());
        assert_eq!(user_name(&paths), "new");
    }

    #[test]
    fn a_lone_marker_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        std::fs::write(import_swapped_marker(&paths), b"").unwrap();
        assert!(!recover_interrupted_import(&paths).unwrap());
        assert!(leftovers(&paths).is_empty());
        assert_eq!(user_name(&paths), "old");
    }

    #[test]
    fn sweep_removes_only_cairns_own_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let paths = data_with(dir.path(), "old");
        let custom = tempfile::tempdir().unwrap();
        let w = |p: PathBuf, text: &str| std::fs::write(p, text).unwrap();

        // Cairn's own leftovers.
        let staging = paths.root.join("import-staging-20261016-000000-000");
        std::fs::create_dir_all(staging.join("attachments")).unwrap();
        w(staging.join("cairn.sqlite"), "x");
        w(paths.root.join("export.zip.part"), "x");
        w(
            paths
                .backups_dir()
                .join("cairn-daily-2026-10-16-0912.zip.part"),
            "x",
        );
        w(
            custom.path().join("cairn-manual-2026-10-16-1530.zip.part"),
            "x",
        );

        // Things that stay.
        w(paths.root.join("notes.txt"), "mine");
        w(
            paths.root.join("import-staging-notes.txt"),
            "a file, not a folder",
        );
        w(
            paths.backups_dir().join("cairn-daily-2026-10-15-0912.zip"),
            "backup",
        );
        w(
            paths
                .backups_dir()
                .join("pre-migration-v1-20261001-000000.sqlite"),
            "copy",
        );
        w(
            custom.path().join("cairn-daily-2026-10-15-0912.zip"),
            "backup",
        );
        w(custom.path().join("holiday.part2"), "x");
        // Not in a folder Cairn looks in: a sub-folder of the data folder, and a stranger's folder.
        let other = tempfile::tempdir().unwrap();
        w(other.path().join("theirs.part"), "x");
        std::fs::create_dir_all(paths.root.join("deeper")).unwrap();
        w(paths.root.join("deeper").join("x.part"), "x");
        w(
            paths.attachments_dir().join("img.part"),
            "an attachment named .part",
        );

        let n = sweep_leftovers(&paths, &[custom.path().to_path_buf()]).unwrap();
        assert_eq!(n, 4);
        assert!(!staging.exists());
        assert!(!paths.root.join("export.zip.part").exists());
        assert!(
            !paths
                .backups_dir()
                .join("cairn-daily-2026-10-16-0912.zip.part")
                .exists()
        );
        assert!(
            !custom
                .path()
                .join("cairn-manual-2026-10-16-1530.zip.part")
                .exists()
        );
        for stays in [
            paths.root.join("notes.txt"),
            paths.root.join("import-staging-notes.txt"),
            paths.backups_dir().join("cairn-daily-2026-10-15-0912.zip"),
            paths
                .backups_dir()
                .join("pre-migration-v1-20261001-000000.sqlite"),
            custom.path().join("cairn-daily-2026-10-15-0912.zip"),
            custom.path().join("holiday.part2"),
            other.path().join("theirs.part"),
            paths.root.join("deeper").join("x.part"),
            paths.attachments_dir().join("img.part"),
            paths.db_file(),
        ] {
            assert!(stays.exists(), "{} was removed", stays.display());
        }
        // Nothing left to do the second time; a missing backup folder is fine.
        assert_eq!(
            sweep_leftovers(&paths, &[PathBuf::from("/no/such/folder")]).unwrap(),
            0
        );
    }
}
