//! Database schema (NFR-REL-3, NFR-REL-6). The version is kept in SQLite's `user_version`: step N
//! brings a database to version N. Versions keep counting across folds, so a version always means
//! one layout. Three sets of SQL (D-86):
//!
//! - `migrations/0.x/`: Cairn 0.x's steps 1 to 10, frozen. They bring data made by 0.x up to
//!   version 10, 0.x's last layout, and are never changed.
//! - `migrations/1.0/`: the schema at 1.0, one file per area, each creating its tables in their
//!   current form. A new database is made from these ([`create_at`]) and is then at version 10
//!   ([`BASELINE`]); `the_schema_at_1_0_equals_0x_steps` proves they make what 0.x's steps make.
//! - [`STEPS`] (`migrations/NNNN_name.sql`): the steps after 1.0, from version 11. Later changes
//!   are new steps appended there; never edit a released step. Steps are folded only when the
//!   major version changes, and the folded-away ones are kept frozen like 0.x's.
//!
//! The upgrade steps of Cairn 0.10.1 and earlier were removed (D-66). Released 0.9 databases reused
//! version 9; their verified layout is equivalent to step 6 and is bridged explicitly before upgrading.
//!
//! **Steps that rebuild a table** other tables refer to ([`REBUILD_STEPS`]) run with foreign keys
//! off, as SQLite's procedure for such changes requires: with them on, dropping the old table would
//! delete or clear every row that refers to it. `PRAGMA foreign_keys` can't change inside a
//! transaction and each step runs in one, so [`migrate_to`] runs the steps in runs: the ordinary
//! steps together with foreign keys on, each rebuild step on its own with them off and
//! `PRAGMA foreign_key_check` before it commits (`M::foreign_key_check`). The connection's own
//! setting is put back afterwards.

use std::sync::LazyLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::Result;

/// Cairn 0.x's steps, versions 1 to 10, frozen (D-86): only for data made by 0.x.
const STEPS_0X: &[&str] = &[
    // Categories (with the built-in General category) and settings.
    include_str!("../migrations/0.x/0001_foundation.sql"),
    // Tasks, labels, key aliases, links, comments, daily history, activity.
    include_str!("../migrations/0.x/0002_tasks.sql"),
    // Groups, pages, tags, page history, task-page links, mentions, attachments, drafts, search index.
    include_str!("../migrations/0.x/0003_knowledge.sql"),
    // AI providers and models, the vector index (chunks with float32 vectors).
    include_str!("../migrations/0.x/0004_ai.sql"),
    // The glossary and its "used in" index.
    include_str!("../migrations/0.x/0005_glossary.sql"),
    // Saved conversations, text from uploaded files, ignored glossary terms.
    include_str!("../migrations/0.x/0006_conversations.sql"),
    // D-67: the model and effort picked in a conversation.
    include_str!("../migrations/0.x/0007_conversation_model.sql"),
    // D-68: archived conversations.
    include_str!("../migrations/0.x/0008_conversation_archive.sql"),
    // D-69: DeepSeek as a provider kind (providers rebuilt for its CHECK; models kept).
    include_str!("../migrations/0.x/0009_deepseek_provider.sql"),
    // D-72: external knowledge mode (pages and groups rebuilt: paths, nested groups, file stamps).
    include_str!("../migrations/0.x/0010_external_knowledge.sql"),
];

/// The schema at 1.0, one file per area, in this order: what a new database is made from (D-86).
const AREAS_1_0: &[&str] = &[
    // Categories (with the built-in General category) and settings.
    include_str!("../migrations/1.0/1_foundation.sql"),
    // Tasks, labels, key aliases, links, comments, daily history, activity.
    include_str!("../migrations/1.0/2_tasks.sql"),
    // Groups, pages, tags, page history, task-page links, mentions, attachments, drafts, search
    // index, removed files of external mode.
    include_str!("../migrations/1.0/3_knowledge.sql"),
    // AI providers and models, the vector index (chunks with float32 vectors).
    include_str!("../migrations/1.0/4_ai.sql"),
    // The glossary and its "used in" index.
    include_str!("../migrations/1.0/5_glossary.sql"),
    // Saved conversations, text from uploaded files, ignored glossary terms.
    include_str!("../migrations/1.0/6_conversations.sql"),
];

/// The version [`AREAS_1_0`] make: the last of 0.x, whose layout they equal.
const BASELINE: usize = 10;
const _: () = assert!(STEPS_0X.len() == BASELINE);

/// The steps after 1.0, from version 11 (`migrations/NNNN_name.sql`), oldest first. Append new steps
/// at the end.
const STEPS: &[&str] = &[];

/// The steps (numbered from 1, as `user_version` counts them) that rebuild tables and must run with
/// foreign keys off. See the module comment.
const REBUILD_STEPS: &[usize] = &[10];

/// The SQL of every step, oldest first: 0.x's, then those after 1.0 (tests build longer lists from it).
pub(crate) fn steps() -> &'static [&'static str] {
    static ALL: LazyLock<Vec<&'static str>> =
        LazyLock::new(|| STEPS_0X.iter().chain(STEPS).copied().collect());
    &ALL
}

pub fn migrations() -> Migrations<'static> {
    Migrations::new(
        steps()
            .iter()
            .enumerate()
            .map(|(i, sql)| {
                if REBUILD_STEPS.contains(&(i + 1)) {
                    M::up(sql).foreign_key_check()
                } else {
                    M::up(sql)
                }
            })
            .collect(),
    )
}

/// Brings `conn` to the latest version: a new database with [`create_at`], existing data with
/// [`migrate_to`].
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let from = source_version(conn)?;
    if from == 0 {
        return create_at(conn, &migrations(), latest_version() as usize);
    }
    // Db::open makes the backup before this bridge changes the recorded version.
    let actual: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if from != actual {
        conn.pragma_update(None, "user_version", from)?;
    }
    migrate_to(conn, &migrations(), latest_version() as usize)
}

/// Resolve reused version numbers by structure, without writing to the source database.
/// Unrecognized old layouts and databases from newer applications must not be upgraded blindly.
pub(crate) fn source_version(conn: &Connection) -> Result<u32> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > latest_version() {
        return Err(crate::CoreError::Invalid(format!(
            "This database uses schema {version}, newer than this Cairn supports ({}). Open it with a newer Cairn.", latest_version()
        )));
    }
    let has = |table: &str, column: &str| -> Result<bool> {
        Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
            [table, column], |r| r.get(0))?)
    };
    if version == 9 && !has("conversations", "model_id")? {
        return match is_released_09(&schema_columns(conn)?, schema_columns)? {
            true => Ok(6),
            false => Err(unsupported_layout()),
        };
    }
    if (version >= 1 && !has("categories", "is_general")?)
        || (version >= 7 && (!has("conversations", "model_id")? || !has("conversations", "effort")?))
        || (version >= 8 && !has("conversations", "archived_at")?)
        || (version >= 10 && !has("pages", "path")?)
    {
        return Err(unsupported_layout());
    }
    Ok(version)
}

/// Released 0.9 databases reused version 9 for step 6's layout (D-69's version 9 added
/// `conversations.model_id`). Whether a version-9 source without that column has step 6's
/// layout, for databases and exports alike: `layout` describes a database the way `source` was
/// described, and is applied to a new database at step 6.
pub(crate) fn is_released_09<L: PartialEq>(source: &L, layout: impl FnOnce(&Connection) -> Result<L>) -> Result<bool> {
    let mut baseline = Connection::open_in_memory()?;
    migrate_to(&mut baseline, &migrations(), 6)?;
    Ok(layout(&baseline)? == *source)
}

fn unsupported_layout() -> crate::CoreError {
    crate::CoreError::Invalid("This database has an unrecognized or incomplete schema. Keep the original and restore a compatible backup; Cairn has not migrated it.".into())
}

type SchemaColumns = std::collections::BTreeMap<String, Vec<(String, String, bool, Option<String>, u32)>>;

/// Include every table and each column's type/default/constraints; column order changed in D-66.
fn schema_columns(conn: &Connection) -> Result<SchemaColumns> {
    let names = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?
        .query_map([], |r| r.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
    let mut result = SchemaColumns::new();
    for name in names {
        let cols = conn.prepare("SELECT name, type, [notnull], dflt_value, pk FROM pragma_table_info(?1) ORDER BY name")?
            .query_map([&name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        result.insert(name, cols);
    }
    Ok(result)
}

/// Runs the steps of `migs` from the database's version up to `target` (never down): ordinary
/// steps with foreign keys on, rebuild steps with them off (see the module comment).
pub(crate) fn migrate_to(conn: &mut Connection, migs: &Migrations<'_>, target: usize) -> Result<()> {
    let was_on: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    let result = (|| -> Result<()> {
        let mut version = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? as usize;
        if version > target {
            return Err(crate::CoreError::Invalid(format!("Cannot migrate schema {version} down to {target}.")));
        }
        while version < target {
            let rebuild = REBUILD_STEPS.contains(&(version + 1));
            // Ordinary steps run together up to the next rebuild step.
            let end = if rebuild {
                version + 1
            } else {
                REBUILD_STEPS
                    .iter()
                    .filter(|s| **s > version + 1)
                    .map(|s| s - 1)
                    .min()
                    .unwrap_or(target)
                    .min(target)
            };
            conn.pragma_update(None, "foreign_keys", !rebuild)?;
            migs.to_version(conn, end)?;
            version = end;
        }
        Ok(())
    })();
    conn.pragma_update(None, "foreign_keys", was_on)?;
    result
}

/// Makes version `target` in an empty database (D-86): from the schema at 1.0 and the steps after it
/// when `target` is 1.0's version or later, else from 0.x's steps (the import of an older export
/// builds the export's own version first).
pub(crate) fn create_at(conn: &mut Connection, migs: &Migrations<'_>, target: usize) -> Result<()> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version != 0 {
        return Err(crate::CoreError::Invalid(format!(
            "Cannot make a new schema over version {version}."
        )));
    }
    if target >= BASELINE {
        let tx = conn.transaction()?;
        for sql in AREAS_1_0 {
            tx.execute_batch(sql)?;
        }
        tx.pragma_update(None, "user_version", BASELINE as u32)?;
        tx.commit()?;
    }
    migrate_to(conn, migs, target)
}

/// Schema version the code expects (= number of steps, 0.x's included).
pub fn latest_version() -> u32 {
    steps().len() as u32
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }
    pub(crate) fn fill_legacy_conversation(c: &Connection) {
        c.execute_batch("INSERT INTO providers (id, name, kind, auth, created_at, updated_at)
            VALUES ('pv1', 'Local', 'local', 'none', 'x', 'x');
            INSERT INTO models (id, provider_id, name, label, format, created_at, updated_at)
            VALUES ('m1', 'pv1', 'model', 'Model', 'chat_completions', 'x', 'x');
            INSERT INTO conversations (id, title, created_at, updated_at) VALUES ('cv1', 'Learning', 'x', 'x');
            INSERT INTO messages (id, conversation_id, seq, role, text, model_id, effort, created_at)
            VALUES ('msg1', 'cv1', 1, 'assistant', 'Keep this answer', 'm1', 'high', 'x');").unwrap();
    }

    pub(crate) fn assert_legacy_conversation(c: &Connection) {
        let saved: (Option<String>, Option<String>, Option<String>) = c.query_row(
            "SELECT model_id, effort, archived_at FROM conversations WHERE id = 'cv1'", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(saved, (Some("m1".into()), Some("high".into()), None));
        assert_eq!(c.query_row("SELECT text FROM messages WHERE id = 'msg1'", [], |r| r.get::<_, String>(0)).unwrap(), "Keep this answer");
        assert_eq!(c.query_row("SELECT count(*) FROM models WHERE provider_id = 'pv1'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        c.execute_batch("INSERT OR IGNORE INTO providers (id, name, kind, auth, created_at, updated_at)
            VALUES ('ds', 'DeepSeek', 'deepseek', 'api_key', 'x', 'x');").unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM providers WHERE kind = 'deepseek'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn rejects_unrecognized_legacy_layout_before_migrating() {
        let mut c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../tests/fixtures/legacy-0.9.0.sql")).unwrap();
        c.execute_batch("ALTER TABLE conversations DROP COLUMN history_budget").unwrap();
        assert!(migrate(&mut c).is_err());
        assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap(), 9);
        assert!(c.prepare("SELECT path FROM pages").is_err());
    }

    /// Every object's SQL without comments, quotes around names and layout, by type and name, so the
    /// same definition written two ways (a step's `ALTER TABLE`, a table renamed) compares equal.
    fn schema_sql(c: &Connection) -> Vec<(String, String, Option<String>)> {
        let normal = |sql: String| {
            let words: Vec<String> = sql
                .lines()
                .map(|l| l.split("--").next().unwrap_or("").replace('"', ""))
                .collect();
            let mut text = words
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            for p in ["(", ")", ","] {
                text = text
                    .replace(&format!(" {p}"), p)
                    .replace(&format!("{p} "), p);
            }
            text
        };
        c.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get::<_, Option<String>>(2)?.map(normal),
                ))
            })
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    /// Every table's rows, sorted, with the times they were made left out.
    fn all_rows(c: &Connection) -> Vec<(String, Vec<String>)> {
        let tables: Vec<String> = c
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND sql NOT LIKE 'CREATE VIRTUAL%' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let mut out = Vec::new();
        for t in tables {
            let mut stmt = c.prepare(&format!("SELECT * FROM \"{t}\"")).unwrap();
            let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
            let mut rows: Vec<String> = stmt
                .query_map([], |r| {
                    Ok((0..names.len())
                        .map(|i| {
                            if names[i].ends_with("_at") {
                                "<time>".into()
                            } else {
                                format!("{:?}", r.get_ref(i).unwrap())
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", "))
                })
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            rows.sort();
            out.push((t, rows));
        }
        out
    }

    /// D-86: the schema at 1.0 makes exactly what 0.x's steps make: every table, column,
    /// constraint, index and row (the General category), at the same version.
    #[test]
    fn the_schema_at_1_0_equals_0x_steps() {
        let mut stepped = Connection::open_in_memory().unwrap();
        migrate_to(&mut stepped, &migrations(), BASELINE).unwrap();
        let mut folded = Connection::open_in_memory().unwrap();
        create_at(&mut folded, &migrations(), BASELINE).unwrap();
        assert_eq!(schema_sql(&folded), schema_sql(&stepped));
        assert_eq!(all_rows(&folded), all_rows(&stepped));
        for c in [&folded, &stepped] {
            assert_eq!(
                c.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                    .unwrap(),
                BASELINE as u32
            );
            assert_eq!(
                c.query_row("PRAGMA foreign_key_check", [], |_| Ok(())).ok(),
                None
            );
        }
    }

    /// D-86: a new database is made from the schema at 1.0, not from 0.x's steps (which leave
    /// `pages` renamed, as `"pages"`), and data from 0.x still takes 0.x's steps.
    #[test]
    fn a_new_database_is_made_from_the_schema_at_1_0() {
        let pages_sql = |c: &Connection| -> String {
            c.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'pages'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        let mut new = Connection::open_in_memory().unwrap();
        migrate(&mut new).unwrap();
        assert!(
            pages_sql(&new).starts_with("CREATE TABLE pages ("),
            "{}",
            pages_sql(&new)
        );

        let mut old = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut old, 9).unwrap();
        migrate(&mut old).unwrap();
        assert!(
            pages_sql(&old).starts_with("CREATE TABLE \"pages\""),
            "{}",
            pages_sql(&old)
        );
        assert_eq!(
            old.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            latest_version()
        );
    }

    #[test]
    fn create_at_refuses_a_database_with_a_schema() {
        let mut c = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut c, 2).unwrap();
        assert!(create_at(&mut c, &migrations(), BASELINE).is_err());
    }

    #[test]
    fn rejects_newer_database() {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "user_version", latest_version() + 1).unwrap();
        assert!(migrate(&mut c).is_err());
    }

}
