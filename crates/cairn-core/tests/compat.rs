//! Compatibility (NFR-REL-6): data folders and exports made by earlier versions open and import,
//! with their data intact. The fixtures, one per schema version from 6 on, and one in external
//! knowledge mode, were made by `examples/compat_fixture.rs` at each version's own commit
//! (`tests/fixtures/compat/README.md`).

use std::path::{Path, PathBuf};

use cairn_core::knowledge::external::scan::{self, Scope};
use cairn_core::settings::{self, KnowledgeMode};
use cairn_core::tasks::{self, TaskStatus};
use cairn_core::{DataPaths, Db, export, migrations};
use rusqlite::Connection;

/// (folder, schema version): every schema version from 6 on, from the last release that used it.
const FIXTURES: &[(&str, u32)] = &[
    ("0.10.2", 6),
    ("0.10.3", 7),
    ("0.11.0", 8),
    ("0.12.1", 9),
    ("0.17.0", 10),
];

/// (folder, schema version) of the fixtures in external mode (D-72, from schema 10), which also
/// hold their knowledge folder (`notes/`).
const EXTERNAL: &[(&str, u32)] = &[("0.17.0-external", 10)];

/// Every fixture, and whether it is in external mode.
fn all() -> impl Iterator<Item = (&'static str, u32, bool)> {
    let internal = FIXTURES.iter().map(|&(name, schema)| (name, schema, false));
    internal.chain(EXTERNAL.iter().map(|&(name, schema)| (name, schema, true)))
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/compat")
        .join(name)
}

/// The fixtures are never opened in place: SQLite could write next to them.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// SQLite keeps `user_version` in the file header, bytes 60 to 63.
fn file_schema_version(db_file: &Path) -> u32 {
    let header = std::fs::read(db_file).unwrap();
    u32::from_be_bytes(header[60..64].try_into().unwrap())
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// An external-mode fixture records the folder it was made in. Its copy is elsewhere, so the copied
/// database is pointed at the copy, as for a folder that stayed where it was.
fn point_at(db_file: &Path, notes: &Path) {
    let conn = Connection::open(db_file).unwrap();
    let root = serde_json::to_string(notes.to_str().unwrap()).unwrap();
    let changed = conn
        .execute(
            "UPDATE settings SET value = ?1 WHERE key = 'externalRoot'",
            [root],
        )
        .unwrap();
    assert_eq!(changed, 1);
}

/// The rows `compat_fixture.rs` made, as the current version reads them; `notes` is the knowledge
/// folder of an external-mode fixture.
fn check(db: &mut Db, name: &str, schema: u32, notes: Option<&Path>) {
    let conn = db.conn();
    let context = format!("fixture {name}");
    assert_eq!(
        db.schema_version().unwrap(),
        migrations::latest_version(),
        "{context}"
    );
    let ok: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok", "{context}");
    assert_eq!(
        count(conn, "SELECT count(*) FROM pragma_foreign_key_check"),
        0,
        "{context}"
    );

    let s = settings::get(conn).unwrap();
    assert_eq!(s.user_name, "Fixture", "{context}");
    let mode = match notes {
        None => KnowledgeMode::Internal,
        Some(_) => KnowledgeMode::External,
    };
    assert_eq!(s.knowledge_mode, Some(mode), "{context}");

    // Tasks: labels, due date, the change, the comment, both links, and the key left by a move.
    let task = tasks::get(conn, "CMP-1").unwrap();
    assert_eq!(task.title, "Fixture task", "{context}");
    assert_eq!(task.status, TaskStatus::InProgress, "{context}");
    assert!(task.labels.contains(&"数据".to_string()), "{context}");
    assert_eq!(task.due_date.as_deref(), Some("2026-12-01"), "{context}");
    assert_eq!(tasks::links(conn, &task.id).unwrap().len(), 2, "{context}");
    assert!(
        !tasks::versions(conn, &task.id).unwrap().is_empty(),
        "{context}"
    );
    let comments = tasks::comments(conn, &task.id).unwrap();
    assert!(
        comments.len() == 1 && comments[0].text.contains("第一条评论"),
        "{context}"
    );
    assert_eq!(
        tasks::get(conn, "OLD-1").unwrap().title,
        "Moved task",
        "{context}"
    );

    // Pages: the changed body, tags, group, history, the task link, the image and the mentions.
    let (page_id, body): (String, String) = conn
        .query_row(
            "SELECT id, body FROM pages WHERE title = 'Fixture page'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(
        body.contains(", changed") && body.contains("中文段落"),
        "{context}"
    );
    let page = cairn_core::knowledge::get_page(conn, &page_id).unwrap();
    assert!(page.tags.contains(&"标签".to_string()), "{context}");
    assert!(page.group_id.is_some(), "{context}");
    assert!(
        !cairn_core::knowledge::versions(conn, &page_id)
            .unwrap()
            .is_empty(),
        "{context}"
    );
    assert_eq!(
        count(conn, "SELECT count(*) FROM task_page_links"),
        1,
        "{context}"
    );
    // External: the nested page and the page written outside Cairn add three.
    let mentions = if notes.is_some() { 6 } else { 3 };
    assert_eq!(
        count(conn, "SELECT count(*) FROM mentions"),
        mentions,
        "{context}"
    );
    // The pasted image: in the data folder, or (external) in the category's `attachments` folder.
    let target = |prefix: &str| {
        body.split(prefix)
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap()
    };
    let image = match notes {
        None => db.paths().attachments_dir().join(target("attachment:")),
        Some(notes) => notes.join(target("folder:")),
    };
    assert!(image.is_file(), "{context}: {}", image.display());

    // Glossary, providers and conversations.
    assert_eq!(
        count(
            conn,
            "SELECT count(*) FROM glossary_entries WHERE term = 'pipeline'"
        ),
        1,
        "{context}"
    );
    assert_eq!(
        count(conn, "SELECT count(*) FROM glossary_ignored"),
        1,
        "{context}"
    );
    assert_eq!(
        count(
            conn,
            "SELECT count(*) FROM providers WHERE kind = 'deepseek'"
        ),
        (schema >= 9) as i64,
        "{context}"
    );
    let chat: String = conn
        .query_row(
            "SELECT id FROM conversations WHERE title = 'Fixture conversation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cairn_core::conversations::messages(conn, &chat)
            .unwrap()
            .len(),
        2,
        "{context}"
    );
    assert_eq!(
        count(conn, "SELECT count(*) FROM chat_files"),
        1,
        "{context}"
    );
    let conversation = cairn_core::conversations::get(conn, &chat).unwrap();
    if schema >= 7 {
        assert!(conversation.model_id.is_some(), "{context}");
        assert_eq!(conversation.effort.as_deref(), Some("high"), "{context}");
    }
    let archived = count(
        conn,
        "SELECT count(*) FROM conversations WHERE archived_at IS NOT NULL",
    );
    assert_eq!(archived, (schema >= 8) as i64, "{context}");
    if let Some(notes) = notes {
        check_external(db, name, notes);
    }
}

/// External mode (D-72): the index of the folder. Its removed pages and groups are checked before
/// the scan, which drops them 30 days after they were removed.
fn check_external(db: &mut Db, name: &str, notes: &Path) {
    let conn = db.conn();
    let context = format!("fixture {name}");
    assert_eq!(
        settings::get(conn).unwrap().external_root.as_deref(),
        notes.to_str(),
        "{context}"
    );
    let paths: Vec<String> = conn
        .prepare("SELECT path FROM pages")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(paths.len(), 4, "{context}");
    for path in &paths {
        assert!(notes.join(path).is_file(), "{context}: {path}");
    }
    assert_eq!(
        count(
            conn,
            "SELECT count(*) FROM groups g JOIN groups p ON g.parent_id = p.id
             WHERE g.path = 'Compat/Fixture group/Inner group' AND p.path = 'Compat/Fixture group'"
        ),
        1,
        "{context}"
    );

    // The page written outside Cairn, with its `[[Fixture page]]` link in the index's form.
    let (origin, body): (String, String) = conn
        .query_row(
            "SELECT origin, body FROM pages WHERE path = 'Compat/Fixture group/From outside.md'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let linked: String = conn
        .query_row(
            "SELECT id FROM pages WHERE title = 'Fixture page'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(origin, "folder", "{context}");
    assert!(body.contains(&format!("(page:{linked})")), "{context}");

    // The group folder deleted outside Cairn: its data and its page's task link wait for it.
    assert_eq!(
        count(
            conn,
            "SELECT count(*) FROM removed_groups WHERE path = 'Compat/Removed group'"
        ),
        1,
        "{context}"
    );
    let task_links: String = conn
        .query_row(
            "SELECT task_links FROM removed_pages WHERE path = 'Compat/Removed group/Removed page.md'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let task = tasks::get(conn, "CMP-1").unwrap();
    assert!(task_links.contains(&task.id), "{context}");

    // The index still matches the folder: a full scan finds nothing to change.
    let summary = scan::scan(db, Scope::All).unwrap();
    assert!(
        !summary.changed_anything() && summary.skipped.is_empty(),
        "{context}: {summary:?}"
    );
}

#[test]
fn every_schema_version_has_a_fixture() {
    let have: Vec<u32> = FIXTURES.iter().map(|f| f.1).collect();
    let need: Vec<u32> = (6..=migrations::latest_version()).collect();
    assert_eq!(
        have, need,
        "every schema version needs a fixture (tests/fixtures/compat/README.md)"
    );
    for (name, schema, _) in all() {
        assert_eq!(
            file_schema_version(&fixture(name).join("data/cairn.sqlite")),
            schema,
            "fixture {name}"
        );
    }
}

#[test]
fn opens_every_fixture_data_folder() {
    for (name, schema, external) in all() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        copy_dir(&fixture(name).join("data"), &data);
        let notes = external.then(|| {
            let notes = tmp.path().join("notes");
            copy_dir(&fixture(name).join("notes"), &notes);
            point_at(&data.join("cairn.sqlite"), &notes);
            notes
        });
        let mut db =
            Db::open(DataPaths::new(&data)).unwrap_or_else(|e| panic!("fixture {name}: {e}"));
        check(&mut db, name, schema, notes.as_deref());
        // An upgrade keeps a copy of the data as it was (NFR-REL-3).
        let backups = std::fs::read_dir(db.paths().backups_dir())
            .map(|d| d.count())
            .unwrap_or(0);
        let upgraded = schema < migrations::latest_version();
        assert_eq!(backups > 0, upgraded, "fixture {name}");
    }
}

#[test]
fn imports_every_fixture_export() {
    for (name, schema, external) in all() {
        let tmp = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(&tmp.path().join("data")).unwrap();
        let archive = fixture(name).join("export.zip");
        let (imported, notes) = if external {
            // Its folder isn't where it was made: the import is told where it is now (FR-KN-EXT-14).
            let notes = tmp.path().join("notes");
            copy_dir(&fixture(name).join("notes"), &notes);
            let imported = export::import_with_root(&mut db, &archive, notes.to_str().unwrap());
            (imported, Some(notes))
        } else {
            (export::import_from(&mut db, &archive), None)
        };
        let summary = imported.unwrap_or_else(|e| panic!("fixture {name}: {e}"));
        assert_eq!(summary.archive_schema_version, schema, "fixture {name}");
        let version = name.trim_end_matches("-external");
        assert_eq!(summary.archive_app_version, version, "fixture {name}");
        check(&mut db, name, schema, notes.as_deref());
    }
}
