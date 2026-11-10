//! What was opened last (D-94): when each task and page was last opened, or picked from a `#` or
//! `[[` suggestion list, so those lists show the most recently opened first. One row per item in
//! `opened` (schema step 11); recording never changes the item's own `updated_at`, and a deleted
//! item takes its row with it.

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::Result;
use crate::util::now_iso;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OpenedKind {
    Task,
    Page,
}

impl OpenedKind {
    fn as_str(self) -> &'static str {
        match self {
            OpenedKind::Task => "task",
            OpenedKind::Page => "page",
        }
    }

    fn table(self) -> &'static str {
        match self {
            OpenedKind::Task => "tasks",
            OpenedKind::Page => "pages",
        }
    }
}

/// Records `id` as opened now. An id that isn't a task (or page) is ignored.
pub fn record(conn: &Connection, kind: OpenedKind, id: &str) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO opened (kind, item_id, opened_at) SELECT ?1, id, ?3 FROM {} WHERE id = ?2 \
             ON CONFLICT (kind, item_id) DO UPDATE SET opened_at = excluded.opened_at",
            kind.table()
        ),
        params![kind.as_str(), id, now_iso()],
    )?;
    Ok(())
}

/// The ids of the `limit` most recently opened tasks (or pages), most recent first.
pub fn recent(conn: &Connection, kind: OpenedKind, limit: u32) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT item_id FROM opened WHERE kind = ?1 ORDER BY opened_at DESC, item_id LIMIT ?2",
    )?;
    let ids = stmt
        .query_map(params![kind.as_str(), limit], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::knowledge::{self, NewPage};
    use crate::tasks::{self, NewTask};

    #[test]
    fn the_most_recently_opened_come_first_and_deleted_items_go() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        let category = crate::test_util::category(db.conn(), "Alpha", "AAA");
        let mut task = |title: &str| {
            tasks::create(
                &mut db,
                NewTask {
                    category_id: category.clone(),
                    title: title.into(),
                    ..Default::default()
                },
            )
            .unwrap()
            .id
        };
        let (one, two, three) = (task("one"), task("two"), task("three"));
        let page = knowledge::create_page(
            &mut db,
            NewPage {
                category_id: category.clone(),
                title: "Page".into(),
                body: "Text.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let conn = db.conn();
        let updated = tasks::get(conn, &one).unwrap().updated_at;
        for id in [&one, &two, &three] {
            record(conn, OpenedKind::Task, id).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        record(conn, OpenedKind::Task, &one).unwrap();
        record(conn, OpenedKind::Task, "no-such-task").unwrap();
        record(conn, OpenedKind::Task, &page.id).unwrap(); // a page isn't a task
        record(conn, OpenedKind::Page, &page.id).unwrap();
        assert_eq!(
            recent(conn, OpenedKind::Task, 10).unwrap(),
            vec![one.clone(), three.clone(), two.clone()]
        );
        assert_eq!(
            recent(conn, OpenedKind::Task, 2).unwrap(),
            vec![one.clone(), three.clone()]
        );
        assert_eq!(
            recent(conn, OpenedKind::Page, 10).unwrap(),
            vec![page.id.clone()]
        );
        assert_eq!(tasks::get(conn, &one).unwrap().updated_at, updated);

        tasks::delete(&mut db, &three).unwrap();
        knowledge::delete_page(&mut db, &page.id).unwrap();
        let conn = db.conn();
        assert_eq!(recent(conn, OpenedKind::Task, 10).unwrap(), vec![one, two]);
        assert!(recent(conn, OpenedKind::Page, 10).unwrap().is_empty());
    }
}
