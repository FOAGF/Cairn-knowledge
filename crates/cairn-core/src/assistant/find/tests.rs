use super::*;
use crate::Db;
use crate::assistant::plan::DateRange;
use crate::knowledge::{self, NewPage};
use crate::tasks::{self, NewTask, TaskStatus};

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn setup() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open_at(dir.path()).unwrap();
    (dir, db)
}

fn task(db: &mut Db, title: &str, status: TaskStatus, priority: Priority, due: Option<&str>) -> String {
    tasks::create(
        db,
        NewTask {
            category_id: "p-general".into(),
            title: title.into(),
            status: Some(status),
            priority: Some(priority),
            due_date: due.map(str::to_string),
            ..NewTask::default()
        },
    )
    .unwrap()
    .id
}

fn page(db: &mut Db, title: &str) -> String {
    knowledge::create_page(
        db,
        NewPage {
            category_id: "p-general".into(),
            title: title.into(),
            body: String::new(),
            group_id: None,
            tags: None,
            source_info: None,
            origin: None,
            origin_task_id: None,
            check_status: None,
        },
    )
    .unwrap()
    .id
}

fn run(db: &Db, filters: &PlanFilters, matching: Option<&[ItemRef]>) -> Vec<String> {
    find(
        db.conn(),
        &FindQuery {
            scope_category_ids: &[],
            filters,
            matching,
            today: day("2026-10-15"),
        },
    )
    .unwrap()
    .into_iter()
    .map(|i| i.key.unwrap_or(i.title))
    .collect()
}

#[test]
fn tasks_by_due_then_priority_then_key_and_pages_by_title() {
    let (_d, mut db) = setup();
    task(&mut db, "no date", TaskStatus::Todo, Priority::High, None);
    task(&mut db, "late", TaskStatus::Todo, Priority::Low, Some("2026-10-20"));
    task(&mut db, "soon low", TaskStatus::Todo, Priority::Low, Some("2026-10-12"));
    task(&mut db, "soon high", TaskStatus::Todo, Priority::High, Some("2026-10-12"));
    page(&mut db, "Zebra");
    page(&mut db, "apple");
    let all = run(&db, &PlanFilters::default(), None);
    // GEN-1 no date, GEN-2 late, GEN-3 soon low, GEN-4 soon high.
    assert_eq!(all, ["GEN-4", "GEN-3", "GEN-2", "GEN-1", "apple", "Zebra"]);
}

#[test]
fn filters_keep_only_what_they_name() {
    let (_d, mut db) = setup();
    task(&mut db, "a", TaskStatus::Todo, Priority::High, Some("2026-10-14"));
    task(&mut db, "b", TaskStatus::Done, Priority::High, Some("2026-10-14"));
    task(&mut db, "c", TaskStatus::Todo, Priority::Low, Some("2026-11-01"));
    page(&mut db, "A page");
    let f = |f: PlanFilters| run(&db, &f, None);
    assert_eq!(
        f(PlanFilters { statuses: vec![TaskStatus::Todo], ..Default::default() }),
        ["GEN-1", "GEN-3"]
    );
    assert_eq!(
        f(PlanFilters { priorities: vec![Priority::Low], ..Default::default() }),
        ["GEN-3"]
    );
    let due = DateRange { from: Some(day("2026-10-01")), to: Some(day("2026-10-31")) };
    // A due filter means tasks only.
    assert_eq!(f(PlanFilters { due: Some(due), ..Default::default() }), ["GEN-1", "GEN-2"]);
    assert_eq!(
        f(PlanFilters { kinds: vec![SearchKind::Page], ..Default::default() }),
        ["A page"]
    );
    // Created just now, so an updated range around today keeps both kinds.
    let today = chrono::Local::now().date_naive();
    let updated = DateRange { from: Some(today), to: Some(today) };
    assert_eq!(f(PlanFilters { updated: Some(updated), ..Default::default() }).len(), 4);
    let old = DateRange { from: None, to: Some(day("2020-01-01")) };
    assert!(f(PlanFilters { updated: Some(old), ..Default::default() }).is_empty());
}

#[test]
fn topic_matches_keep_their_order_and_the_filters() {
    let (_d, mut db) = setup();
    let a = task(&mut db, "a", TaskStatus::Todo, Priority::High, None);
    let b = task(&mut db, "b", TaskStatus::Done, Priority::High, None);
    let p = page(&mut db, "P");
    let matching = [ItemRef::page(&p), ItemRef::task(&b), ItemRef::task(&a), ItemRef::task("gone")];
    let filters = PlanFilters { statuses: vec![TaskStatus::Todo], ..Default::default() };
    // The status filter drops the page and the done task.
    assert_eq!(run(&db, &filters, Some(&matching)), ["GEN-1"]);
    assert_eq!(run(&db, &PlanFilters::default(), Some(&matching)), ["P", "GEN-2", "GEN-1"]);
}
