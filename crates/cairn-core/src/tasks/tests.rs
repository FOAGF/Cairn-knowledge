use chrono::NaiveDate;
use rusqlite::Connection;

use super::query::list_on;
use super::workdays::{blocked_working_days, working_days_between};
use super::*;
use crate::categories::{self, NewCategory, CategoryColor};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

struct Env {
    _dir: tempfile::TempDir,
    db: Db,
    /// Active category "AAA" and "BBB".
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
    Env {
        _dir: dir,
        db,
        a,
        b,
    }
}

impl Env {
    fn new_task(&mut self, category: &str, title: &str) -> Task {
        create(
            &mut self.db,
            NewTask {
                category_id: category.into(),
                title: title.into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn at<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> T {
        self.db.tx(|tx| f(tx)).unwrap()
    }

    fn set_status(&mut self, id: &str, status: TaskStatus, st: &Stamp) -> Task {
        let st = st.clone();
        self.at(|c| {
            update_in(
                c,
                id,
                TaskPatch {
                    status: Some(status),
                    ..Default::default()
                },
                &st,
            )
        })
    }
}

fn invalid<T: std::fmt::Debug>(r: Result<T>) -> String {
    match r {
        Err(CoreError::Invalid(m)) => m,
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn conflict<T: std::fmt::Debug>(r: Result<T>) -> String {
    match r {
        Err(CoreError::Conflict(m)) => m,
        other => panic!("expected Conflict, got {other:?}"),
    }
}

// ------------------------------------------------------------ keys, create, validation

#[test]
fn keys_count_up_per_category_and_are_never_reused() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t1 = e.new_task(&a, "one");
    let t2 = e.new_task(&a, "two");
    let u1 = e.new_task(&b, "other");
    assert_eq!(
        (t1.key.as_str(), t2.key.as_str(), u1.key.as_str()),
        ("AAA-1", "AAA-2", "BBB-1")
    );
    delete(&mut e.db, &t2.id).unwrap();
    assert_eq!(e.new_task(&a, "three").key, "AAA-3");
    assert_eq!(
        get(e.db.conn(), "aaa-1").unwrap().id,
        t1.id,
        "case-insensitive key"
    );
}

#[test]
fn create_defaults_and_validation() {
    let mut e = env();
    let a = e.a.clone();
    let t = create(
        &mut e.db,
        NewTask {
            category_id: a.clone(),
            title: "  Fix 登录 bug  ".into(),
            labels: Some(vec![" UI ".into(), "ui".into(), "".into(), "api".into()]),
            due_date: Some("2026-02-28".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(t.title, "Fix 登录 bug");
    assert_eq!(
        (t.status, t.priority, t.task_type),
        (TaskStatus::Todo, Priority::Medium, TaskType::General)
    );
    assert_eq!(t.labels, vec!["api", "UI"]);
    assert_eq!(t.completed_at, None);
    assert_eq!(get(e.db.conn(), &t.id).unwrap(), t);

    let new = |title: &str, due: Option<&str>, category: &str| NewTask {
        category_id: category.into(),
        title: title.into(),
        due_date: due.map(Into::into),
        ..Default::default()
    };
    assert!(invalid(create(&mut e.db, new("   ", None, &a))).contains("title"));
    for bad in [
        "2026-02-30",
        "2026-13-01",
        "28/02/2026",
        "2026-2-3",
        "tomorrow",
    ] {
        invalid(create(&mut e.db, new("x", Some(bad), &a)));
    }
    assert!(matches!(
        create(&mut e.db, new("x", None, "nope")),
        Err(CoreError::NotFound { .. })
    ));
    // Failed creates must not burn numbers.
    assert_eq!(e.new_task(&a, "next").key, "AAA-2");
}

#[test]
fn archived_categories_refuse_new_and_moved_tasks() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t = e.new_task(&a, "t");
    categories::set_archived(e.db.conn(), &b, true).unwrap();
    let msg = invalid(create(
        &mut e.db,
        NewTask {
            category_id: b.clone(),
            title: "x".into(),
            ..Default::default()
        },
    ));
    assert!(msg.contains("archived"), "{msg}");
    invalid(move_to_category(&mut e.db, &t.id, &b));
    assert_eq!(get(e.db.conn(), &t.id).unwrap().key, "AAA-1");
}

// ------------------------------------------------------------ update, completed_at

#[test]
fn update_changes_only_given_fields_and_validates() {
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "t");
    let u = update(
        &mut e.db,
        &t.id,
        TaskPatch {
            description: Some("desc".into()),
            priority: Some(Priority::High),
            due_date: Some("2026-03-01".into()),
            labels: Some(vec!["b".into(), "A".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!((u.title.as_str(), u.description.as_str()), ("t", "desc"));
    assert_eq!(u.labels, vec!["A", "b"]);
    assert_eq!(u.due_date.as_deref(), Some("2026-03-01"));
    // Empty string clears, absent keeps.
    let u = update(
        &mut e.db,
        &t.id,
        TaskPatch {
            due_date: Some("".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(u.due_date, None);
    let u = update(
        &mut e.db,
        &t.id,
        TaskPatch {
            title: Some(" new ".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(u.title, "new");
    invalid(update(
        &mut e.db,
        &t.id,
        TaskPatch {
            title: Some(" ".into()),
            ..Default::default()
        },
    ));
    invalid(update(
        &mut e.db,
        &t.id,
        TaskPatch {
            due_date: Some("2026-02-31".into()),
            ..Default::default()
        },
    ));
    assert!(matches!(
        update(&mut e.db, "nope", TaskPatch::default()),
        Err(CoreError::NotFound { .. })
    ));
    // A failed update changes nothing.
    assert_eq!(get(e.db.conn(), &t.id).unwrap().title, "new");
}

#[test]
fn completed_at_rules() {
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "t");
    let s1 = Stamp::on(d("2026-03-02"));
    let s2 = Stamp::on(d("2026-03-03"));
    let s3 = Stamp::on(d("2026-03-04"));
    let s4 = Stamp::on(d("2026-03-05"));

    let t = e.set_status(&t.id, TaskStatus::InProgress, &s1);
    assert_eq!(t.completed_at, None);
    let done = e.set_status(&t.id, TaskStatus::Done, &s1);
    assert_eq!(done.completed_at.as_deref(), Some(s1.at.as_str()));
    // done -> wont_do keeps the original time.
    let wont = e.set_status(&t.id, TaskStatus::WontDo, &s2);
    assert_eq!(wont.completed_at, done.completed_at);
    assert_eq!(wont.updated_at, s2.at);
    // Reopening clears it; closing again sets a new one.
    let open = e.set_status(&t.id, TaskStatus::Blocked, &s3);
    assert_eq!(open.completed_at, None);
    let again = e.set_status(&t.id, TaskStatus::Done, &s4);
    assert_eq!(again.completed_at.as_deref(), Some(s4.at.as_str()));
    // Persisted.
    assert_eq!(get(e.db.conn(), &t.id).unwrap(), again);
}

#[test]
fn creating_closed_sets_completed_at_and_noop_update_writes_nothing() {
    let mut e = env();
    let a = e.a.clone();
    let t = create(
        &mut e.db,
        NewTask {
            category_id: a,
            title: "x".into(),
            status: Some(TaskStatus::Done),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(t.completed_at.as_deref(), Some(t.created_at.as_str()));
    let before = activity(e.db.conn(), 100).unwrap().len();
    let same = update(
        &mut e.db,
        &t.id,
        TaskPatch {
            title: Some("x".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(same, t);
    assert_eq!(activity(e.db.conn(), 100).unwrap().len(), before);
}

// ------------------------------------------------------------ versions

#[test]
fn one_version_per_day_overwritten_by_later_changes() {
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "t");
    let today = Stamp::now();
    for title in ["v1", "v2", "v3"] {
        e.at(|c| {
            update_in(
                c,
                &t.id,
                TaskPatch {
                    title: Some(title.into()),
                    ..Default::default()
                },
                &today,
            )
        });
    }
    let v = versions(e.db.conn(), &t.id).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].date, date_str(today.day));
    assert_eq!(v[0].snapshot.title, "v3");
    assert_eq!(v[0].snapshot.category_id, a);

    let later = Stamp::on(today.day + chrono::Duration::days(2));
    e.at(|c| {
        update_in(
            c,
            &t.id,
            TaskPatch {
                priority: Some(Priority::Low),
                ..Default::default()
            },
            &later,
        )
    });
    let earlier = Stamp::on(today.day - chrono::Duration::days(3));
    e.at(|c| {
        update_in(
            c,
            &t.id,
            TaskPatch {
                priority: Some(Priority::High),
                ..Default::default()
            },
            &earlier,
        )
    });
    let v = versions(e.db.conn(), &t.id).unwrap();
    assert_eq!(v.len(), 3);
    assert!(v.windows(2).all(|w| w[0].date < w[1].date), "oldest first");
    assert_eq!(v[0].snapshot.priority, Priority::High);
    assert_eq!(v[1].snapshot.title, "v3");
    assert_eq!(v[2].snapshot.priority, Priority::Low);
    // Lookup works through the current key too.
    assert_eq!(versions(e.db.conn(), "AAA-1").unwrap().len(), 3);
}

#[test]
fn comments_make_no_version_and_no_updated_at_change() {
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "t");
    add_comment(&mut e.db, &t.id, "hello").unwrap();
    assert_eq!(versions(e.db.conn(), &t.id).unwrap().len(), 1);
    assert_eq!(get(e.db.conn(), &t.id).unwrap().updated_at, t.updated_at);
}

// ------------------------------------------------------------ activity

#[test]
fn activity_rules() {
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "t");
    let kinds = |e: &Env| -> Vec<ActivityKind> {
        let mut v = activity(e.db.conn(), 100).unwrap();
        v.reverse(); // oldest first
        v.into_iter().map(|a| a.kind).collect()
    };
    assert_eq!(kinds(&e), vec![ActivityKind::TaskCreated]);

    let day1 = Stamp::on(d("2026-05-04"));
    let day2 = Stamp::on(d("2026-05-05"));
    for title in ["a", "b", "c"] {
        e.at(|c| {
            update_in(
                c,
                &t.id,
                TaskPatch {
                    title: Some(title.into()),
                    ..Default::default()
                },
                &day1,
            )
        });
    }
    e.at(|c| add_comment_in(c, &t.id, "hi", &day1));
    // Same task, same day: a single task_updated.
    let updated = |e: &Env| {
        activity(e.db.conn(), 100)
            .unwrap()
            .into_iter()
            .filter(|a| a.kind == ActivityKind::TaskUpdated)
            .count()
    };
    assert_eq!(updated(&e), 1);
    e.at(|c| {
        update_in(
            c,
            &t.id,
            TaskPatch {
                title: Some("d".into()),
                ..Default::default()
            },
            &day2,
        )
    });
    assert_eq!(updated(&e), 2);
    // Completing writes task_completed instead of task_updated.
    e.set_status(&t.id, TaskStatus::Done, &day2);
    assert_eq!(updated(&e), 2);
    assert!(
        activity(e.db.conn(), 100)
            .unwrap()
            .iter()
            .any(|a| a.ref_id == t.id && a.kind == ActivityKind::TaskCompleted && a.at == day2.at)
    );
    // wont_do is an update, not a completion.
    let t2 = e.new_task(&a, "t2");
    e.set_status(&t2.id, TaskStatus::WontDo, &day2);
    assert!(
        !activity(e.db.conn(), 100)
            .unwrap()
            .iter()
            .any(|a| a.ref_id == t2.id && a.kind == ActivityKind::TaskCompleted)
    );
    // Newest first, with a limit; other tasks are not touched.
    let all = activity(e.db.conn(), 100).unwrap();
    assert!(all.windows(2).all(|w| w[0].at >= w[1].at));
    assert_eq!(activity(e.db.conn(), 2).unwrap().len(), 2);
}

// ------------------------------------------------------------ labels

#[test]
fn labels_are_unique_sorted_and_listed() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t = create(
        &mut e.db,
        NewTask {
            category_id: a,
            title: "t".into(),
            labels: Some(vec![
                "zeta".into(),
                "Alpha".into(),
                "alpha".into(),
                "  ".into(),
                "beta ".into(),
            ]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(t.labels, vec!["Alpha", "beta", "zeta"]);
    create(
        &mut e.db,
        NewTask {
            category_id: b,
            title: "u".into(),
            labels: Some(vec!["ALPHA".into(), "中文".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        labels(e.db.conn()).unwrap(),
        vec!["Alpha", "beta", "zeta", "中文"]
    );
    let u = update(
        &mut e.db,
        &t.id,
        TaskPatch {
            labels: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(u.labels.is_empty());
}

// ------------------------------------------------------------ move and aliases

#[test]
fn move_gives_new_key_and_keeps_old_as_alias() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t = e.new_task(&a, "t");
    e.new_task(&b, "existing");
    let moved = move_to_category(&mut e.db, &t.id, &b).unwrap();
    assert_eq!(
        (moved.key.as_str(), moved.category_id.as_str()),
        ("BBB-2", b.as_str())
    );
    // Resolves by id, new key, old key (any case).
    for k in [t.id.as_str(), "BBB-2", "AAA-1", "aaa-1"] {
        assert_eq!(get(e.db.conn(), k).unwrap().id, t.id, "{k}");
    }
    let found = get_by_keys(
        e.db.conn(),
        &[
            "aaa-1".into(),
            "BBB-2".into(),
            "ZZZ-9".into(),
            "BBB-1".into(),
        ],
    )
    .unwrap();
    assert_eq!(found.len(), 2, "same task once, unknown skipped");
    assert_eq!(found[0].id, t.id);
    // Old number is not reused in the old category.
    assert_eq!(e.new_task(&a, "next").key, "AAA-2");
    // Moving back restores the task's own old key; the key it leaves stays an alias.
    let back = move_to_category(&mut e.db, &t.id, &a).unwrap();
    assert_eq!(back.key, "AAA-1");
    assert_eq!(get(e.db.conn(), "BBB-2").unwrap().id, t.id);
    assert_eq!(get(e.db.conn(), "AAA-1").unwrap().id, t.id);
    // The category's counter is untouched by the return.
    assert_eq!(e.new_task(&a, "after").key, "AAA-3");
    // Version holds the new category; same-category move is a no-op.
    let v = versions(e.db.conn(), &t.id).unwrap();
    assert_eq!(v.last().unwrap().snapshot.category_id, a);
    assert_eq!(move_to_category(&mut e.db, &t.id, &a).unwrap().key, "AAA-1");
    assert!(matches!(
        move_to_category(&mut e.db, &t.id, "nope"),
        Err(CoreError::NotFound { .. })
    ));
}

/// FR-TASK-UI-6: back and forth keeps reusing each category's own key, with no new numbers.
#[test]
fn moving_back_and_forth_reuses_keys() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t = e.new_task(&a, "t");
    let keys: Vec<String> = [&b, &a, &b, &a]
        .into_iter()
        .map(|p| move_to_category(&mut e.db, &t.id, p).unwrap().key)
        .collect();
    assert_eq!(keys, ["BBB-1", "AAA-1", "BBB-1", "AAA-1"]);
    // Exactly one alias (the key in the other category) remains.
    let aliases: Vec<String> =
        e.db.conn()
            .prepare("SELECT key FROM task_key_aliases WHERE task_id = ?1")
            .unwrap()
            .query_map([&t.id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
    assert_eq!(aliases, ["BBB-1"]);
    assert_eq!(e.new_task(&b, "next").key, "BBB-2");
}

/// With several old keys in the target category (e.g. from before this rule), the lowest wins.
#[test]
fn moving_back_picks_the_lowest_old_key() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t = e.new_task(&a, "t");
    move_to_category(&mut e.db, &t.id, &b).unwrap();
    e.db.conn()
        .execute(
            "INSERT INTO task_key_aliases (key, task_id) VALUES ('aaa-7', ?1)",
            [&t.id],
        )
        .unwrap();
    let back = move_to_category(&mut e.db, &t.id, &a).unwrap();
    assert_eq!(back.key, "AAA-1");
    assert_eq!(
        get(e.db.conn(), "AAA-7").unwrap().id,
        t.id,
        "the other old key still resolves"
    );
}

#[test]
fn move_keeps_links() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let p = e.new_task(&a, "parent");
    let c = e.new_task(&a, "child");
    add_link(&mut e.db, TaskLinkType::Parent, &p.id, &c.id).unwrap();
    move_to_category(&mut e.db, &p.id, &b).unwrap();
    move_to_category(&mut e.db, &c.id, &b).unwrap();
    assert_eq!(links(e.db.conn(), &p.id).unwrap().len(), 1);
}

// ------------------------------------------------------------ comments

#[test]
fn comments_crud() {
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "t");
    invalid(add_comment(&mut e.db, &t.id, "  \n"));
    assert!(matches!(
        add_comment(&mut e.db, "nope", "x"),
        Err(CoreError::NotFound { .. })
    ));
    let c1 = add_comment(&mut e.db, "AAA-1", "first").unwrap();
    let c2 = add_comment(&mut e.db, &t.id, "second 中文").unwrap();
    assert_eq!(c1.task_id, t.id);
    let list = comments(e.db.conn(), &t.id).unwrap();
    assert_eq!(
        list.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        vec!["first", "second 中文"]
    );
    assert_eq!(list[0].updated_at, None);
    let edited = update_comment(&mut e.db, &c1.id, "first!").unwrap();
    assert_eq!(edited.text, "first!");
    assert!(edited.updated_at.is_some());
    invalid(update_comment(&mut e.db, &c1.id, ""));
    delete_comment(&mut e.db, &c2.id).unwrap();
    assert_eq!(comments(e.db.conn(), &t.id).unwrap().len(), 1);
    assert!(matches!(
        delete_comment(&mut e.db, &c2.id),
        Err(CoreError::NotFound { .. })
    ));
    assert!(matches!(
        update_comment(&mut e.db, &c2.id, "x"),
        Err(CoreError::NotFound { .. })
    ));
}

// ------------------------------------------------------------ links

#[test]
fn relates_links_are_symmetric_and_unique() {
    let mut e = env();
    let a = e.a.clone();
    let (x, y, z) = (
        e.new_task(&a, "x"),
        e.new_task(&a, "y"),
        e.new_task(&a, "z"),
    );
    let l = add_link(&mut e.db, TaskLinkType::Relates, &x.id, &y.id).unwrap();
    assert_eq!(l.link_type, TaskLinkType::Relates);
    conflict(add_link(&mut e.db, TaskLinkType::Relates, &x.id, &y.id));
    conflict(add_link(&mut e.db, TaskLinkType::Relates, &y.id, &x.id));
    assert!(invalid(add_link(&mut e.db, TaskLinkType::Relates, &x.id, &x.id)).contains("itself"));
    add_link(&mut e.db, TaskLinkType::Relates, &z.id, &x.id).unwrap();
    // The link shows on both tasks.
    assert_eq!(links(e.db.conn(), &x.id).unwrap().len(), 2);
    assert_eq!(links(e.db.conn(), &y.id).unwrap().len(), 1);
    assert_eq!(links(e.db.conn(), &z.id).unwrap().len(), 1);
    // A relates link next to a parent link of the same pair is allowed.
    add_link(&mut e.db, TaskLinkType::Parent, &x.id, &y.id).unwrap();
    remove_link(&mut e.db, &l.id).unwrap();
    assert_eq!(links(e.db.conn(), &y.id).unwrap().len(), 1);
    assert!(matches!(
        remove_link(&mut e.db, &l.id),
        Err(CoreError::NotFound { .. })
    ));
    assert!(matches!(
        add_link(&mut e.db, TaskLinkType::Relates, &x.id, "nope"),
        Err(CoreError::NotFound { .. })
    ));
}

#[test]
fn parent_links_one_parent_one_level() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let p = e.new_task(&a, "parent");
    let p2 = e.new_task(&a, "parent2");
    let c = e.new_task(&b, "child in other category");
    let g = e.new_task(&a, "grandchild");
    let l = add_link(&mut e.db, TaskLinkType::Parent, &p.id, &c.id).unwrap();
    assert_eq!(
        (l.from_task_id.as_str(), l.to_task_id.as_str()),
        (p.id.as_str(), c.id.as_str())
    );
    // Duplicate, second parent, reverse.
    conflict(add_link(&mut e.db, TaskLinkType::Parent, &p.id, &c.id));
    assert!(conflict(add_link(&mut e.db, TaskLinkType::Parent, &p2.id, &c.id)).contains("parent"));
    // A sub-task can't have sub-tasks.
    assert!(invalid(add_link(&mut e.db, TaskLinkType::Parent, &c.id, &g.id)).contains("sub-task"));
    invalid(add_link(&mut e.db, TaskLinkType::Parent, &c.id, &p.id));
    // A task with sub-tasks can't become a sub-task.
    assert!(
        invalid(add_link(&mut e.db, TaskLinkType::Parent, &p2.id, &p.id)).contains("sub-tasks")
    );
    invalid(add_link(&mut e.db, TaskLinkType::Parent, &p.id, &p.id));
    // A parent can have several sub-tasks; removing frees the sub-task.
    add_link(&mut e.db, TaskLinkType::Parent, &p.id, &g.id).unwrap();
    remove_link(&mut e.db, &l.id).unwrap();
    add_link(&mut e.db, TaskLinkType::Parent, &p2.id, &c.id).unwrap();
}

// ------------------------------------------------------------ delete

#[test]
fn delete_removes_everything() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t = create(
        &mut e.db,
        NewTask {
            category_id: a.clone(),
            title: "t".into(),
            labels: Some(vec!["x".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    let other = e.new_task(&a, "other");
    move_to_category(&mut e.db, &t.id, &b).unwrap();
    add_comment(&mut e.db, &t.id, "c").unwrap();
    add_link(&mut e.db, TaskLinkType::Relates, &t.id, &other.id).unwrap();
    delete(&mut e.db, "AAA-1").unwrap(); // by old key
    let count = |table: &str| -> i64 {
        e.db.conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(count("tasks"), 1);
    for table in [
        "task_labels",
        "task_key_aliases",
        "task_links",
        "task_comments",
        "task_versions",
    ] {
        let n: i64 =
            e.db.conn()
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {table} WHERE {} = ?1",
                        if table == "task_links" {
                            "from_task_id"
                        } else {
                            "task_id"
                        }
                    ),
                    [&t.id],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    assert!(
        activity(e.db.conn(), 100)
            .unwrap()
            .iter()
            .all(|a| a.ref_id != t.id)
    );
    assert!(matches!(
        get(e.db.conn(), "AAA-1"),
        Err(CoreError::NotFound { .. })
    ));
    assert!(matches!(
        delete(&mut e.db, &t.id),
        Err(CoreError::NotFound { .. })
    ));
    assert_eq!(links(e.db.conn(), &other.id).unwrap().len(), 0);
    // The deleted number stays used.
    assert_eq!(e.new_task(&b, "n").key, "BBB-2");
}

// ------------------------------------------------------------ list

struct Fixture {
    e: Env,
}

fn fixture() -> Fixture {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let mk = |e: &mut Env,
              p: &str,
              title: &str,
              desc: &str,
              st: TaskStatus,
              pr: Priority,
              ty: TaskType,
              due: Option<&str>,
              labels: &[&str]| {
        create(
            &mut e.db,
            NewTask {
                category_id: p.into(),
                title: title.into(),
                description: Some(desc.into()),
                status: Some(st),
                priority: Some(pr),
                task_type: Some(ty),
                due_date: due.map(Into::into),
                labels: Some(labels.iter().map(|s| s.to_string()).collect()),
            },
        )
        .unwrap()
    };
    use Priority::*;
    use TaskStatus::*;
    use TaskType::*;
    mk(
        &mut e,
        &a,
        "Login page",
        "Uses OAuth",
        Todo,
        Medium,
        Feature,
        Some("2026-10-12"),
        &["ui"],
    ); // AAA-1 overdue (Mon last... today is Wed 14th)
    mk(
        &mut e,
        &a,
        "修复登录错误",
        "用户无法登录",
        InProgress,
        High,
        Bug,
        Some("2026-10-14"),
        &["bug", "后端"],
    ); // AAA-2 today
    mk(
        &mut e,
        &a,
        "Release notes",
        "",
        Blocked,
        Low,
        General,
        Some("2026-10-18"),
        &[],
    ); // AAA-3 Sunday of this week
    mk(
        &mut e,
        &a,
        "Old thing",
        "ÉCOLE",
        Done,
        High,
        General,
        Some("2026-10-01"),
        &["ui"],
    ); // AAA-4 closed
    mk(
        &mut e,
        &b,
        "No date",
        "login too",
        Todo,
        High,
        Bug,
        None,
        &["UI"],
    ); // BBB-1
    mk(
        &mut e,
        &b,
        "Next week",
        "",
        Todo,
        Low,
        Feature,
        Some("2026-10-19"),
        &[],
    ); // BBB-2
    mk(
        &mut e,
        &b,
        "Dropped",
        "",
        WontDo,
        Medium,
        General,
        Some("2026-10-05"),
        &[],
    ); // BBB-3
    Fixture { e }
}

fn keys(f: &Fixture, filter: TaskFilter) -> Vec<String> {
    list_on(f.e.db.conn(), &filter, d("2026-10-14"))
        .unwrap()
        .into_iter()
        .map(|t| t.key)
        .collect()
}

#[test]
fn list_hides_closed_unless_asked() {
    let f = fixture();
    assert_eq!(keys(&f, TaskFilter::default()).len(), 5);
    let all = keys(
        &f,
        TaskFilter {
            include_closed: Some(true),
            ..Default::default()
        },
    );
    assert_eq!(all.len(), 7);
    let explicit = keys(
        &f,
        TaskFilter {
            statuses: Some(vec![TaskStatus::Done]),
            include_closed: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(explicit, vec!["AAA-4"]);
    let empty_means_any = keys(
        &f,
        TaskFilter {
            statuses: Some(vec![]),
            category_ids: Some(vec![]),
            ..Default::default()
        },
    );
    assert_eq!(empty_means_any.len(), 5);
}

#[test]
fn list_filters() {
    let f = fixture();
    let a = f.e.a.clone();
    let k = |filter: TaskFilter| {
        let mut v = keys(
            &f,
            TaskFilter {
                include_closed: Some(true),
                sort: Some(TaskSortField::Key),
                ..filter
            },
        );
        v.sort();
        v
    };
    assert_eq!(
        k(TaskFilter {
            category_ids: Some(vec![a.clone()]),
            ..Default::default()
        })
        .len(),
        4
    );
    assert_eq!(
        k(TaskFilter {
            types: Some(vec![TaskType::Bug]),
            ..Default::default()
        }),
        vec!["AAA-2", "BBB-1"]
    );
    assert_eq!(
        k(TaskFilter {
            priorities: Some(vec![Priority::High]),
            ..Default::default()
        }),
        vec!["AAA-2", "AAA-4", "BBB-1"]
    );
    assert_eq!(
        k(TaskFilter {
            statuses: Some(vec![TaskStatus::Todo, TaskStatus::Blocked]),
            ..Default::default()
        }),
        vec!["AAA-1", "AAA-3", "BBB-1", "BBB-2"]
    );
    // Labels: any of, case-insensitive, trimmed.
    assert_eq!(
        k(TaskFilter {
            labels: Some(vec![" UI ".into()]),
            ..Default::default()
        }),
        vec!["AAA-1", "AAA-4", "BBB-1"]
    );
    assert_eq!(
        k(TaskFilter {
            labels: Some(vec!["bug".into(), "后端".into()]),
            ..Default::default()
        }),
        vec!["AAA-2"]
    );
    assert_eq!(
        k(TaskFilter {
            labels: Some(vec!["ui".into(), "bug".into()]),
            ..Default::default()
        })
        .len(),
        4
    );
    assert_eq!(
        k(TaskFilter {
            labels: Some(vec![]),
            ..Default::default()
        })
        .len(),
        7
    );
    // Combined.
    assert_eq!(
        k(TaskFilter {
            category_ids: Some(vec![a]),
            labels: Some(vec!["ui".into()]),
            priorities: Some(vec![Priority::High]),
            ..Default::default()
        }),
        vec!["AAA-4"]
    );
}

#[test]
fn list_due_filters() {
    let f = fixture();
    // Today is Wednesday 2026-10-14; the week is 12..=18 October.
    let due = |due, closed| {
        let mut v = keys(
            &f,
            TaskFilter {
                due: Some(due),
                include_closed: Some(closed),
                ..Default::default()
            },
        );
        v.sort();
        v
    };
    // Overdue = open and due before today (closed tasks never count).
    assert_eq!(due(DueFilter::Overdue, true), vec!["AAA-1"]);
    assert_eq!(
        due(DueFilter::ThisWeek, false),
        vec!["AAA-1", "AAA-2", "AAA-3"]
    );
    assert_eq!(due(DueFilter::None, false), vec!["BBB-1"]);
    assert_eq!(due(DueFilter::Today, false), vec!["AAA-2"]);
    // One exact day (the Home calendar): closed tasks only when asked for.
    let on = |day: &str, closed| {
        keys(
            &f,
            TaskFilter {
                due_on: Some(day.into()),
                include_closed: Some(closed),
                ..Default::default()
            },
        )
    };
    assert_eq!(on("2026-10-18", false), vec!["AAA-3"]);
    assert_eq!(on("2026-10-01", false), Vec::<String>::new());
    assert_eq!(on("2026-10-01", true), vec!["AAA-4"]);
    // Sunday 18th is in, Monday 19th is out; a Sunday "today" still uses the week that ended on it.
    let sunday = list_on(
        f.e.db.conn(),
        &TaskFilter {
            due: Some(DueFilter::ThisWeek),
            ..Default::default()
        },
        d("2026-10-18"),
    )
    .unwrap();
    assert_eq!(sunday.len(), 3);
    let monday = list_on(
        f.e.db.conn(),
        &TaskFilter {
            due: Some(DueFilter::ThisWeek),
            ..Default::default()
        },
        d("2026-10-19"),
    )
    .unwrap();
    assert_eq!(
        monday.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(),
        vec!["BBB-2"]
    );
}

#[test]
fn list_text_search() {
    let f = fixture();
    let t = |s: &str| {
        let mut v = keys(
            &f,
            TaskFilter {
                text: Some(s.into()),
                include_closed: Some(true),
                sort: Some(TaskSortField::Key),
                ..Default::default()
            },
        );
        v.sort();
        v
    };
    assert_eq!(t("login"), vec!["AAA-1", "BBB-1"]); // title and description, case-insensitive
    assert_eq!(t("LOGIN"), vec!["AAA-1", "BBB-1"]);
    assert_eq!(t("登录"), vec!["AAA-2"]);
    assert_eq!(t("无法"), vec!["AAA-2"]); // description only
    assert_eq!(t("aaa-3"), vec!["AAA-3"]); // key
    assert_eq!(t("école"), vec!["AAA-4"]); // non-ASCII case folding
    assert_eq!(
        t("  "),
        keys(
            &f,
            TaskFilter {
                include_closed: Some(true),
                sort: Some(TaskSortField::Key),
                ..Default::default()
            }
        )
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
    );
    assert!(t("zzz").is_empty());
    // D-52: every word must appear, in any order, across key, title and description.
    assert_eq!(t("login page"), vec!["AAA-1"]);
    assert_eq!(t("page login"), vec!["AAA-1"]);
    assert_eq!(t("  PAGE   oauth "), vec!["AAA-1"]); // title + description
    assert_eq!(t("aaa-1 oauth"), vec!["AAA-1"]); // key + description
    assert!(t("login zzz").is_empty()); // one missing word fails the row
    assert_eq!(t("login too"), vec!["BBB-1"]);
    // A run of Han characters without spaces is one word, matched as a substring.
    assert_eq!(t("登录错误"), vec!["AAA-2"]);
    assert!(t("错误登录").is_empty());
    assert_eq!(t("登录 无法"), vec!["AAA-2"]);
    assert_eq!(t("无法 登录"), vec!["AAA-2"]);
    assert_eq!(t("修复 aaa-2"), vec!["AAA-2"]);
    assert!(t("登录 oauth").is_empty());
    // SQL metacharacters are plain text.
    assert!(t("%").is_empty());
    assert!(t("'; DROP TABLE tasks; --").is_empty());
    assert_eq!(
        keys(
            &f,
            TaskFilter {
                text: Some("login".into()),
                include_closed: Some(true),
                limit: Some(1),
                ..Default::default()
            }
        )
        .len(),
        1
    );
}

#[test]
fn list_sorting_and_limit() {
    let f = fixture();
    let s = |sort, descending: bool| {
        keys(
            &f,
            TaskFilter {
                sort: Some(sort),
                descending: Some(descending),
                ..Default::default()
            },
        )
    };
    // Due: soonest first, no date last.
    assert_eq!(
        s(TaskSortField::Due, false),
        vec!["AAA-1", "AAA-2", "AAA-3", "BBB-2", "BBB-1"]
    );
    assert_eq!(
        s(TaskSortField::Due, true),
        vec!["BBB-1", "BBB-2", "AAA-3", "AAA-2", "AAA-1"]
    );
    // Default sort is due; priority high first with a stable tie-breaker (creation order).
    assert_eq!(
        keys(&f, TaskFilter::default()),
        s(TaskSortField::Due, false)
    );
    assert_eq!(
        s(TaskSortField::Priority, false),
        vec!["AAA-2", "BBB-1", "AAA-1", "AAA-3", "BBB-2"]
    );
    assert_eq!(
        s(TaskSortField::Priority, true),
        vec!["AAA-3", "BBB-2", "AAA-1", "AAA-2", "BBB-1"]
    );
    assert_eq!(
        s(TaskSortField::Key, false),
        vec!["AAA-1", "AAA-2", "AAA-3", "BBB-1", "BBB-2"]
    );
    assert_eq!(
        s(TaskSortField::Created, true),
        vec!["BBB-2", "BBB-1", "AAA-3", "AAA-2", "AAA-1"]
    );
    assert_eq!(
        keys(
            &f,
            TaskFilter {
                limit: Some(2),
                ..Default::default()
            }
        ),
        vec!["AAA-1", "AAA-2"]
    );
}

#[test]
fn key_sort_is_numeric() {
    let mut e = env();
    let a = e.a.clone();
    for i in 0..11 {
        e.new_task(&a, &format!("t{i}"));
    }
    let v = list_on(
        e.db.conn(),
        &TaskFilter {
            sort: Some(TaskSortField::Key),
            ..Default::default()
        },
        d("2026-10-14"),
    )
    .unwrap();
    assert_eq!(v[1].key, "AAA-2");
    assert_eq!(v[9].key, "AAA-10");
    assert_eq!(v[10].key, "AAA-11");
}

#[test]
fn updated_sort_follows_changes() {
    let mut e = env();
    let a = e.a.clone();
    let t1 = e.new_task(&a, "1");
    let _t2 = e.new_task(&a, "2");
    std::thread::sleep(std::time::Duration::from_millis(5));
    update(
        &mut e.db,
        &t1.id,
        TaskPatch {
            title: Some("1b".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let v = list(
        e.db.conn(),
        &TaskFilter {
            sort: Some(TaskSortField::Updated),
            descending: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(v[0].id, t1.id);
}

#[test]
fn list_is_fast_with_20000_tasks() {
    let mut e = env();
    let a = e.a.clone();
    e.db.tx(|tx| {
        for i in 0..20_000 {
            create_in(
                tx,
                NewTask {
                    category_id: a.clone(),
                    title: format!("task {i} 标题"),
                    labels: Some(vec![format!("l{}", i % 50)]),
                    due_date: Some(format!("2026-{:02}-{:02}", 1 + i % 12, 1 + i % 28)),
                    ..Default::default()
                },
                &Stamp::now(),
            )?;
        }
        Ok(())
    })
    .unwrap();
    let start = std::time::Instant::now();
    let v = list(
        e.db.conn(),
        &TaskFilter {
            labels: Some(vec!["l7".into()]),
            limit: Some(50),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(v.len(), 50);
    let all = list(
        e.db.conn(),
        &TaskFilter {
            text: Some("标题".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(all.len(), 20_000);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

// ------------------------------------------------------------ working days

#[test]
fn working_days_inclusive_monday_to_friday() {
    // 2026-10-12 is a Monday.
    assert_eq!(
        working_days_between(d("2026-10-12"), d("2026-10-12")),
        1,
        "same day"
    );
    assert_eq!(working_days_between(d("2026-10-12"), d("2026-10-16")), 5);
    assert_eq!(
        working_days_between(d("2026-10-12"), d("2026-10-18")),
        5,
        "weekend excluded"
    );
    assert_eq!(working_days_between(d("2026-10-12"), d("2026-10-19")), 6);
    assert_eq!(
        working_days_between(d("2026-10-16"), d("2026-10-19")),
        2,
        "Fri..Mon"
    );
    assert_eq!(
        working_days_between(d("2026-10-17"), d("2026-10-18")),
        0,
        "weekend only"
    );
    assert_eq!(
        working_days_between(d("2026-10-14"), d("2026-10-12")),
        0,
        "end before start"
    );
}

#[test]
fn blocked_days_carry_forward() {
    use TaskStatus::*;
    let (start, end) = (d("2026-10-12"), d("2026-10-23")); // Mon..Fri of the next week
    let v = |s: &str, st| (d(s), st);
    // No history: never blocked.
    assert_eq!(blocked_working_days(start, end, &[]), 0);
    // Blocked from Wed 14 (end of day), unblocked Tue 20: Wed, Thu, Fri, Mon (weekend skipped) = 4.
    assert_eq!(
        blocked_working_days(
            start,
            end,
            &[v("2026-10-14", Blocked), v("2026-10-20", InProgress)]
        ),
        4
    );
    // Order of the input doesn't matter.
    assert_eq!(
        blocked_working_days(
            start,
            end,
            &[v("2026-10-20", InProgress), v("2026-10-14", Blocked)]
        ),
        4
    );
    // A version before the start sets the initial status.
    assert_eq!(
        blocked_working_days(
            start,
            end,
            &[v("2026-10-01", Blocked), v("2026-10-14", Todo)]
        ),
        2
    );
    // Blocked and still blocked on the last day (done that day would be a different version).
    assert_eq!(
        blocked_working_days(start, end, &[v("2026-10-09", Blocked)]),
        10
    );
    // Versions after the end don't matter; versions on weekends carry into Monday.
    assert_eq!(
        blocked_working_days(
            start,
            end,
            &[v("2026-10-17", Blocked), v("2026-10-30", Todo)]
        ),
        5
    );
}

#[test]
fn completion_summary_for_done_tasks() {
    let mut e = env();
    let a = e.a.clone();
    let t = create_at(&mut e, &a, d("2026-10-12")); // Monday
    assert_eq!(
        completion_summary(e.db.conn(), &t.id).unwrap(),
        None,
        "not done"
    );
    // Wed: blocked; Thu: in progress; Fri 16: done. Blocked at the end of Wed only.
    e.set_status(&t.id, TaskStatus::Blocked, &Stamp::on(d("2026-10-14")));
    e.set_status(&t.id, TaskStatus::InProgress, &Stamp::on(d("2026-10-15")));
    e.set_status(&t.id, TaskStatus::Done, &Stamp::on(d("2026-10-16")));
    assert_eq!(
        completion_summary(e.db.conn(), "AAA-1").unwrap(),
        Some(CompletionSummary {
            working_days: 5,
            blocked_days: 2 - 1
        })
    );
}

#[test]
fn completion_summary_same_day_is_one_and_wont_do_is_none() {
    let mut e = env();
    let a = e.a.clone();
    let t = create_at(&mut e, &a, d("2026-10-13"));
    e.set_status(&t.id, TaskStatus::Done, &Stamp::on(d("2026-10-13")));
    assert_eq!(
        completion_summary(e.db.conn(), &t.id).unwrap(),
        Some(CompletionSummary {
            working_days: 1,
            blocked_days: 0
        })
    );
    let u = create_at(&mut e, &a, d("2026-10-13"));
    e.set_status(&u.id, TaskStatus::WontDo, &Stamp::on(d("2026-10-14")));
    assert_eq!(completion_summary(e.db.conn(), &u.id).unwrap(), None);
    assert!(matches!(
        completion_summary(e.db.conn(), "nope"),
        Err(CoreError::NotFound { .. })
    ));
}

#[test]
fn completion_summary_with_version_before_start_and_reopen() {
    let mut e = env();
    let a = e.a.clone();
    // Created Friday 9th (blocked at once), reopened and finished Tue 13th after the weekend.
    let t = create_at(&mut e, &a, d("2026-10-09"));
    e.set_status(&t.id, TaskStatus::Blocked, &Stamp::on(d("2026-10-09")));
    e.set_status(&t.id, TaskStatus::Done, &Stamp::on(d("2026-10-13")));
    // Fri blocked (1), weekend skipped, Mon blocked (carried, 1), Tue done.
    assert_eq!(
        completion_summary(e.db.conn(), &t.id).unwrap(),
        Some(CompletionSummary {
            working_days: 3,
            blocked_days: 2
        })
    );
}

fn create_at(e: &mut Env, category: &str, day: NaiveDate) -> Task {
    let st = Stamp::on(day);
    let p = category.to_string();
    e.at(|c| {
        create_in(
            c,
            NewTask {
                category_id: p,
                title: "t".into(),
                ..Default::default()
            },
            &st,
        )
    })
}

// ------------------------------------------------------------ seed

#[test]
fn seed_is_idempotent_and_never_overwrites() {
    let mut e = env();
    let a = e.a.clone();
    let task = |id: &str, key: &str, category: &str| Task {
        id: id.into(),
        key: key.into(),
        category_id: category.into(),
        title: format!("seed {id}"),
        task_type: TaskType::Bug,
        description: "d".into(),
        status: TaskStatus::Done,
        priority: Priority::High,
        labels: vec!["z".into(), "a".into()],
        due_date: Some("2026-01-01".into()),
        created_at: "2026-01-01T00:00:00.000Z".into(),
        updated_at: "2026-01-02T00:00:00.000Z".into(),
        completed_at: Some("2026-01-02T00:00:00.000Z".into()),
    };
    let snap = |status| TaskSnapshot {
        title: "s".into(),
        task_type: TaskType::Bug,
        description: String::new(),
        status,
        priority: Priority::High,
        labels: vec![],
        due_date: None,
        category_id: a.clone(),
    };
    let seed = || TaskSeed {
        tasks: vec![
            task("t-1", "AAA-7", &a),
            task("t-2", "AAA-12", &a),
            task("t-orphan", "XXX-1", "no-such-category"),
        ],
        links: vec![
            TaskLink {
                id: "l-1".into(),
                link_type: TaskLinkType::Parent,
                from_task_id: "t-1".into(),
                to_task_id: "t-2".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
            },
            TaskLink {
                id: "l-bad".into(),
                link_type: TaskLinkType::Relates,
                from_task_id: "t-1".into(),
                to_task_id: "ghost".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
            },
            TaskLink {
                id: "l-self".into(),
                link_type: TaskLinkType::Relates,
                from_task_id: "t-1".into(),
                to_task_id: "t-1".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
            },
        ],
        comments: vec![
            TaskComment {
                id: "c-1".into(),
                task_id: "t-1".into(),
                text: "hi".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: None,
            },
            TaskComment {
                id: "c-bad".into(),
                task_id: "ghost".into(),
                text: "hi".into(),
                created_at: "2026-01-01T00:00:00.000Z".into(),
                updated_at: None,
            },
        ],
        history: vec![
            TaskVersion {
                task_id: "t-1".into(),
                date: "2026-01-01".into(),
                snapshot: snap(TaskStatus::Todo),
            },
            TaskVersion {
                task_id: "t-1".into(),
                date: "2026-01-02".into(),
                snapshot: snap(TaskStatus::Done),
            },
            TaskVersion {
                task_id: "ghost".into(),
                date: "2026-01-02".into(),
                snapshot: snap(TaskStatus::Done),
            },
        ],
        activity: vec![ActivityEvent {
            id: "a-1".into(),
            kind: ActivityKind::TaskCompleted,
            ref_id: "t-1".into(),
            at: "2026-01-02T00:00:00.000Z".into(),
        }],
    };
    seed_if_missing(&mut e.db, seed()).unwrap();
    let dump = |e: &Env| -> Vec<i64> {
        [
            "tasks",
            "task_labels",
            "task_links",
            "task_comments",
            "task_versions",
            "activity",
        ]
        .iter()
        .map(|t| {
            e.db.conn()
                .query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect()
    };
    assert_eq!(dump(&e), vec![2, 4, 1, 1, 2, 1]);
    // Edit seeded data, seed again: nothing is overwritten or duplicated.
    update(
        &mut e.db,
        "t-1",
        TaskPatch {
            title: Some("edited".into()),
            ..Default::default()
        },
    )
    .unwrap();
    seed_if_missing(&mut e.db, seed()).unwrap();
    assert_eq!(get(e.db.conn(), "t-1").unwrap().title, "edited");
    assert_eq!(get(e.db.conn(), "AAA-12").unwrap().labels, vec!["a", "z"]);
    let n = dump(&e);
    assert_eq!(
        &n[..5],
        &[2, 4, 1, 1, 3][..5].iter().map(|x| *x).collect::<Vec<_>>()[..],
        "edit added today's version only"
    );
    // New tasks continue after the highest seeded number.
    assert_eq!(e.new_task(&a, "fresh").key, "AAA-13");
}

// ---------------------------------------------------------------- many at once (FR-TASK-UI-7)

#[test]
fn many_tasks_change_together_and_skips_are_named() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let t1 = e.new_task(&a, "one");
    let t2 = e.new_task(&a, "two");
    let ids = vec![
        t1.id.clone(),
        t2.id.clone(),
        t1.id.clone(),
        "gone".to_string(),
    ];

    let r = update_many(
        &mut e.db,
        &ids,
        TaskBulkChange::Status {
            status: TaskStatus::Done,
        },
    )
    .unwrap();
    assert_eq!(r.done, 2, "a repeated id counts once");
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(
        (r.skipped[0].id.as_str(), r.skipped[0].title.as_str()),
        ("gone", "")
    );
    for id in [&t1.id, &t2.id] {
        let t = get(e.db.conn(), id).unwrap();
        assert_eq!(t.status, TaskStatus::Done);
        assert!(t.completed_at.is_some());
    }

    let both = vec![t1.id.clone(), t2.id.clone()];
    update_many(
        &mut e.db,
        &both,
        TaskBulkChange::Priority {
            priority: Priority::High,
        },
    )
    .unwrap();
    update_many(
        &mut e.db,
        &both,
        TaskBulkChange::AddLabel {
            label: " Urgent ".into(),
        },
    )
    .unwrap();
    update_many(
        &mut e.db,
        &both,
        TaskBulkChange::AddLabel {
            label: "urgent".into(),
        },
    )
    .unwrap();
    let t = get(e.db.conn(), &t1.id).unwrap();
    assert_eq!(
        (t.priority, t.labels.clone()),
        (Priority::High, vec!["Urgent".to_string()])
    );
    update_many(
        &mut e.db,
        &both,
        TaskBulkChange::RemoveLabel {
            label: "URGENT".into(),
        },
    )
    .unwrap();
    assert!(get(e.db.conn(), &t2.id).unwrap().labels.is_empty());
    assert!(
        invalid(update_many(
            &mut e.db,
            &both,
            TaskBulkChange::AddLabel { label: " ".into() }
        ))
        .contains("label")
    );

    // Moving gives new keys, as one move does.
    let r = update_many(
        &mut e.db,
        &both,
        TaskBulkChange::Category {
            category_id: b.clone(),
        },
    )
    .unwrap();
    assert_eq!(r.done, 2);
    let keys: Vec<String> = both
        .iter()
        .map(|id| get(e.db.conn(), id).unwrap().key)
        .collect();
    assert_eq!(keys, ["BBB-1", "BBB-2"]);

    // An archived category refuses every task, each named; nothing changes.
    categories::set_archived(e.db.conn(), &a, true).unwrap();
    let r = update_many(
        &mut e.db,
        &both,
        TaskBulkChange::Category {
            category_id: a.clone(),
        },
    )
    .unwrap();
    assert_eq!(r.done, 0);
    assert_eq!(r.skipped[0].title, "BBB-1 one");
    assert!(
        r.skipped[0].reason.contains("archived"),
        "{}",
        r.skipped[0].reason
    );
    assert_eq!(get(e.db.conn(), &t1.id).unwrap().key, "BBB-1");

    let r = delete_many(&mut e.db, &both).unwrap();
    assert_eq!((r.done, r.skipped.len()), (2, 0));
    assert!(get(e.db.conn(), &t1.id).is_err() && get(e.db.conn(), &t2.id).is_err());
}

#[test]
fn a_skipped_task_keeps_everything_it_had() {
    // The savepoint puts back a task whose change failed halfway, so nothing of it is kept.
    let mut e = env();
    let a = e.a.clone();
    let t = e.new_task(&a, "kept");
    let before = get(e.db.conn(), &t.id).unwrap();
    let r = e.at(|c| {
        crate::bulk::each(
            c,
            std::slice::from_ref(&t.id),
            |_, _| None,
            |c, id| {
                c.execute("UPDATE tasks SET title = 'changed' WHERE id = ?1", [id])?;
                Err(CoreError::Conflict("stop".into()))
            },
        )
    });
    assert_eq!((r.done, r.skipped[0].reason.as_str()), (0, "stop"));
    assert_eq!(get(e.db.conn(), &t.id).unwrap(), before);
}
