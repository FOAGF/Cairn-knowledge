//! Opening the database: pragmas, automatic backup, then migrations (NFR-REL-1, NFR-REL-3).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, Transaction};

use crate::Result;
use crate::migrations::{latest_version, migrate};
use crate::paths::DataPaths;

/// An open Cairn database. The Tauri layer keeps one behind a mutex.
pub struct Db {
    conn: Connection,
    paths: DataPaths,
}

impl Db {
    /// Opens (or creates) the database in `paths.root` and brings the schema up to date.
    /// If an existing database needs migrating, a copy is saved in `backups/` first.
    pub fn open(paths: DataPaths) -> Result<Db> {
        paths.ensure()?;
        let existed = paths.db_file().exists();
        let mut conn = Connection::open(paths.db_file())?;
        let version = user_version(&conn)?;
        let source_version = crate::migrations::source_version(&conn)?;
        configure(&conn)?;
        if existed && source_version < latest_version() {
            backup_before_migration(&conn, &paths, version)?;
        }
        migrate(&mut conn)?;
        if source_version < latest_version() {
            // A schema upgrade may add derived indexes (search, mentions) for existing data.
            crate::hooks::rebuild_all(&conn)?;
        }
        // D-72: a database from before this run that records no knowledge mode is internal (an
        // older version made it); one made just now leaves it unset, so the UI asks.
        if existed {
            crate::settings::default_mode_to_internal(&conn)?;
        }
        tracing::info!(from = version, to = latest_version(), "database ready");
        Ok(Db { conn, paths })
    }

    /// Convenience for tests and tools: open a database in `dir`.
    pub fn open_at(dir: &Path) -> Result<Db> {
        Db::open(DataPaths::new(dir))
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn paths(&self) -> &DataPaths {
        &self.paths
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<u32> {
        user_version(&self.conn)
    }

    /// Runs `f` in a transaction: committed if it returns `Ok`, rolled back otherwise. External
    /// mode's file changes made inside it are journaled, so a rollback has the watcher read those
    /// files again, and work that waits for the commit runs after it (`knowledge::external::journal`).
    pub fn tx<T>(&mut self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let journal = crate::knowledge::external::journal::open();
        let tx = self.conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        journal.committed(&self.conn);
        Ok(out)
    }
}

impl Db {
    /// Replaces the current database file and attachments folder with a prepared database
    /// (`new_db`) and attachments folder (`new_attachments`), both inside the data folder so the
    /// renames stay on one file system (FR-DATA-3). Used by import.
    ///
    /// The old files are set aside first. If anything fails, they are put back and the old
    /// database is reopened, so the current data is never lost.
    pub fn replace_from(&mut self, new_db: &Path, new_attachments: &Path) -> Result<()> {
        let db_file = self.paths.db_file();
        let attachments = self.paths.attachments_dir();
        let live = import_live_files(&self.paths);

        // Close the connection (this also folds the WAL into the main file).
        let placeholder = Connection::open_in_memory()?;
        let old = std::mem::replace(&mut self.conn, placeholder);
        if let Err((conn, e)) = old.close() {
            self.conn = conn;
            return Err(e.into());
        }

        let swapped = (|| -> Result<Connection> {
            for p in &live {
                if p.exists() {
                    let stash = pre_import_stash(p);
                    remove_any(&stash)?;
                    std::fs::rename(p, stash)?;
                }
            }
            std::fs::rename(new_db, &db_file)?;
            std::fs::rename(new_attachments, &attachments)?;
            let mut conn = Connection::open(&db_file)?;
            configure(&conn)?;
            migrate(&mut conn)?;
            Ok(conn)
        })();

        match swapped {
            Ok(conn) => {
                self.conn = conn;
                // NFR-REL-5: the swap is finished. From here on, the stashes are only clean-up, and
                // a start-up that finds them must not undo the import (startup.rs).
                let marker = import_swapped_marker(&self.paths);
                if let Err(e) = std::fs::File::create(&marker).and_then(|f| f.sync_all()) {
                    tracing::warn!(error = %e, "could not write the import marker");
                }
                for p in &live {
                    if let Err(e) = remove_any(&pre_import_stash(p)) {
                        tracing::warn!(error = %e, "could not remove a pre-import leftover");
                    }
                }
                let _ = std::fs::remove_file(&marker);
                Ok(())
            }
            Err(e) => {
                tracing::error!(error = %e, "swapping in the imported data failed, restoring");
                for p in &live {
                    let stash = pre_import_stash(p);
                    if stash.exists() {
                        let _ = remove_any(p);
                        if let Err(e) = std::fs::rename(&stash, p) {
                            tracing::error!(error = %e, file = %p.display(), "could not restore");
                        }
                    }
                }
                if let Err(e2) = Connection::open(&db_file)
                    .map_err(crate::CoreError::from)
                    .and_then(|c| configure(&c).map(|()| c))
                    .map(|c| self.conn = c)
                {
                    tracing::error!(error = %e2, "could not reopen the old database");
                }
                Err(e)
            }
        }
    }
}

/// The files [`Db::replace_from`] sets aside: the database, its WAL and shared-memory files, and
/// the attachments folder.
pub(crate) fn import_live_files(paths: &DataPaths) -> [PathBuf; 4] {
    let db_file = paths.db_file();
    [
        db_file.clone(),
        with_suffix(&db_file, "-wal"),
        with_suffix(&db_file, "-shm"),
        paths.attachments_dir(),
    ]
}

/// Where [`Db::replace_from`] sets `live` aside while an import swaps the data (NFR-REL-5).
pub(crate) fn pre_import_stash(live: &Path) -> PathBuf {
    with_suffix(live, ".pre-import")
}

/// Written once the imported data is fully in place and removed after the stashes are deleted:
/// tells a start-up that finds stashes whether the import finished (NFR-REL-5).
pub(crate) fn import_swapped_marker(paths: &DataPaths) -> PathBuf {
    paths.root.join("import-swapped.marker")
}

/// `path` with `suffix` appended to the file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Removes a file or folder if it exists.
pub(crate) fn remove_any(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) => Ok(()),
    }
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL: crash-safe and readers don't block the writer. foreign_keys is off by default in SQLite.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

fn user_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

fn backup_before_migration(conn: &Connection, paths: &DataPaths, version: u32) -> Result<()> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let file = paths
        .backups_dir()
        .join(format!("pre-migration-v{version}-{stamp}.sqlite"));
    conn.execute("VACUUM INTO ?1", [file.to_string_lossy()])?;
    tracing::info!(file = %file.display(), "database backed up before migration");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_database_is_migrated_with_general_category() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        assert_eq!(db.schema_version().unwrap(), latest_version());
        let general: String = db
            .conn()
            .query_row("SELECT name FROM categories WHERE is_general = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(general, "General");
        let mode: String = db
            .conn()
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn reopening_keeps_data_and_makes_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        drop(Db::open_at(dir.path()).unwrap());
        let db = Db::open_at(dir.path()).unwrap();
        assert_eq!(db.schema_version().unwrap(), latest_version());
        assert_eq!(
            std::fs::read_dir(db.paths().backups_dir()).unwrap().count(),
            0
        );
    }

    /// A database from before the search index existed gets its tasks indexed on upgrade.
    #[test]
    fn upgrade_rebuilds_derived_indexes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        paths.ensure().unwrap();
        {
            // Schema v2 (tasks, no search index) with one task.
            let mut conn = Connection::open(paths.db_file()).unwrap();
            crate::migrations::migrations().to_version(&mut conn, 2).unwrap();
            conn.execute_batch(
                "INSERT INTO tasks (id, key, number, category_id, title, created_at, updated_at)
                 VALUES ('t1', 'GEN-1', 1, 'p-general', 'Calibrate the pressure sensor', 'x', 'x')",
            )
            .unwrap();
        }
        let db = Db::open(paths).unwrap();
        let hits = crate::search::search(
            db.conn(),
            &crate::search::SearchQuery {
                text: "pressure".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        // The vector pieces are cut too (step 4), waiting for the indexer.
        let (n, ready): (i64, i64) = db
            .conn()
            .query_row("SELECT COUNT(*), COUNT(vector) FROM chunks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, ready), (1, 0));
    }

    /// A schema-v3 database (before the vector index) with a page and a task gets pieces for both on upgrade.
    #[test]
    fn upgrade_from_v3_cuts_pieces() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        paths.ensure().unwrap();
        {
            let mut conn = Connection::open(paths.db_file()).unwrap();
            crate::migrations::migrations().to_version(&mut conn, 3).unwrap();
            conn.execute_batch(
                "INSERT INTO tasks (id, key, number, category_id, title, created_at, updated_at)
                 VALUES ('t1', 'GEN-1', 1, 'p-general', 'Calibrate the sensor', 'x', 'x');
                 INSERT INTO task_comments (id, task_id, text, created_at)
                 VALUES ('c1', 't1', 'Done on site', 'x');
                 INSERT INTO pages (id, category_id, title, body, created_at, updated_at)
                 VALUES ('pg1', 'p-general', 'Hose guide', '# Pre-start\n\nCheck the hoses.', 'x', 'x'),
                        ('pg2', 'p-general', 'Empty page', '', 'x', 'x');",
            )
            .unwrap();
        }
        let db = Db::open(paths).unwrap();
        let st = crate::vectors::status(db.conn()).unwrap();
        assert_eq!((st.items, st.chunks, st.chunks_ready), (3, 4, 0));
        let heading: String = db
            .conn()
            .query_row("SELECT heading FROM chunks WHERE ref_id = 'pg1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(heading, "Pre-start");
    }

    /// D-69: step 9 rebuilds `providers` for its new CHECK; the models must survive it, still
    /// tied to their provider.
    #[test]
    fn the_deepseek_step_keeps_providers_and_models() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        paths.ensure().unwrap();
        {
            let mut conn = Connection::open(paths.db_file()).unwrap();
            crate::migrations::migrations().to_version(&mut conn, 8).unwrap();
            conn.execute_batch(
                "INSERT INTO providers (id, name, kind, endpoint, auth, created_at, updated_at)
                 VALUES ('pv1', 'Work', 'azure_foundry', 'https://x/openai/v1', 'azure_cli', 'x', 'x');
                 INSERT INTO models (id, provider_id, name, label, format, web_search, created_at, updated_at)
                 VALUES ('m1', 'pv1', 'gpt', 'GPT', 'responses', 1, 'x', 'x'),
                        ('m2', 'pv1', 'ds', 'DeepSeek', 'chat_completions', 0, 'x', 'x');",
            )
            .unwrap();
        }
        let db = Db::open(paths).unwrap();
        let n = |sql: &str| -> i64 { db.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(n("SELECT count(*) FROM providers"), 1);
        assert_eq!(n("SELECT count(*) FROM models WHERE provider_id = 'pv1'"), 2);
        assert_eq!(n("SELECT web_search FROM models WHERE id = 'm1'"), 1);
        db.conn()
            .execute_batch(
                "INSERT INTO providers (id, name, kind, auth, created_at, updated_at)
                 VALUES ('pv2', 'DeepSeek', 'deepseek', 'api_key', 'x', 'x')",
            )
            .unwrap();
        // The foreign key still holds: deleting the provider deletes its models.
        db.conn().execute("DELETE FROM providers WHERE id = 'pv1'", []).unwrap();
        assert_eq!(n("SELECT count(*) FROM models"), 0);
    }

    /// The rows of a knowledge database made at schema version 9 (before D-72).
    const V9_KNOWLEDGE: &str =
        "INSERT INTO categories (id, name, key, created_at, updated_at) VALUES ('c1', 'Ops', 'OPS', 'x', 'x');
         INSERT INTO tasks (id, key, number, category_id, title, created_at, updated_at)
         VALUES ('t1', 'OPS-1', 1, 'c1', 'Fix the pump', 'x', 'x');
         INSERT INTO groups (id, category_id, name, description, created_at, updated_at)
         VALUES ('g1', 'c1', 'Pumps', 'Pump service', 'x', 'x');
         INSERT INTO pages (id, category_id, group_id, title, body, source_info, origin, origin_task_id,
                            check_status, archived, created_at, updated_at)
         VALUES ('pg1', 'c1', 'g1', 'Pump check', 'See [Hoses](page:pg2) and #OPS-1.', 'Manual', 'task', 't1',
                 'checked', 0, 'a', 'b'),
                ('pg2', 'c1', NULL, 'Hoses', 'Hose text', NULL, 'manual', NULL, 'not_checked', 1, 'c', 'd');
         INSERT INTO page_tags (page_id, tag) VALUES ('pg1', 'pumps'), ('pg1', 'safety');
         INSERT INTO page_versions (page_id, day, snapshot) VALUES ('pg1', '2026-10-01', '{}');
         INSERT INTO task_page_links (id, task_id, page_id, created_at) VALUES ('l1', 't1', 'pg1', 'x');";

    #[test]
    fn released_090_database_is_upgraded_without_losing_data() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        paths.ensure().unwrap();
        {
            let c = Connection::open(paths.db_file()).unwrap();
            c.execute_batch(include_str!("../tests/fixtures/legacy-0.9.0.sql")).unwrap();
            c.execute_batch(V9_KNOWLEDGE).unwrap();
            crate::migrations::tests::fill_legacy_conversation(&c);
        }
        let db = Db::open(paths).unwrap();
        crate::migrations::tests::assert_legacy_conversation(db.conn());
        let page = crate::knowledge::get_page(db.conn(), "pg1").unwrap();
        assert_eq!(page.tags, ["pumps", "safety"]);
        assert_eq!(page.origin_task_id.as_deref(), Some("t1"));
        assert_eq!(page.group_id.as_deref(), Some("g1"));
        assert_eq!(db.conn().query_row("SELECT count(*) FROM task_page_links", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(db.conn().query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(db.schema_version().unwrap(), latest_version());
        let backups: Vec<_> = std::fs::read_dir(db.paths().backups_dir()).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(user_version(&backup).unwrap(), 9);
        assert!(backup.prepare("SELECT model_id FROM conversations").is_err());
        drop(db);
        let reopened = Db::open_at(dir.path()).unwrap();
        crate::migrations::tests::assert_legacy_conversation(reopened.conn());
        assert_eq!(std::fs::read_dir(reopened.paths().backups_dir()).unwrap().count(), 1);
    }

    /// D-72: step 10 rebuilds `pages` and `groups` with foreign keys off; every row made at
    /// version 9 survives with its tags, history, task links and group, the foreign keys hold
    /// afterwards, and the uniqueness rules moved into partial indexes.
    #[test]
    fn the_external_knowledge_step_keeps_pages_and_groups() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataPaths::new(dir.path());
        paths.ensure().unwrap();
        {
            let mut conn = Connection::open(paths.db_file()).unwrap();
            conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
            crate::migrations::migrations().to_version(&mut conn, 9).unwrap();
            conn.execute_batch(V9_KNOWLEDGE).unwrap();
        }
        let db = Db::open(paths).unwrap();
        assert_eq!(db.schema_version().unwrap(), 10);
        assert_eq!(std::fs::read_dir(db.paths().backups_dir()).unwrap().count(), 1);
        let c = db.conn();
        let n = |sql: &str| -> i64 { c.query_row(sql, [], |r| r.get(0)).unwrap() };
        let fk_on: bool = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert!(fk_on, "foreign keys are on again after the step");
        assert_eq!(n("SELECT COUNT(*) FROM pragma_foreign_key_check"), 0);
        let page = crate::knowledge::get_page(c, "pg1").unwrap();
        assert_eq!(
            (page.group_id.as_deref(), page.origin_task_id.as_deref(), page.source_info.as_deref()),
            (Some("g1"), Some("t1"), Some("Manual"))
        );
        assert_eq!((page.created_at.as_str(), page.updated_at.as_str()), ("a", "b"));
        assert_eq!((page.path, page.file_hash), (None, None));
        assert_eq!(page.tags, ["pumps", "safety"]);
        assert!(crate::knowledge::get_page(c, "pg2").unwrap().archived);
        assert_eq!(n("SELECT COUNT(*) FROM page_versions"), 1);
        assert_eq!(n("SELECT COUNT(*) FROM task_page_links"), 1);
        let g = &crate::knowledge::groups(c, None).unwrap()[0];
        assert_eq!((g.name.as_str(), g.description.as_str(), g.parent_id.as_deref(), g.path.as_deref()), ("Pumps", "Pump service", None, None));
        // The derived indexes were rebuilt on upgrade, and the mode is internal.
        assert_eq!(n("SELECT COUNT(*) FROM mentions WHERE source_id = 'pg1'"), 2);
        assert_eq!(
            crate::settings::knowledge_mode(c).unwrap().0,
            Some(crate::settings::KnowledgeMode::Internal)
        );

        // Internal rows: titles and group names unique within a category, ignoring case.
        let page_row = |id: &str, title: &str, path: Option<&str>| {
            c.execute(
                "INSERT INTO pages (id, category_id, title, created_at, updated_at, path) VALUES (?1, 'c1', ?2, 'x', 'x', ?3)",
                rusqlite::params![id, title, path],
            )
        };
        assert!(page_row("pg3", "PUMP CHECK", None).is_err());
        // External rows: unique by path only, exactly as written.
        page_row("pg4", "Pump check", Some("Ops/Pump check.md")).unwrap();
        page_row("pg5", "Pump check", Some("Ops/Sub/Pump check.md")).unwrap();
        page_row("pg6", "pump check", Some("Ops/pump check.md")).unwrap();
        assert!(page_row("pg7", "Other", Some("Ops/Pump check.md")).is_err());
        let group_row = |id: &str, name: &str, path: Option<&str>| {
            c.execute(
                "INSERT INTO groups (id, category_id, name, created_at, updated_at, path) VALUES (?1, 'c1', ?2, 'x', 'x', ?3)",
                rusqlite::params![id, name, path],
            )
        };
        assert!(group_row("g2", "pumps", None).is_err());
        group_row("g3", "Pumps", Some("Ops/Pumps")).unwrap();
        assert!(group_row("g4", "x", Some("Ops/Pumps")).is_err());
        // The foreign keys act again: a deleted page takes its tags, a deleted group ungroups.
        c.execute("DELETE FROM pages WHERE id = 'pg1'", []).unwrap();
        assert_eq!(n("SELECT COUNT(*) FROM page_tags"), 0);
        page_row("pg8", "In group", None).unwrap();
        c.execute("UPDATE pages SET group_id = 'g1' WHERE id = 'pg8'", []).unwrap();
        c.execute("DELETE FROM groups WHERE id = 'g1'", []).unwrap();
        assert_eq!(n("SELECT COUNT(*) FROM pages WHERE group_id IS NOT NULL"), 0);
    }

    #[test]
    fn outdated_database_is_backed_up_before_migrating() {
        let dir = tempfile::tempdir().unwrap();
        // An empty database file at version 0 stands in for an older release.
        let paths = DataPaths::new(dir.path());
        paths.ensure().unwrap();
        Connection::open(paths.db_file())
            .unwrap()
            .execute_batch("CREATE TABLE old (x)")
            .unwrap();
        let db = Db::open(paths).unwrap();
        let backups: Vec<_> = std::fs::read_dir(db.paths().backups_dir())
            .unwrap()
            .collect();
        assert_eq!(backups.len(), 1);
    }
}
