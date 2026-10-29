use std::io::Read;

use chrono::NaiveDate;
use rusqlite::Connection;

use super::*;
use crate::categories::{self, NewCategory, CategoryColor};
use crate::tasks::{NewTask, Stamp};
use crate::{CoreError, attachments, drafts, mentions};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

struct Env {
    dir: tempfile::TempDir,
    db: Db,
    a: String,
    b: String,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open_at(dir.path()).unwrap();
    let mk = |name: &str, key: &str| {
        categories::create(
            db.conn(),
            NewCategory {
                name: name.into(),
                key: key.into(),
                description: String::new(),
                color: CategoryColor::preset("blue"),
            },
        )
        .unwrap()
        .id
    };
    let a = mk("Alpha", "AAA");
    let b = mk("Beta", "BBB");
    Env { dir, db, a, b }
}

impl Env {
    fn page(&mut self, category: &str, title: &str, body: &str) -> Page {
        create_page(
            &mut self.db,
            NewPage {
                category_id: category.into(),
                title: title.into(),
                body: body.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn group(&mut self, category: &str, name: &str) -> Group {
        create_group(
            &mut self.db,
            NewGroup {
                category_id: category.into(),
                name: name.into(),
                description: None,
                parent_id: None,
            },
        )
        .unwrap()
    }

    fn task(&mut self, category: &str, title: &str, description: &str) -> tasks::Task {
        tasks::create(
            &mut self.db,
            NewTask {
                category_id: category.into(),
                title: title.into(),
                description: Some(description.into()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn at<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> T {
        self.db.tx(|tx| f(tx)).unwrap()
    }

    fn image(&mut self, bytes: &[u8]) -> Attachment {
        attachments::save(&mut self.db, bytes, "image/png").unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn file_exists(&self, name: &str) -> bool {
        self.db.paths().attachments_dir().join(name).exists()
    }
}

fn patch_body(body: &str) -> PagePatch {
    PagePatch {
        body: Some(body.into()),
        ..Default::default()
    }
}

fn msg<T: std::fmt::Debug>(r: Result<T>) -> (&'static str, String) {
    let e = r.unwrap_err();
    (e.kind(), e.to_string())
}

// ---------------------------------------------------------------- groups

#[test]
fn group_rules() {
    let mut e = env();
    let g = e.group(&e.a.clone(), "  Design  ");
    assert_eq!(g.name, "Design");
    // Required, unique per category and case-insensitively.
    assert_eq!(
        msg(create_group(
            &mut e.db,
            NewGroup {
                category_id: e.a.clone(),
                name: "   ".into(),
                description: None,
                parent_id: None,
            }
        ))
        .0,
        "invalid"
    );
    let (kind, m) = msg(create_group(
        &mut e.db,
        NewGroup {
            category_id: e.a.clone(),
            name: "DESIGN".into(),
            description: None,
            parent_id: None,
        },
    ));
    assert_eq!(kind, "conflict");
    assert!(m.contains("DESIGN"));
    // The same name in another category is fine.
    e.group(&e.b.clone(), "design");
    // Unknown category.
    assert_eq!(
        msg(create_group(
            &mut e.db,
            NewGroup {
                category_id: "nope".into(),
                name: "x".into(),
                description: None,
                parent_id: None,
            }
        ))
        .0,
        "not_found"
    );
    let ops = e.group(&e.a.clone(), "Ops");
    // Rename: conflict with another, but its own name in another case is fine.
    assert_eq!(
        msg(update_group(
            &mut e.db,
            &ops.id,
            GroupPatch {
                name: Some("design".into()),
                description: None,
                parent_id: None,
            }
        ))
        .0,
        "conflict"
    );
    let renamed = update_group(
        &mut e.db,
        &ops.id,
        GroupPatch {
            name: Some("OPS".into()),
            description: Some(" Runbooks ".into()),
            parent_id: None,
        },
    )
    .unwrap();
    assert_eq!(
        (renamed.name.as_str(), renamed.description.as_str()),
        ("OPS", "Runbooks")
    );
    // No-op update writes nothing.
    let same = update_group(&mut e.db, &ops.id, GroupPatch::default()).unwrap();
    assert_eq!(same.updated_at, renamed.updated_at);
    // Listing: sorted by name, optionally by category.
    let names: Vec<_> = groups(e.db.conn(), Some(&e.a))
        .unwrap()
        .into_iter()
        .map(|g| g.name)
        .collect();
    assert_eq!(names, vec!["Design", "OPS"]);
    assert_eq!(groups(e.db.conn(), None).unwrap().len(), 3);
}

#[test]
fn deleting_a_group_ungroups_its_pages() {
    let mut e = env();
    let g = e.group(&e.a.clone(), "G");
    let p = create_page(
        &mut e.db,
        NewPage {
            category_id: e.a.clone(),
            title: "P".into(),
            body: String::new(),
            group_id: Some(g.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.group_id.as_deref(), Some(g.id.as_str()));
    delete_group(&mut e.db, &g.id).unwrap();
    assert_eq!(get_page(e.db.conn(), &p.id).unwrap().group_id, None);
    assert_eq!(msg(delete_group(&mut e.db, &g.id)).0, "not_found");
}

// ---------------------------------------------------------------- pages

#[test]
fn create_page_rules() {
    let mut e = env();
    let a = e.a.clone();
    let p = create_page(
        &mut e.db,
        NewPage {
            category_id: a.clone(),
            title: "  Setup guide ".into(),
            body: "Hello".into(),
            tags: Some(vec![
                " rust ".into(),
                "Rust".into(),
                "api".into(),
                "".into(),
                "Zeta".into(),
            ]),
            source_info: Some("  ".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p.title, "Setup guide");
    assert_eq!(p.tags, vec!["api", "rust", "Zeta"]);
    assert_eq!(p.source_info, None);
    assert_eq!(p.check_status, CheckStatus::NotChecked);
    assert_eq!(p.origin, PageOrigin::Manual);
    assert_eq!(get_page(e.db.conn(), &p.id).unwrap(), p);

    // Title required; unique per category, ignoring case; Chinese titles work.
    assert_eq!(
        msg(create_page(
            &mut e.db,
            NewPage {
                category_id: a.clone(),
                title: " ".into(),
                ..Default::default()
            }
        ))
        .0,
        "invalid"
    );
    let (kind, m) = msg(create_page(
        &mut e.db,
        NewPage {
            category_id: a.clone(),
            title: "SETUP GUIDE".into(),
            ..Default::default()
        },
    ));
    assert_eq!(kind, "conflict");
    assert!(m.contains("Alpha") && m.contains("SETUP GUIDE"));
    e.page(&a, "安装指南", "");
    assert_eq!(
        msg(create_page(
            &mut e.db,
            NewPage {
                category_id: a.clone(),
                title: "安装指南".into(),
                ..Default::default()
            }
        ))
        .0,
        "conflict"
    );
    // Another category may reuse the title.
    e.page(&e.b.clone(), "Setup guide", "");

    // Unknown or archived category; group of another category; unknown group.
    assert_eq!(
        msg(create_page(
            &mut e.db,
            NewPage {
                category_id: "nope".into(),
                title: "x".into(),
                ..Default::default()
            }
        ))
        .0,
        "not_found"
    );
    let other = e.group(&e.b.clone(), "B-group");
    assert_eq!(
        msg(create_page(
            &mut e.db,
            NewPage {
                category_id: a.clone(),
                title: "y".into(),
                group_id: Some(other.id.clone()),
                ..Default::default()
            }
        ))
        .0,
        "invalid"
    );
    assert_eq!(
        msg(create_page(
            &mut e.db,
            NewPage {
                category_id: a.clone(),
                title: "y".into(),
                group_id: Some("nope".into()),
                ..Default::default()
            }
        ))
        .0,
        "not_found"
    );
    let b = e.b.clone();
    categories::set_archived(e.db.conn(), &b, true).unwrap();
    let (kind, m) = msg(create_page(
        &mut e.db,
        NewPage {
            category_id: b,
            title: "z".into(),
            ..Default::default()
        },
    ));
    assert_eq!(kind, "invalid");
    assert!(m.contains("archived"));
    // Activity and a first version are written.
    assert_eq!(
        e.count(&format!(
            "SELECT COUNT(*) FROM activity WHERE kind = 'page_created' AND ref_id = '{}'",
            p.id
        )),
        1
    );
    assert_eq!(versions(e.db.conn(), &p.id).unwrap().len(), 1);
}

#[test]
fn update_rename_group_and_noop() {
    let mut e = env();
    let a = e.a.clone();
    let p1 = e.page(&a, "One", "body");
    let p2 = e.page(&a, "Two", "");
    let g = e.group(&a, "G");
    let other = e.group(&e.b.clone(), "H");

    // Rename conflicts (case-insensitively) but may change its own case.
    assert_eq!(
        msg(update_page(
            &mut e.db,
            &p2.id,
            PagePatch {
                title: Some("ONE".into()),
                ..Default::default()
            }
        ))
        .0,
        "conflict"
    );
    assert_eq!(
        msg(update_page(
            &mut e.db,
            &p2.id,
            PagePatch {
                title: Some(" ".into()),
                ..Default::default()
            }
        ))
        .0,
        "invalid"
    );
    let r = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            title: Some("ONE".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.title, "ONE");
    // Group set, wrong category refused, cleared with "".
    let r = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            group_id: Some(g.id.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.group_id.as_deref(), Some(g.id.as_str()));
    assert_eq!(
        msg(update_page(
            &mut e.db,
            &p1.id,
            PagePatch {
                group_id: Some(other.id.clone()),
                ..Default::default()
            }
        ))
        .0,
        "invalid"
    );
    let r = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            group_id: Some(String::new()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.group_id, None);
    // Source info set and cleared.
    let r = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            source_info: Some("https://x.test".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.source_info.as_deref(), Some("https://x.test"));
    let r = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            source_info: Some(String::new()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.source_info, None);
    // Tags replace the old ones.
    let r = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            tags: Some(vec!["b".into(), "A".into(), "B".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r.tags, vec!["A", "b"]);
    assert_eq!(tags(e.db.conn()).unwrap(), vec!["A", "b"]);
    update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            tags: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(tags(e.db.conn()).unwrap().is_empty());

    // A change that changes nothing writes nothing.
    let before = get_page(e.db.conn(), &p1.id).unwrap();
    let acts = e.count("SELECT COUNT(*) FROM activity");
    let same = update_page(
        &mut e.db,
        &p1.id,
        PagePatch {
            title: Some(" ONE ".into()),
            body: Some("body".into()),
            tags: Some(vec![]),
            source_info: Some("".into()),
            group_id: Some("".into()),
            base_hash: None,
        },
    )
    .unwrap();
    assert_eq!(same, before);
    assert_eq!(e.count("SELECT COUNT(*) FROM activity"), acts);
    assert_eq!(
        msg(update_page(&mut e.db, "nope", PagePatch::default())).0,
        "not_found"
    );
}

#[test]
fn move_page_rules() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let g = e.group(&a, "G");
    let p = create_page(
        &mut e.db,
        NewPage {
            category_id: a.clone(),
            title: "Move me".into(),
            group_id: Some(g.id),
            ..Default::default()
        },
    )
    .unwrap();
    e.page(&b, "move ME", "");
    let (kind, m) = msg(move_page(&mut e.db, &p.id, &b));
    assert_eq!(kind, "conflict");
    assert!(m.contains("Beta"));
    update_page(
        &mut e.db,
        &p.id,
        PagePatch {
            title: Some("Moved".into()),
            ..Default::default()
        },
    )
    .unwrap();
    // Same category: nothing happens.
    assert_eq!(move_page(&mut e.db, &p.id, &a).unwrap().title, "Moved");
    let moved = move_page(&mut e.db, &p.id, &b).unwrap();
    assert_eq!(
        (moved.category_id.as_str(), moved.group_id.clone()),
        (b.as_str(), None)
    );
    assert_eq!(get_page(e.db.conn(), &p.id).unwrap(), moved);
    // Archived target.
    categories::set_archived(e.db.conn(), &a, true).unwrap();
    assert_eq!(msg(move_page(&mut e.db, &p.id, &a)).0, "invalid");
    assert_eq!(msg(move_page(&mut e.db, &p.id, "nope")).0, "not_found");
}

#[test]
fn archive_and_check_status() {
    let mut e = env();
    let a = e.a.clone();
    let p = e.page(&a, "P", "x");
    let f = |e: &Env, include| {
        list_pages(
            e.db.conn(),
            &PageFilter {
                include_archived: Some(include),
                ..Default::default()
            },
        )
        .unwrap()
        .len()
    };
    let r = set_archived(&mut e.db, &p.id, true).unwrap();
    assert!(r.archived);
    assert_eq!((f(&e, false), f(&e, true)), (0, 1));
    let again = set_archived(&mut e.db, &p.id, true).unwrap();
    assert_eq!(again.updated_at, r.updated_at);
    assert!(!set_archived(&mut e.db, &p.id, false).unwrap().archived);

    let before = get_page(e.db.conn(), &p.id).unwrap();
    let c = set_check_status(&mut e.db, &p.id, CheckStatus::Checked).unwrap();
    assert_eq!(c.check_status, CheckStatus::Checked);
    assert_eq!(c.updated_at, before.updated_at);
    assert_eq!(
        get_page(e.db.conn(), &p.id).unwrap().check_status,
        CheckStatus::Checked
    );
    assert_eq!(
        msg(set_check_status(&mut e.db, "nope", CheckStatus::Checked)).0,
        "not_found"
    );
}

#[test]
fn list_filters_and_sorting() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let g = e.group(&a, "G");
    let mk =
        |e: &mut Env, day: &str, category: &str, title: &str, tags: &[&str], group: Option<&str>| {
            let st = Stamp::on(d(day));
            let (category, title) = (category.to_string(), title.to_string());
            let tags: Vec<String> = tags.iter().map(|s| s.to_string()).collect();
            let group = group.map(String::from);
            e.at(|c| {
                create_page_in(
                    c,
                    NewPage {
                        category_id: category,
                        title,
                        tags: Some(tags),
                        group_id: group,
                        ..Default::default()
                    },
                    &st,
                )
            })
        };
    let p1 = mk(&mut e, "2026-03-01", &a, "banana", &["fruit"], Some(&g.id));
    let p2 = mk(&mut e, "2026-03-02", &a, "Apple", &["fruit", "red"], None);
    let p3 = mk(&mut e, "2026-03-03", &b, "cherry", &["RED"], None);
    let ids = |f: PageFilter| -> Vec<String> {
        list_pages(e.db.conn(), &f)
            .unwrap()
            .into_iter()
            .map(|p| p.title)
            .collect()
    };
    assert_eq!(
        ids(PageFilter::default()),
        vec!["cherry", "Apple", "banana"]
    );
    assert_eq!(
        ids(PageFilter {
            descending: Some(false),
            ..Default::default()
        }),
        vec!["banana", "Apple", "cherry"]
    );
    assert_eq!(
        ids(PageFilter {
            sort: Some(PageSortField::Title),
            ..Default::default()
        }),
        vec!["Apple", "banana", "cherry"]
    );
    assert_eq!(
        ids(PageFilter {
            sort: Some(PageSortField::Created),
            descending: Some(true),
            limit: Some(2),
            ..Default::default()
        }),
        vec!["cherry", "Apple"]
    );
    assert_eq!(
        ids(PageFilter {
            category_ids: Some(vec![b.clone()]),
            ..Default::default()
        }),
        vec!["cherry"]
    );
    assert!(
        ids(PageFilter {
            category_ids: Some(vec![]),
            ..Default::default()
        })
        .is_empty()
    );
    assert_eq!(
        ids(PageFilter {
            group_id: Some(g.id.clone()),
            ..Default::default()
        }),
        vec!["banana"]
    );
    assert_eq!(
        ids(PageFilter {
            category_ids: Some(vec![a.clone()]),
            ungrouped: Some(true),
            ..Default::default()
        }),
        vec!["Apple"]
    );
    assert_eq!(
        ids(PageFilter {
            tags: Some(vec!["red".into()]),
            ..Default::default()
        }),
        vec!["cherry", "Apple"]
    );
    assert_eq!(
        ids(PageFilter {
            tags: Some(vec!["fruit".into(), "red".into()]),
            ..Default::default()
        })
        .len(),
        3
    );
    assert_eq!(
        ids(PageFilter {
            check_status: Some(CheckStatus::Checked),
            ..Default::default()
        })
        .len(),
        0
    );
    let _ = (p1, p2);
    let by_id =
        get_pages_by_ids(e.db.conn(), &[p3.id.clone(), "nope".into(), p3.id.clone()]).unwrap();
    assert_eq!(by_id.len(), 1);
    assert_eq!(by_id[0].tags, vec!["RED"]);
}

#[test]
fn word_counts() {
    assert_eq!(count_words(""), 0);
    assert_eq!(count_words("Hello, world! It's a well-known fact."), 6);
    assert_eq!(count_words("你好，世界"), 4);
    assert_eq!(count_words("Rust 很好 v2.0 ok"), 6);
    let mut e = env();
    let a = e.a.clone();
    let p = e.page(&a, "W", "三个字 and two");
    let s = list_pages(e.db.conn(), &PageFilter::default()).unwrap();
    assert_eq!(s[0].word_count, 5);
    assert_eq!(s[0].id, p.id);
}

// ---------------------------------------------------------------- history

#[test]
fn one_version_per_day_and_restore() {
    let mut e = env();
    let a = e.a.clone();
    let (d1, d2, d3) = (
        Stamp::on(d("2026-03-01")),
        Stamp::on(d("2026-03-02")),
        Stamp::on(d("2026-03-03")),
    );
    let g = e.group(&a, "G");
    let p = e.at(|c| {
        create_page_in(
            c,
            NewPage {
                category_id: a.clone(),
                title: "Doc".into(),
                body: "v1".into(),
                tags: Some(vec!["t1".into()]),
                group_id: Some(g.id.clone()),
                ..Default::default()
            },
            &d1,
        )
    });
    e.at(|c| update_page_in(c, &p.id, patch_body("v1b"), &d1));
    e.at(|c| {
        update_page_in(
            c,
            &p.id,
            PagePatch {
                body: Some("v2".into()),
                tags: Some(vec!["t2".into()]),
                title: Some("Doc 2".into()),
                group_id: Some(String::new()),
                source_info: Some("src".into()),
                base_hash: None,
            },
            &d2,
        )
    });
    let vs = versions(e.db.conn(), &p.id).unwrap();
    assert_eq!(vs.len(), 2);
    assert_eq!(
        (vs[0].date.as_str(), vs[0].snapshot.body.as_str()),
        ("2026-03-01", "v1b")
    );
    assert_eq!(vs[0].snapshot.group_id.as_deref(), Some(g.id.as_str()));
    assert_eq!(vs[1].snapshot.title, "Doc 2");
    // One page_updated activity per day (the create row is separate).
    assert_eq!(
        e.count("SELECT COUNT(*) FROM activity WHERE kind = 'page_updated'"),
        2
    );

    // Restore day 1 on day 3: a normal update, nothing lost.
    let r = e.at(|c| restore_version_in(c, &p.id, "2026-03-01", &d3));
    assert_eq!((r.title.as_str(), r.body.as_str()), ("Doc", "v1b"));
    assert_eq!(r.tags, vec!["t1"]);
    assert_eq!(r.group_id.as_deref(), Some(g.id.as_str()));
    assert_eq!(r.source_info, None);
    let vs = versions(e.db.conn(), &p.id).unwrap();
    assert_eq!(vs.len(), 3);
    assert_eq!(vs[1].snapshot.body, "v2");
    assert_eq!(vs[2].snapshot.body, "v1b");
    assert_eq!(
        msg(restore_version(&mut e.db, &p.id, "1999-01-01")).0,
        "not_found"
    );

    // A group deleted since then is dropped on restore; a taken title is a conflict.
    delete_group(&mut e.db, &g.id).unwrap();
    e.at(|c| update_page_in(c, &p.id, patch_body("later"), &d3));
    let r = e.at(|c| restore_version_in(c, &p.id, "2026-03-01", &d3));
    assert_eq!((r.group_id, r.body.as_str()), (None, "v1b"));
    e.at(|c| {
        update_page_in(
            c,
            &p.id,
            PagePatch {
                title: Some("Renamed".into()),
                ..Default::default()
            },
            &d3,
        )
    });
    e.page(&a, "Doc", "");
    assert_eq!(
        msg(restore_version(&mut e.db, &p.id, "2026-03-01")).0,
        "conflict"
    );
}

// ---------------------------------------------------------------- links and delete

#[test]
fn task_page_link_rules() {
    let mut e = env();
    let a = e.a.clone();
    let p = e.page(&a, "P", "");
    let q = e.page(&a, "Q", "");
    let t = e.task(&a, "T", "");
    let l = link_task_page(&mut e.db, &t.id, &p.id).unwrap();
    assert_eq!(
        (l.task_id.as_str(), l.page_id.as_str()),
        (t.id.as_str(), p.id.as_str())
    );
    let (kind, m) = msg(link_task_page(&mut e.db, &t.id, &p.id));
    assert_eq!(kind, "conflict");
    assert!(m.contains("AAA-1"));
    // A key works for the task too.
    assert_eq!(msg(link_task_page(&mut e.db, "AAA-1", &p.id)).0, "conflict");
    assert_eq!(msg(link_task_page(&mut e.db, "nope", &p.id)).0, "not_found");
    assert_eq!(msg(link_task_page(&mut e.db, &t.id, "nope")).0, "not_found");
    link_task_page(&mut e.db, &t.id, &q.id).unwrap();
    assert_eq!(
        task_page_links(e.db.conn(), Some(&t.id), None)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        task_page_links(e.db.conn(), None, Some(&p.id))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        task_page_links(e.db.conn(), Some(&t.id), Some(&q.id))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(task_page_links(e.db.conn(), None, None).unwrap().len(), 2);
    unlink_task_page(&mut e.db, &l.id).unwrap();
    assert_eq!(msg(unlink_task_page(&mut e.db, &l.id)).0, "not_found");
    // Deleting a page removes its links; deleting a task too.
    delete_page(&mut e.db, &q.id).unwrap();
    assert!(task_page_links(e.db.conn(), None, None).unwrap().is_empty());
}

#[test]
fn delete_page_cleans_up() {
    let mut e = env();
    let a = e.a.clone();
    let (ia, ib, ic, id_) = (
        e.image(b"image-a"),
        e.image(b"image-b"),
        e.image(b"image-c"),
        e.image(b"image-d"),
    );
    let img = |n: &Attachment| format!("![x](attachment:{})", n.name);

    let target = e.page(
        &a,
        "Target",
        &format!("{} {} {}", img(&ia), img(&ib), img(&ic)),
    );
    // B stays: another page's body. C stays: only a task description. D: in a version of another page.
    let other = e.page(&a, "Other", &img(&ib));
    let hist = e.page(&a, "Hist", &img(&id_));
    let (d1, d2) = (Stamp::now(), Stamp::on(d("2000-01-02")));
    let _ = (&d1, &d2);
    // Old version of "Hist" holds D, current body doesn't (different local days).
    e.at(|c| {
        c.execute(
            "UPDATE page_versions SET day = '2000-01-01' WHERE page_id = ?1",
            [&hist.id],
        )?;
        Ok(())
    });
    update_page(&mut e.db, &hist.id, patch_body("no image now")).unwrap();
    // Target's body mentions D too, so D becomes a candidate.
    update_page(
        &mut e.db,
        &target.id,
        patch_body(&format!(
            "{} {} {} {}",
            img(&ia),
            img(&ib),
            img(&ic),
            img(&id_)
        )),
    )
    .unwrap();
    let t = e.task(&a, "Task", &img(&ic));
    link_task_page(&mut e.db, &t.id, &target.id).unwrap();
    // A page that mentions the target, and the target mentioning it.
    update_page(
        &mut e.db,
        &other.id,
        patch_body(&format!("{} [T](page:{})", img(&ib), target.id)),
    )
    .unwrap();
    drafts::save(e.db.conn(), "page", &target.id, "{}").unwrap();
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 1);
    assert!(
        e.count(&format!(
            "SELECT COUNT(*) FROM activity WHERE ref_id = '{}'",
            target.id
        )) >= 2
    );

    delete_page(&mut e.db, &target.id).unwrap();
    assert_eq!(msg(get_page(e.db.conn(), &target.id)).0, "not_found");
    for table in ["page_versions", "page_tags", "task_page_links", "activity"] {
        let col = if table == "activity" {
            "ref_id"
        } else {
            "page_id"
        };
        assert_eq!(
            e.count(&format!(
                "SELECT COUNT(*) FROM {table} WHERE {col} = '{}'",
                target.id
            )),
            0,
            "{table}"
        );
    }
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 0);
    assert!(
        drafts::get(e.db.conn(), "page", &target.id)
            .unwrap()
            .is_none()
    );
    // A is gone (file and row); B, C and D are still referenced.
    assert!(!e.file_exists(&ia.name));
    assert_eq!(
        e.count(&format!(
            "SELECT COUNT(*) FROM attachments WHERE name = '{}'",
            ia.name
        )),
        0
    );
    for n in [&ib, &ic, &id_] {
        assert!(e.file_exists(&n.name), "{}", n.name);
    }
    // The other page and its history are untouched.
    assert_eq!(get_page(e.db.conn(), &other.id).unwrap().title, "Other");
    assert_eq!(msg(delete_page(&mut e.db, &target.id)).0, "not_found");
}

#[test]
fn garbage_collection_counts_all_sources() {
    let mut e = env();
    let a = e.a.clone();
    let mk = |e: &mut Env, s: &str| e.image(s.as_bytes());
    let imgs: Vec<_> = ["page", "page-version", "task", "comment", "draft", "unused"]
        .iter()
        .map(|s| mk(&mut e, s))
        .collect();
    let r = |i: usize| format!("attachment:{}", imgs[i].name);
    let p = e.page(&a, "P", &r(0));
    // Page version only: change the body on a later day.
    e.at(|c| {
        update_page_in(
            c,
            &p.id,
            patch_body(&format!("{} {}", r(0), r(1))),
            &Stamp::on(d("2000-01-01")),
        )
    });
    e.at(|c| update_page_in(c, &p.id, patch_body(&r(0)), &Stamp::on(d("2000-01-02"))));
    let t = e.task(&a, "T", &r(2));
    tasks::add_comment(&mut e.db, &t.id, &r(3)).unwrap();
    drafts::save(
        e.db.conn(),
        "new_page",
        "x",
        &format!("{{\"body\":\"![i]({})\"}}", r(4)),
    )
    .unwrap();
    // A stray file without a row.
    let stray = e.db.paths().attachments_dir().join("0123456789abcdef.png");
    std::fs::write(&stray, b"x").unwrap();
    std::fs::write(e.db.paths().attachments_dir().join("keep.txt"), b"x").unwrap();

    assert_eq!(attachments::collect_garbage(&mut e.db).unwrap(), 2);
    assert!(!e.file_exists(&imgs[5].name));
    assert!(!stray.exists());
    for n in &imgs[..5] {
        assert!(e.file_exists(&n.name), "{}", n.name);
    }
    assert!(e.file_exists("keep.txt"));
    assert_eq!(attachments::collect_garbage(&mut e.db).unwrap(), 0);
}

// ---------------------------------------------------------------- attachments and drafts

#[test]
fn attachments_dedupe_and_mime() {
    let mut e = env();
    let a1 = attachments::save(&mut e.db, b"\x89PNG data", "image/png").unwrap();
    let a2 = attachments::save(&mut e.db, b"\x89PNG data", "IMAGE/PNG").unwrap();
    assert_eq!(a1, a2);
    assert_eq!(a1.size, 9);
    assert_eq!(a1.mime, "image/png");
    assert!(a1.name.ends_with(".png"));
    assert_eq!(a1.name.len(), 16 + 4);
    assert_eq!(e.count("SELECT COUNT(*) FROM attachments"), 1);
    let on_disk = std::fs::read(e.db.paths().attachments_dir().join(&a1.name)).unwrap();
    assert_eq!(on_disk, b"\x89PNG data");
    // Different content, other types.
    let b = attachments::save(&mut e.db, b"jpeg", "image/jpeg").unwrap();
    assert!(b.name.ends_with(".jpg"));
    assert_ne!(b.name, a1.name);
    assert!(
        attachments::save(&mut e.db, b"gif", "image/gif")
            .unwrap()
            .name
            .ends_with(".gif")
    );
    assert!(
        attachments::save(&mut e.db, b"webp", "image/webp")
            .unwrap()
            .name
            .ends_with(".webp")
    );
    for bad in ["image/svg+xml", "text/html", "application/pdf", ""] {
        assert_eq!(
            msg(attachments::save(&mut e.db, b"x", bad)).0,
            "invalid",
            "{bad}"
        );
    }
    assert_eq!(
        msg(attachments::save(&mut e.db, b"", "image/png")).0,
        "invalid"
    );
    assert_eq!(e.count("SELECT COUNT(*) FROM attachments"), 3 + 1);
}

#[test]
fn draft_rules() {
    let e = env();
    let c = e.db.conn();
    assert_eq!(msg(drafts::save(c, "bogus", "x", "{}")).0, "invalid");
    for k in [
        "page",
        "new_page",
        "task_description",
        "comment",
        "new_comment",
    ] {
        drafts::save(c, k, "r1", "{}").unwrap();
    }
    let first = drafts::save(c, "new_page", "n1", "{\"a\":1}").unwrap();
    c.execute(
        "UPDATE drafts SET updated_at = '2000-01-01T00:00:00.000Z' WHERE ref_id = 'r1'",
        [],
    )
    .unwrap();
    let _second = drafts::save(c, "new_page", "n2", "{\"a\":2}").unwrap();
    // Replace keeps one row per (kind, ref).
    let replaced = drafts::save(c, "new_page", "n1", "{\"a\":3}").unwrap();
    assert_eq!(
        drafts::get(c, "new_page", "n1").unwrap(),
        Some(replaced.clone())
    );
    assert_eq!(replaced.content, "{\"a\":3}");
    assert!(replaced.updated_at >= first.updated_at);
    let list = drafts::list(c, "new_page").unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list.last().unwrap().ref_id, "r1");
    assert_eq!(drafts::get(c, "page", "zzz").unwrap(), None);
    drafts::delete(c, "new_page", "n1").unwrap();
    drafts::delete(c, "new_page", "n1").unwrap();
    assert_eq!(drafts::list(c, "new_page").unwrap().len(), 2);
    assert!(drafts::list(c, "comment").unwrap().len() == 1);
}

// ---------------------------------------------------------------- mentions

fn mentioned(e: &Env, target: MentionTarget, id: &str) -> Vec<Backlink> {
    mentions::backlinks(e.db.conn(), target, id).unwrap()
}

#[test]
fn mentions_and_backlinks() {
    let mut e = env();
    let a = e.a.clone();
    let target = e.page(&a, "Target", "");
    let task = e.task(&a, "Fix login", "");
    let body = format!(
        "See [Target](page:{t}) and #AAA-1, not `#AAA-1` or [x](page:{t}) in code.\n```\n#AAA-1 [y](page:{t})\n```\n#ZZZ-9 [bad](page:nope)",
        t = target.id
    );
    // Code spans hide mentions; the same target twice is one row.
    let src = e.page(
        &a,
        "Source",
        &format!("[T](page:{}) `[c](page:nope)`", target.id),
    );
    update_page(&mut e.db, &src.id, patch_body(&body)).unwrap();
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 2);
    let b = mentioned(&e, MentionTarget::Page, &target.id);
    assert_eq!(b.len(), 1);
    assert_eq!(
        (b[0].source, b[0].label.as_str(), b[0].page_id.as_deref()),
        (MentionSource::Page, "Source", Some(src.id.as_str()))
    );
    assert_eq!(b[0].task_id, None);
    let b = mentioned(&e, MentionTarget::Task, &task.id);
    assert_eq!(b.len(), 1);

    // Editing replaces the rows.
    update_page(&mut e.db, &src.id, patch_body("nothing")).unwrap();
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 0);
    update_page(
        &mut e.db,
        &src.id,
        patch_body(&format!("[T](page:{})", target.id)),
    )
    .unwrap();

    // A task description and a comment mention the page and the task.
    let t2 = e.task(
        &a,
        "Other",
        &format!("Needs [Target](page:{}) and #AAA-1", target.id),
    );
    let c = tasks::add_comment(
        &mut e.db,
        &task.id,
        &format!("see [Target](page:{})", target.id),
    )
    .unwrap();
    let b = mentioned(&e, MentionTarget::Page, &target.id);
    assert_eq!(b.len(), 3);
    let kinds: Vec<_> = b.iter().map(|x| x.source).collect();
    assert!(
        kinds.contains(&MentionSource::Page)
            && kinds.contains(&MentionSource::Task)
            && kinds.contains(&MentionSource::Comment)
    );
    assert!(b.windows(2).all(|w| w[0].at >= w[1].at), "newest first");
    let comment = b
        .iter()
        .find(|x| x.source == MentionSource::Comment)
        .unwrap();
    assert_eq!(comment.source_id, c.id);
    assert_eq!(comment.task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(comment.label, "AAA-1 Fix login");
    let from_task = b.iter().find(|x| x.source == MentionSource::Task).unwrap();
    assert_eq!(
        (from_task.task_id.as_deref(), from_task.label.as_str()),
        (Some(t2.id.as_str()), "AAA-2 Other")
    );
    // #AAA-1 from the description of task 2 only.
    assert_eq!(mentioned(&e, MentionTarget::Task, &task.id).len(), 1);

    // Moving a task gives it a new key; the old one is an alias that still resolves on the next save.
    let moved = tasks::move_to_category(&mut e.db, &task.id, &e.b.clone()).unwrap();
    assert_eq!(moved.key, "BBB-1");
    update_page(
        &mut e.db,
        &src.id,
        patch_body(&format!("[T](page:{}) #AAA-1", target.id)),
    )
    .unwrap();
    assert_eq!(mentioned(&e, MentionTarget::Task, &task.id).len(), 2);

    // Deleting a comment, then a source task, then a source page, then the target.
    tasks::delete_comment(&mut e.db, &c.id).unwrap();
    assert_eq!(mentioned(&e, MentionTarget::Page, &target.id).len(), 2);
    tasks::delete(&mut e.db, &t2.id).unwrap();
    assert_eq!(mentioned(&e, MentionTarget::Page, &target.id).len(), 1);
    assert_eq!(mentioned(&e, MentionTarget::Task, &task.id).len(), 1);
    delete_page(&mut e.db, &src.id).unwrap();
    assert!(mentioned(&e, MentionTarget::Page, &target.id).is_empty());
    assert!(mentioned(&e, MentionTarget::Task, &task.id).is_empty());
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 0);
}

#[test]
fn deleting_the_target_or_a_task_removes_rows() {
    let mut e = env();
    let a = e.a.clone();
    let target = e.page(&a, "Target", "");
    let t = e.task(&a, "T", "");
    let t_mentions = e.task(&a, "U", &format!("[x](page:{})", target.id));
    tasks::add_comment(&mut e.db, &t.id, &format!("#AAA-2 [x](page:{})", target.id)).unwrap();
    let p = e.page(&a, "P", &format!("#AAA-1 [x](page:{})", target.id));
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 5);
    // Deleting a task drops the rows it is source or target of, and those of its comments.
    tasks::delete(&mut e.db, &t.id).unwrap();
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 2);
    assert!(mentioned(&e, MentionTarget::Task, &t_mentions.id).is_empty());
    // Deleting the target page drops rows pointing at it.
    delete_page(&mut e.db, &target.id).unwrap();
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), 0);
    assert_eq!(
        get_page(e.db.conn(), &p.id).unwrap().body.contains("page:"),
        true
    );
}

#[test]
fn rebuild_reindexes_everything() {
    let mut e = env();
    let a = e.a.clone();
    let target = e.page(&a, "Target", "");
    e.page(&a, "Src", &format!("[x](page:{})", target.id));
    let t = e.task(&a, "T", "#AAA-1");
    tasks::add_comment(&mut e.db, &t.id, &format!("[x](page:{})", target.id)).unwrap();
    let before = e.count("SELECT COUNT(*) FROM mentions");
    assert_eq!(before, 2);
    e.db.conn().execute("DELETE FROM mentions", []).unwrap();
    mentions::rebuild(e.db.conn()).unwrap();
    assert_eq!(e.count("SELECT COUNT(*) FROM mentions"), before);
    crate::hooks::rebuild_all(e.db.conn()).unwrap_or(());
}

// ---------------------------------------------------------------- export

fn export_env() -> (Env, Page, Attachment) {
    let mut e = env();
    let a = e.a.clone();
    let g = e.group(&a, "Guides");
    let other = e.page(&a, "Other page", "x");
    let t = e.task(&a, "Fix login", "");
    let img = e.image(b"\x89PNG\r\n image bytes");
    let missing = "attachment:ffffffffffffffff.png";
    let body = format!(
        "# Head\n\nSee [the old name](page:{o}) and #{k} and #AAA-99.\n\n![shot](attachment:{n})\n\n`#{k} [c](page:{o})`\n\n```\n![in code](attachment:{n})\n```\n\n![gone]({missing})\n",
        o = other.id,
        k = t.key,
        n = img.name
    );
    let p = create_page(
        &mut e.db,
        NewPage {
            category_id: a,
            title: "Setup: \"guide\"".into(),
            body,
            group_id: Some(g.id),
            tags: Some(vec!["beta".into(), "alpha".into()]),
            source_info: Some("https://example.test/a".into()),
            ..Default::default()
        },
    )
    .unwrap();
    (e, p, img)
}

#[test]
fn export_markdown_folder() {
    let (e, p, img) = export_env();
    let out = e.dir.path().join("out dir").join("My page.md");
    let r = export_markdown(&e.db, &p.id, &out, MarkdownExportMode::Folder).unwrap();
    assert_eq!(r.images, 1);
    assert_eq!(r.path, out.to_string_lossy());
    let md = std::fs::read_to_string(&out).unwrap();
    assert!(md.starts_with("---\ntitle: \"Setup: \\\"guide\\\"\"\ncategory: \"Alpha\"\ngroup: \"Guides\"\ntags: [\"alpha\", \"beta\"]\nsource_info: \"https://example.test/a\"\ncreated: \""), "{md}");
    assert!(md.contains("\n---\n\n# Head"));
    // Mentions are readable text; unknown keys stay; code is untouched.
    assert!(
        md.contains("See Other page and AAA-1 Fix login and #AAA-99."),
        "{md}"
    );
    assert!(md.contains("`#AAA-1 [c](page:"));
    // Images are relative, the missing one stays as it was.
    assert!(
        md.contains(&format!("![shot](My%20page_files/{})", img.name)),
        "{md}"
    );
    assert!(md.contains(&format!("![in code](attachment:{})", img.name)));
    assert!(md.contains("![gone](attachment:ffffffffffffffff.png)"));
    let copied = out.with_file_name("My page_files").join(&img.name);
    assert_eq!(std::fs::read(copied).unwrap(), b"\x89PNG\r\n image bytes");
}

#[test]
fn export_markdown_zip_and_no_images() {
    let (e, p, img) = export_env();
    let out = e.dir.path().join("page.zip");
    let r = export_markdown(&e.db, &p.id, &out, MarkdownExportMode::Zip).unwrap();
    assert_eq!(r.images, 1);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    let mut names: Vec<_> = zip.file_names().map(String::from).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["page.md".to_string(), format!("page_files/{}", img.name)]
    );
    let mut md = String::new();
    zip.by_name("page.md")
        .unwrap()
        .read_to_string(&mut md)
        .unwrap();
    assert!(md.contains(&format!("![shot](page_files/{})", img.name)));
    let mut bytes = Vec::new();
    zip.by_name(&format!("page_files/{}", img.name))
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    assert_eq!(bytes, b"\x89PNG\r\n image bytes");

    // No images: no folder in Folder mode; no group, tags or source: no such lines.
    let mut e = e;
    let a = e.a.clone();
    let plain = e.page(&a, "Plain", "just text");
    let out = e.dir.path().join("plain.md");
    let r = export_markdown(&e.db, &plain.id, &out, MarkdownExportMode::Folder).unwrap();
    assert_eq!(r.images, 0);
    assert!(!e.dir.path().join("plain_files").exists());
    let md = std::fs::read_to_string(&out).unwrap();
    assert!(!md.contains("group:") && !md.contains("tags:") && !md.contains("source_info:"));
    assert!(md.ends_with("---\n\njust text\n"));
    assert_eq!(
        msg(export_markdown(
            &e.db,
            "nope",
            &out,
            MarkdownExportMode::Folder
        ))
        .0,
        "not_found"
    );
}

// ---------------------------------------------------------------- seed

#[test]
fn seed_is_idempotent_and_never_overwrites() {
    let mut e = env();
    let a = e.a.clone();
    let task = e.task(&a, "T", "");
    let g = Group {
        id: "g1".into(),
        category_id: a.clone(),
        parent_id: None,
        name: "Seed group".into(),
        description: "d".into(),
        path: None,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-01T00:00:00.000Z".into(),
    };
    let page = |id: &str, title: &str, body: &str, group: Option<&str>| Page {
        id: id.into(),
        category_id: a.clone(),
        group_id: group.map(String::from),
        title: title.into(),
        body: body.into(),
        tags: vec!["Zed".into(), "alpha".into(), "ALPHA".into()],
        source_info: None,
        origin: PageOrigin::File,
        origin_task_id: Some(task.id.clone()),
        check_status: CheckStatus::Checked,
        archived: false,
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-02T00:00:00.000Z".into(),
        path: None,
        file_hash: None,
    };
    let snap = |t: &str| PageSnapshot {
        title: t.into(),
        body: "old".into(),
        tags: vec![],
        group_id: None,
        source_info: None,
    };
    let seed = || KnowledgeSeed {
        groups: vec![
            g.clone(),
            Group {
                id: "g2".into(),
                category_id: "missing".into(),
                ..g.clone()
            },
        ],
        pages: vec![
            page("p1", "Seed one", "links [two](page:p2) #AAA-1", Some("g1")),
            page("p2", "Seed two", "", Some("missing-group")),
            page("p3", "Orphan", "", None),
            Page {
                category_id: "missing".into(),
                ..page("p4", "No category", "", None)
            },
        ],
        versions: vec![
            PageVersion {
                page_id: "p1".into(),
                date: "2026-01-01".into(),
                snapshot: snap("Seed one"),
            },
            PageVersion {
                page_id: "nope".into(),
                date: "2026-01-01".into(),
                snapshot: snap("x"),
            },
        ],
        task_page_links: vec![
            TaskPageLink {
                id: "l1".into(),
                task_id: task.id.clone(),
                page_id: "p1".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
            },
            TaskPageLink {
                id: "l2".into(),
                task_id: "nope".into(),
                page_id: "p1".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
            },
            TaskPageLink {
                id: "l3".into(),
                task_id: task.id.clone(),
                page_id: "nope".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
            },
        ],
    };
    seed_if_missing(&mut e.db, seed()).unwrap();
    let p1 = get_page(e.db.conn(), "p1").unwrap();
    assert_eq!(
        (p1.group_id.as_deref(), p1.tags.clone()),
        (Some("g1"), vec!["alpha".to_string(), "Zed".to_string()])
    );
    assert_eq!(get_page(e.db.conn(), "p2").unwrap().group_id, None);
    assert_eq!(msg(get_page(e.db.conn(), "p4")).0, "not_found");
    assert_eq!(groups(e.db.conn(), None).unwrap().len(), 1);
    assert_eq!(versions(e.db.conn(), "p1").unwrap().len(), 1);
    assert_eq!(task_page_links(e.db.conn(), None, None).unwrap().len(), 1);
    // Hooks ran: mentions of the inserted pages exist.
    assert_eq!(mentioned(&e, MentionTarget::Page, "p2").len(), 1);
    assert_eq!(mentioned(&e, MentionTarget::Task, &task.id).len(), 1);

    // Edit, then seed again: nothing is overwritten, nothing duplicated.
    update_page(&mut e.db, "p1", patch_body("edited")).unwrap();
    let before = (
        e.count("SELECT COUNT(*) FROM pages"),
        e.count("SELECT COUNT(*) FROM page_versions"),
        e.count("SELECT COUNT(*) FROM page_tags"),
    );
    seed_if_missing(&mut e.db, seed()).unwrap();
    assert_eq!(get_page(e.db.conn(), "p1").unwrap().body, "edited");
    let after = (
        e.count("SELECT COUNT(*) FROM pages"),
        e.count("SELECT COUNT(*) FROM page_versions"),
        e.count("SELECT COUNT(*) FROM page_tags"),
    );
    assert_eq!(before, after);
    // The edited page's mentions stay those of its new body.
    assert!(mentioned(&e, MentionTarget::Page, "p2").is_empty());
}

#[test]
fn error_kinds_are_stable() {
    // Guards the messages used by the UI.
    assert!(matches!(
        CoreError::Conflict("x".into()),
        CoreError::Conflict(_)
    ));
}

// ---------------------------------------------------------------- many at once (FR-KN-UI-3)

#[test]
fn many_pages_change_together_and_skips_are_named() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let p1 = e.page(&a, "One", "x");
    let p2 = e.page(&a, "Two", "y");
    let clash = e.page(&b, "Two", "already in Beta");
    let both = vec![p1.id.clone(), p2.id.clone()];

    let r = update_many(&mut e.db, &both, PageBulkChange::Archive).unwrap();
    assert_eq!(r.done, 2);
    assert!(get_page(e.db.conn(), &p1.id).unwrap().archived);
    update_many(&mut e.db, &both, PageBulkChange::Restore).unwrap();
    assert!(!get_page(e.db.conn(), &p2.id).unwrap().archived);

    update_many(
        &mut e.db,
        &both,
        PageBulkChange::AddTag {
            tag: " Rust ".into(),
        },
    )
    .unwrap();
    assert_eq!(get_page(e.db.conn(), &p1.id).unwrap().tags, ["Rust"]);
    update_many(
        &mut e.db,
        &both,
        PageBulkChange::RemoveTag { tag: "rust".into() },
    )
    .unwrap();
    assert!(get_page(e.db.conn(), &p2.id).unwrap().tags.is_empty());
    assert_eq!(
        msg(update_many(
            &mut e.db,
            &both,
            PageBulkChange::AddTag { tag: "".into() }
        ))
        .0,
        "invalid"
    );

    update_many(&mut e.db, &both, PageBulkChange::MarkChecked).unwrap();
    assert_eq!(
        get_page(e.db.conn(), &p1.id).unwrap().check_status,
        CheckStatus::Checked
    );

    // Into a group of the same category, then to Beta: "Two" clashes there and stays where it was.
    let ga = e.group(&a, "Notes");
    let gb = e.group(&b, "Inbox");
    update_many(
        &mut e.db,
        &both,
        PageBulkChange::Move {
            category_id: a.clone(),
            group_id: Some(ga.id.clone()),
        },
    )
    .unwrap();
    assert_eq!(
        get_page(e.db.conn(), &p2.id).unwrap().group_id.as_deref(),
        Some(ga.id.as_str())
    );
    let r = update_many(
        &mut e.db,
        &both,
        PageBulkChange::Move {
            category_id: b.clone(),
            group_id: Some(gb.id.clone()),
        },
    )
    .unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].title, "Two");
    assert!(
        r.skipped[0].reason.contains("already has a page titled"),
        "{}",
        r.skipped[0].reason
    );
    let moved = get_page(e.db.conn(), &p1.id).unwrap();
    assert_eq!(
        (moved.category_id.as_str(), moved.group_id.as_deref()),
        (b.as_str(), Some(gb.id.as_str()))
    );
    let stayed = get_page(e.db.conn(), &p2.id).unwrap();
    assert_eq!(
        (stayed.category_id.as_str(), stayed.group_id.as_deref()),
        (a.as_str(), Some(ga.id.as_str()))
    );
    // No group, and a group of another category is refused as a whole.
    update_many(
        &mut e.db,
        std::slice::from_ref(&p1.id),
        PageBulkChange::Move {
            category_id: b.clone(),
            group_id: None,
        },
    )
    .unwrap();
    assert_eq!(get_page(e.db.conn(), &p1.id).unwrap().group_id, None);
    assert_eq!(
        msg(update_many(
            &mut e.db,
            &both,
            PageBulkChange::Move {
                category_id: b.clone(),
                group_id: Some(ga.id.clone())
            }
        ))
        .0,
        "invalid"
    );

    let r = delete_many(&mut e.db, &[p1.id.clone(), clash.id.clone()]).unwrap();
    assert_eq!(r.done, 2);
    assert!(get_page(e.db.conn(), &p1.id).is_err() && get_page(e.db.conn(), &clash.id).is_err());
    assert!(get_page(e.db.conn(), &p2.id).is_ok());
}

#[test]
fn deleting_many_pages_cleans_up_their_images() {
    let mut e = env();
    let a = e.a.clone();
    let (i1, i2) = (e.image(b"image-1"), e.image(b"image-2"));
    let p1 = e.page(&a, "P1", &format!("![x](attachment:{})", i1.name));
    let p2 = e.page(&a, "P2", &format!("![x](attachment:{})", i2.name));
    let keep = e.page(&a, "Keep", &format!("![x](attachment:{})", i2.name));
    delete_many(&mut e.db, &[p1.id, p2.id]).unwrap();
    assert!(!e.file_exists(&i1.name), "an image no page uses is removed");
    assert!(e.file_exists(&i2.name), "an image another page uses stays");
    assert!(get_page(e.db.conn(), &keep.id).is_ok());
}
