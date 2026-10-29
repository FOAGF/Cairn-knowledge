use serde_json::json;

use super::*;
use crate::assistant::formats::candidates_block;
use crate::assistant::items::{ItemRef, load};
use crate::knowledge::NewPage;
use crate::categories::{NewCategory, CategoryColor};

struct Env {
    _dir: tempfile::TempDir,
    db: Db,
    a: String,
    b: String,
    t1: tasks::Task,
    t2: tasks::Task,
    page: knowledge::Page,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(dir.path()).unwrap();
    let mk = |db: &Db, name: &str, key: &str| {
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
    let a = mk(&db, "Alpha", "AAA");
    let b = mk(&db, "Beta", "BBB");
    let new = |db: &mut Db, title: &str| {
        tasks::create(
            db,
            NewTask {
                category_id: a.clone(),
                title: title.into(),
                due_date: Some("2026-10-20".into()),
                labels: Some(vec!["hose".into()]),
                ..NewTask::default()
            },
        )
        .unwrap()
    };
    let t1 = new(&mut db, "Auger hose wear");
    let t2 = new(&mut db, "Pump seal");
    let page = knowledge::create_page(
        &mut db,
        NewPage {
            category_id: a.clone(),
            title: "Pre-start".into(),
            body: "# Pre-start\n\nCheck hoses.".into(),
            ..NewPage::default()
        },
    )
    .unwrap();
    Env { _dir: dir, db, a, b, t1, t2, page }
}

impl Env {
    /// t1, t2, p1.
    fn labels(&self) -> Vec<(String, ItemInfo)> {
        let items = load(
            self.db.conn(),
            &[ItemRef::task(&self.t1.id), ItemRef::task(&self.t2.id), ItemRef::page(&self.page.id)],
        )
        .unwrap();
        let (_, map) = candidates_block(&items);
        map.into_iter().map(|(l, i)| (l, items[i].clone())).collect()
    }

    fn categories(&self) -> Vec<PlanCategory> {
        vec![
            PlanCategory { id: self.a.clone(), name: "Alpha".into(), in_scope: false },
            PlanCategory { id: self.b.clone(), name: "Beta".into(), in_scope: true },
        ]
    }

    fn read(&self, v: Value) -> ReadAction {
        match v {
            Value::Object(m) => read(&m, &self.labels(), &self.categories()),
            _ => read(&Map::new(), &self.labels(), &self.categories()),
        }
    }
}

#[test]
fn unknown_ops_targets_and_wrong_types_are_dropped() {
    let e = env();
    let r = e.read(json!({"changes": [
        {"op": "delete_task", "target": "t1"},
        {"op": "update_task", "target": "t9", "status": "done"},
        {"op": "update_task", "target": "p1", "status": "done"},
        {"op": "update_page", "target": "t1", "change": "x"},
        {"op": "comment", "target": "p1", "text": "x"},
        "nonsense",
        {"op": "update_task", "target": "t1", "status": "done"}
    ], "reply": " Marking it. "}));
    assert_eq!(r.changes.len(), 1);
    assert_eq!(r.reply, "Marking it.");
    assert_eq!(r.notes.len(), 6);
}

#[test]
fn creates_fall_back_and_clean_their_values() {
    let e = env();
    let r = e.read(json!({"changes": [
        {"op": "create_task", "category": "Nowhere", "title": " New task ", "type": "chore",
         "priority": "urgent", "due": "next week", "labels": [" a ", "", "b"], "description": " d ",
         "links": [
            {"to": "t1", "kind": "relates"}, {"to": "t1", "kind": "page"}, {"to": "p1", "kind": "page"},
            {"to": "p1", "kind": "relates"}, {"to": "t9", "kind": "relates"}, {"to": "t2", "kind": "parent"},
            {"to": "t1", "kind": "relates"}],
         "terms": [{"term": "MMU", "meaning": "x", "confidence": "high"}, {"term": ""}]},
        {"op": "create_task", "category": "alpha", "title": "  "},
        {"op": "create_page", "category": "BETA", "title": "Hose guide", "content": "from t1"},
        {"op": "create_page", "category": "Beta", "title": "Second"}
    ]}));
    let ProposedChange::CreateTask {
        category_id, title, task_type, priority, due, labels, description, links,
    } = &r.changes[0]
    else {
        panic!("not a create");
    };
    assert_eq!(category_id, &e.b, "the first category in scope");
    assert_eq!((title.as_str(), description.as_str()), ("New task", "d"));
    assert_eq!((*task_type, *priority, due.clone()), (TaskType::General, Priority::Medium, None));
    assert_eq!(labels, &["a", "b"]);
    let kinds: Vec<_> = links.iter().map(|l| (l.kind, l.target_name.as_str())).collect();
    assert_eq!(
        kinds,
        [(LinkKind::Relates, "AAA-1"), (LinkKind::Page, "Pre-start"), (LinkKind::Parent, "AAA-2")]
    );
    assert_eq!(r.changes.len(), 1);
    assert_eq!(r.terms.len(), 1);
    assert_eq!((r.terms[0].0, r.terms[0].1.len()), (0, 1));
    let page = r.new_page.unwrap();
    assert_eq!((page.category_id.as_str(), page.title.as_str()), (e.b.as_str(), "Hose guide"));
}

#[test]
fn updates_keep_only_what_changes() {
    let e = env();
    let r = e.read(json!({"changes": [
        {"op": "update_task", "target": "t1", "status": "done", "priority": "medium", "type": "general",
         "due": "2026-10-20", "labels": ["HOSE"], "description": ""},
        {"op": "update_task", "target": "t2", "status": "done", "priority": "high"},
        {"op": "update_task", "target": "t2", "status": "done", "priority": "high"},
        {"op": "update_task", "target": "t1", "due": null, "links": [
            {"to": "t1", "kind": "relates"}, {"to": "t2", "kind": "relates"}]},
        {"op": "update_task", "target": "t2", "due": "soon"},
        {"op": "update_task", "target": "t1", "status": "nope"}
    ]}));
    assert_eq!(r.changes.len(), 3, "{:?}", r.notes);
    let ProposedChange::UpdateTask { status, priority, task_type, due, labels, description, .. } =
        &r.changes[0]
    else {
        panic!()
    };
    assert_eq!((*status, *priority, *task_type), (Some(TaskStatus::Done), None, None));
    assert!(due.is_none() && labels.is_none() && description.is_none());
    let ProposedChange::UpdateTask { priority, status, .. } = &r.changes[1] else { panic!() };
    assert_eq!((*status, *priority), (Some(TaskStatus::Done), Some(Priority::High)));
    let ProposedChange::UpdateTask { clear_due, due, links, .. } = &r.changes[2] else { panic!() };
    assert!(*clear_due && due.is_none());
    assert_eq!(links.len(), 1, "no link to itself");
    assert!(r.notes.iter().any(|n| n.contains("duplicate")));
    assert!(r.notes.iter().any(|n| n.contains("changes nothing")));
}

#[test]
fn dates_comments_and_page_changes() {
    let e = env();
    let r = e.read(json!({"changes": [
        {"op": "update_task", "target": "t2", "due": "2026-11-02", "labels": ["x", "hose"]},
        {"op": "comment", "target": "t1", "text": "  "},
        {"op": "comment", "target": "t1", "text": " Replaced the hose. "},
        {"op": "update_page", "target": "p1", "change": ""},
        {"op": "update_page", "target": "p1", "change": "Step 2: 10 minutes"},
        {"op": "update_page", "target": "p1", "change": "Step 2: 10 minutes"}
    ]}));
    let ProposedChange::UpdateTask { due, labels, .. } = &r.changes[0] else { panic!() };
    assert_eq!(due.as_deref(), Some("2026-11-02"));
    assert_eq!(labels.as_deref(), Some(&["x".to_string(), "hose".to_string()][..]));
    assert_eq!(
        r.changes[1],
        ProposedChange::Comment {
            task_id: e.t1.id.clone(),
            key: "AAA-1".into(),
            text: "Replaced the hose.".into()
        }
    );
    assert_eq!(r.changes.len(), 2);
    assert_eq!(
        r.page_edits,
        [PageEditRequest { page_id: e.page.id.clone(), change: "Step 2: 10 minutes".into() }]
    );
}

#[test]
fn nothing_proposed_keeps_the_question() {
    let e = env();
    let r = e.read(json!({"changes": "none", "reply": "Which task do you mean?"}));
    assert!(r.changes.is_empty() && r.new_page.is_none());
    assert_eq!(r.reply, "Which task do you mean?");
    assert_eq!(e.read(json!([])).reply, "");
}

#[test]
fn apply_creates_with_links_and_updates() {
    let mut e = env();
    let change = ProposedChange::CreateTask {
        category_id: e.a.clone(),
        title: "Order hose".into(),
        task_type: TaskType::Feature,
        priority: Priority::High,
        due: Some("2026-11-01".into()),
        labels: vec!["parts".into()],
        description: "Two metres".into(),
        links: vec![
            LinkTo { kind: LinkKind::Parent, target_id: e.t1.id.clone(), target_name: "AAA-1".into() },
            LinkTo { kind: LinkKind::Relates, target_id: e.t2.id.clone(), target_name: "AAA-2".into() },
            LinkTo { kind: LinkKind::Page, target_id: e.page.id.clone(), target_name: "Pre-start".into() },
        ],
    };
    let (line, id) = apply(&mut e.db, &change).unwrap();
    assert_eq!(line, "Created AAA-3");
    let t = tasks::get(e.db.conn(), &id.unwrap()).unwrap();
    assert_eq!(
        (t.priority, t.task_type, t.due_date.as_deref()),
        (Priority::High, TaskType::Feature, Some("2026-11-01"))
    );
    assert_eq!(t.description, "Two metres");
    assert_eq!(tasks::links(e.db.conn(), &t.id).unwrap().len(), 2);
    assert_eq!(knowledge::task_page_links(e.db.conn(), Some(&t.id), None).unwrap().len(), 1);

    let upd = |status, priority, clear_due| ProposedChange::UpdateTask {
        task_id: e.t1.id.clone(),
        key: "AAA-1".into(),
        title: "Auger hose wear".into(),
        status,
        priority,
        task_type: None,
        due: None,
        clear_due,
        labels: None,
        description: None,
        links: vec![],
    };
    let done = apply(&mut e.db, &upd(Some(TaskStatus::Done), None, false)).unwrap();
    assert_eq!(done.0, "Marked AAA-1 as done");
    let high = apply(&mut e.db, &upd(None, Some(Priority::High), false)).unwrap();
    assert_eq!(high.0, "Set AAA-1 to high priority");
    let both = apply(&mut e.db, &upd(Some(TaskStatus::Todo), Some(Priority::Low), true)).unwrap();
    assert_eq!(both.0, "Updated AAA-1");
    let t = tasks::get(e.db.conn(), &e.t1.id).unwrap();
    assert_eq!((t.status, t.priority, t.due_date), (TaskStatus::Todo, Priority::Low, None));
}

#[test]
fn apply_comments_and_pages() {
    let mut e = env();
    let comment =
        ProposedChange::Comment { task_id: e.t1.id.clone(), key: "AAA-1".into(), text: "Done on site".into() };
    let (line, id) = apply(&mut e.db, &comment).unwrap();
    assert_eq!((line.as_str(), id.as_deref()), ("Commented on AAA-1", Some(e.t1.id.as_str())));
    assert_eq!(tasks::comments(e.db.conn(), &e.t1.id).unwrap().len(), 1);

    let page = |base: &str| ProposedChange::UpdatePage {
        page_id: e.page.id.clone(),
        title: "Pre-start".into(),
        change: "x".into(),
        old_body: e.page.body.clone(),
        new_body: "# Pre-start\n\nCheck hoses and seals.".into(),
        base_updated_at: base.into(),
    };
    let stale = apply(&mut e.db, &page("2000-01-01T00:00:00Z")).unwrap_err();
    assert_eq!(stale.to_string(), "The page changed since this was proposed. Ask again.");
    assert_eq!(knowledge::get_page(e.db.conn(), &e.page.id).unwrap().body, e.page.body);
    let (line, _) = apply(&mut e.db, &page(&e.page.updated_at)).unwrap();
    assert_eq!(line, "Updated page “Pre-start”");
    assert!(knowledge::get_page(e.db.conn(), &e.page.id).unwrap().body.contains("and seals"));
}

#[test]
fn history_lines_follow_the_row_state() {
    let e = env();
    let mut row = ProposalRow {
        change: ProposedChange::Comment { task_id: e.t1.id.clone(), key: "AAA-1".into(), text: "Hi".into() },
        ticked: true,
        state: RowState::Pending,
        result: None,
        result_id: None,
    };
    assert_eq!(row_line(e.db.conn(), &row), "Proposed: Comment on AAA-1: “Hi”");
    row.state = RowState::Rejected;
    assert_eq!(row_line(e.db.conn(), &row), "Rejected: Comment on AAA-1: “Hi”");
    row.state = RowState::Done;
    row.result = Some("Commented on AAA-1".into());
    assert_eq!(row_line(e.db.conn(), &row), "Commented on AAA-1");
    row.state = RowState::Failed;
    row.result = Some("boom".into());
    assert_eq!(row_line(e.db.conn(), &row), "Failed: Comment on AAA-1: “Hi” (boom)");
}
