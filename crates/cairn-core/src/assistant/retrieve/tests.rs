use super::*;
use crate::glossary::{self, NewGlossaryEntry};
use crate::knowledge::{self, NewPage};
use crate::tasks::{self, NewTask, TaskStatus};
use crate::vectors::{next_jobs, store_vectors};
use crate::{Db, categories};

const RULE: RelevanceRule = RelevanceRule {
    floor: 0.40,
    margin: 0.30,
};
const DIMS: usize = 8;

fn axis(i: usize, j: usize, mix: f32) -> Vec<f32> {
    let mut v = [0.0f32; DIMS];
    v[i] = 1.0;
    v[j] += mix;
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

/// Pieces about hoses sit on the query's axis, "tube" ones close to it, "MMU" ones a little off,
/// anything else far away.
fn fake_vector(text: &str) -> Vec<f32> {
    if text.contains("hose") {
        axis(0, 1, 0.0)
    } else if text.contains("tube") {
        axis(0, 1, 0.9)
    } else if text.contains("MMU") {
        axis(0, 1, 0.2)
    } else {
        axis(3, 4, 0.0)
    }
}

struct Env {
    _dir: tempfile::TempDir,
    db: Db,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    Env {
        db: Db::open_at(dir.path()).unwrap(),
        _dir: dir,
    }
}

impl Env {
    fn page(&mut self, title: &str, body: &str) -> String {
        knowledge::create_page(
            &mut self.db,
            NewPage {
                category_id: "p-general".into(),
                title: title.into(),
                body: body.into(),
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

    fn task(&mut self, title: &str, description: &str, status: TaskStatus) -> String {
        tasks::create(
            &mut self.db,
            NewTask {
                category_id: "p-general".into(),
                title: title.into(),
                description: Some(description.into()),
                status: Some(status),
                ..NewTask::default()
            },
        )
        .unwrap()
        .id
    }

    /// Gives every waiting piece its fake vector.
    fn embed(&self) {
        let c = self.db.conn();
        let done: Vec<_> = next_jobs(c, 1000)
            .unwrap()
            .into_iter()
            .map(|j| {
                let v = fake_vector(&j.text);
                (j, v)
            })
            .collect();
        store_vectors(c, &done).unwrap();
    }

    fn run(
        &self,
        words: &str,
        vector: Option<&[f32]>,
        budget: usize,
        pinned: &[ItemRef],
    ) -> Retrieved {
        self.run_with(words, vector, budget, pinned, &PlanFilters::default(), &[])
    }

    fn run_with(
        &self,
        words: &str,
        vector: Option<&[f32]>,
        budget: usize,
        pinned: &[ItemRef],
        filters: &PlanFilters,
        scope: &[String],
    ) -> Retrieved {
        retrieve(
            self.db.conn(),
            &Retrieval {
                words,
                query_vector: vector,
                scope_category_ids: scope,
                filters,
                rule: RULE,
                budget_tokens: budget,
                pinned,
            },
        )
        .unwrap()
    }
}

fn titles(v: &[SourceRef]) -> Vec<&str> {
    v.iter().map(|s| s.title.as_str()).collect()
}

#[test]
fn meaning_results_need_the_floor_and_the_margin_and_keyword_results_pass() {
    let mut e = env();
    e.page("Hose guide", "hose replacement steps");
    e.page("Tube notes", "tube cleaning routine");
    e.page("Fuel", "unrelated stuff about fuel");
    e.embed();
    let q = axis(0, 1, 0.0);
    let r = e.run("hose", Some(&q), 8000, &[]);
    assert_eq!(titles(&r.sources), vec!["Hose guide", "Tube notes"]);
    assert_eq!(titles(&r.more), vec!["Fuel"]);
    assert_eq!(r.sources[0].part, None, "no section before the first heading");
    assert_eq!(r.items.len(), 2);
    assert!(r.sources[0].snippet.contains("hose replacement"));

    // Without the search model only the keyword result is used.
    let r = e.run("hose", None, 8000, &[]);
    assert_eq!(titles(&r.sources), vec!["Hose guide"]);
}

#[test]
fn a_hit_through_glossary_lines_only_must_pass_the_meaning_rule() {
    let mut e = env();
    glossary::create(
        &mut e.db,
        NewGlossaryEntry {
            category_id: None,
            term: "MMU".into(),
            other_names: None,
            description: Some("Mobile manufacturing unit".into()),
        },
    )
    .unwrap();
    e.page("Unit notes", "The MMU needs service on Friday.");
    e.page("Hose guide", "hose replacement steps");
    e.embed();
    let q = axis(0, 1, 0.0);
    // "manufacturing" is only in the glossary line the page uses.
    let r = e.run("manufacturing", Some(&q), 8000, &[]);
    // (The hose page passes by meaning on its own.)
    assert_eq!(
        titles(&r.sources),
        vec!["Unit notes", "Hose guide"],
        "close enough by meaning"
    );
    // Far away by meaning: the same hit does not pass.
    let far = axis(5, 6, 0.0);
    let r = e.run("manufacturing", Some(&far), 8000, &[]);
    assert!(r.sources.is_empty(), "{:?}", titles(&r.sources));
    assert!(titles(&r.more).contains(&"Unit notes"));
    // A word in the page's own text is a plain keyword hit.
    let r = e.run("friday", Some(&far), 8000, &[]);
    assert_eq!(titles(&r.sources), vec!["Unit notes"]);
    // The glossary entry itself is not a source: its id goes to the glossary block.
    let r = e.run("MMU", Some(&q), 8000, &[]);
    assert_eq!(r.term_ids.len(), 1);
    assert!(titles(&r.sources).contains(&"Unit notes"));
}

#[test]
fn pinned_items_come_first_and_the_budget_decides_what_is_sent() {
    let mut e = env();
    let a = e.page(
        "Hose guide",
        &format!("hose {}", "replacement steps. ".repeat(100)),
    );
    let b = e.page(
        "Tube notes",
        &format!("tube {}", "cleaning routine. ".repeat(100)),
    );
    let c = e.page("Fuel", "fuel only");
    e.embed();
    let q = axis(0, 1, 0.0);
    let r = e.run("hose", Some(&q), 8000, &[ItemRef::page(&c)]);
    assert_eq!(
        titles(&r.sources),
        vec!["Fuel", "Hose guide", "Tube notes"]
    );
    // About 480 tokens each: room for the pinned page and one more.
    let r = e.run("hose", Some(&q), 600, &[ItemRef::page(&c)]);
    assert_eq!(titles(&r.sources), vec!["Fuel", "Hose guide"]);
    assert_eq!(titles(&r.more)[0], "Tube notes");
    // A pinned item is sent even when it does not fit; without words only pinned items are.
    let r = e.run(
        "",
        None,
        1,
        &[ItemRef::page(&b), ItemRef::page(&a), ItemRef::page("gone")],
    );
    assert_eq!(titles(&r.sources), vec!["Tube notes", "Hose guide"]);
    assert!(r.more.is_empty());
}

#[test]
fn task_pieces_are_a_description_or_a_dated_comment() {
    let mut e = env();
    let t = e.task(
        "Auger service",
        "Check the hose at the elbow.",
        TaskStatus::Todo,
    );
    tasks::add_comment(&mut e.db, &t, "Moved the clamp upstream.").unwrap();
    e.embed();
    let r = e.run("auger", None, 8000, &[]);
    assert_eq!(r.sources.len(), 1);
    assert_eq!(r.sources[0].key.as_deref(), Some("GEN-1"));
    assert_eq!(r.pieces[0].part, PiecePart::Description);
    assert_eq!(r.pieces[0].text, "Check the hose at the elbow.");
    // The words pick the piece when there is no vector.
    let r = e.run("clamp", None, 8000, &[]);
    let PiecePart::Comment(day) = &r.pieces[0].part else {
        panic!("{:?}", r.pieces[0].part)
    };
    assert_eq!(day.len(), 10);
    assert_eq!(r.pieces[0].text, "Moved the clamp upstream.");
    assert_eq!(
        r.sources[0].part.as_deref(),
        Some(format!("Comment {day}").as_str())
    );
}

#[test]
fn plan_filters_narrow_the_candidates_and_all_words_failing_retries_with_any() {
    let mut e = env();
    e.task("Hose open", "hose is leaking", TaskStatus::Todo);
    e.task("Hose closed", "hose was replaced", TaskStatus::Done);
    e.page("Hose guide", "hose replacement steps");
    let filters = PlanFilters {
        statuses: vec![TaskStatus::Todo],
        ..PlanFilters::default()
    };
    // A status filter means tasks only.
    let r = e.run_with("hose", None, 8000, &[], &filters, &[]);
    assert_eq!(titles(&r.sources), vec!["Hose open"]);
    // "hose" and "zzz" together find nothing; any word finds the hose items.
    let r = e.run("hose zzz", None, 8000, &[]);
    assert_eq!(r.sources.len(), 3);
    // A scope of another category finds nothing.
    e.db.conn()
        .execute(
            "INSERT INTO categories (id, name, key, created_at, updated_at) VALUES ('p2', 'Other', 'OTH', 'x', 'x')",
            [],
        )
        .unwrap();
    let _ = categories::list(e.db.conn(), true).unwrap();
    let none = e.run_with(
        "hose",
        None,
        8000,
        &[],
        &PlanFilters::default(),
        &["p2".to_string()],
    );
    assert!(none.sources.is_empty());
}

#[test]
fn the_page_title_line_is_not_a_section() {
    let mut e = env();
    e.page("Hose guide", "# Hose guide\n\nIntro about the hose.\n\n## Steps\n\nReplace the hose every year.");
    let r = e.run("hose", None, 8000, &[]);
    let parts: Vec<Option<&str>> = r.sources.iter().map(|s| s.part.as_deref()).collect();
    assert!(parts.iter().all(|p| *p != Some("Hose guide")), "{parts:?}");
    assert!(r.pieces.iter().all(|p| !matches!(&p.part, PiecePart::Section(path) if path.first().map(String::as_str) == Some("Hose guide"))));
}

#[test]
fn a_referenced_task_is_sent_whole() {
    // D-83: the description and every comment, in order, even where the words don't match.
    let mut e = env();
    let t = e.task("Site visit", "Visit the north site and check the network rack.", TaskStatus::Todo);
    for c in ["Rack is fine.", "Printer jammed again.", "The wifi drops every hour near the gate."] {
        tasks::add_comment(&mut e.db, &t, c).unwrap();
    }
    e.embed();
    let r = e.run("printer", None, 8000, &[ItemRef::task(&t)]);
    let text: Vec<&str> = r.pieces.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(r.pieces[0].part, PiecePart::Description);
    let all = text.join("\n");
    for c in ["Rack is fine.", "Printer jammed again.", "The wifi drops every hour"] {
        assert!(all.contains(c), "{c} missing from {all}");
    }
    assert!(!all.contains('['), "nothing was left out: {all}");
}

#[test]
fn a_long_referenced_task_keeps_its_start_and_best_matches() {
    let mut e = env();
    let t = e.task("Site visit", &"Check the rack and the cabling. ".repeat(20), TaskStatus::Todo);
    let filler = "Nothing new on the rack today. ".repeat(40);
    tasks::add_comment(&mut e.db, &t, &format!("Day one. {filler}")).unwrap();
    tasks::add_comment(&mut e.db, &t, &format!("Wifi drops near the gate. {}", "wifi ".repeat(30))).unwrap();
    tasks::add_comment(&mut e.db, &t, &format!("Day three. {filler}")).unwrap();
    e.embed();
    let whole = e.run("wifi", None, 100_000, &[ItemRef::task(&t)]);
    assert!(whole.pieces.len() >= 4, "{} pieces", whole.pieces.len());
    // Half of this budget holds the description and about one comment.
    let r = e.run("wifi", None, 1000, &[ItemRef::task(&t)]);
    assert!(r.pieces.len() < whole.pieces.len());
    assert_eq!(r.pieces[0].part, PiecePart::Description);
    let all: String = r.pieces.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n");
    assert!(all.contains("Wifi drops near the gate."), "{all}");
    assert!(!all.contains("Day one."), "{all}");
    // The last piece says what was left out, by the names the sources use.
    let last = &r.pieces.last().unwrap().text;
    // A page writer gets the pieces without it.
    let plain = without_left_out_notes(&r.pieces);
    let end = &plain.last().unwrap().text;
    assert!(end.contains("Wifi drops near the gate.") && !end.contains('['), "{end}");
    assert_eq!(plain[0].text, r.pieces[0].text);
    let PiecePart::Comment(day) = &whole.pieces[1].part else { panic!("{:?}", whole.pieces[1].part) };
    assert!(
        last.ends_with(&format!("sent without 1 of its parts: Comment {day}. Do not guess what is missing; if it may hold what you need, say what was not sent, so the user can paste it.]")),
        "{last}"
    );
}
