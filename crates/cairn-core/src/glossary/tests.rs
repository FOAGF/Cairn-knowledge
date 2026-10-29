use super::*;
use crate::knowledge::{self, NewPage, PagePatch};
use crate::categories::{self, NewCategory, CategoryColor};
use crate::search::{SearchHit, SearchQuery};
use crate::tasks::{self, NewTask, TaskPatch};

struct Env {
    _dir: tempfile::TempDir,
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
    Env {
        _dir: dir,
        db,
        a,
        b,
    }
}

fn entry(term: &str, others: &[&str], desc: &str) -> NewGlossaryEntry {
    NewGlossaryEntry {
        term: term.into(),
        other_names: Some(others.iter().map(|s| (*s).into()).collect()),
        description: Some(desc.into()),
        ..Default::default()
    }
}

fn in_category(category: &str, mut e: NewGlossaryEntry) -> NewGlossaryEntry {
    e.category_id = Some(category.into());
    e
}

fn page(db: &mut Db, category: &str, title: &str, body: &str) -> String {
    knowledge::create_page(
        db,
        NewPage {
            category_id: category.into(),
            title: title.into(),
            body: body.into(),
            ..Default::default()
        },
    )
    .unwrap()
    .id
}

fn set_body(db: &mut Db, id: &str, body: &str) {
    knowledge::update_page(
        db,
        id,
        PagePatch {
            body: Some(body.into()),
            ..Default::default()
        },
    )
    .unwrap();
}

fn task(db: &mut Db, category: &str, title: &str, desc: &str) -> String {
    tasks::create(
        db,
        NewTask {
            category_id: category.into(),
            title: title.into(),
            description: Some(desc.into()),
            ..Default::default()
        },
    )
    .unwrap()
    .id
}

fn uses(db: &Db, entry_id: &str) -> u32 {
    get(db.conn(), entry_id).unwrap().use_count
}

fn keyword(db: &Db, text: &str) -> Vec<SearchHit> {
    search::search(
        db.conn(),
        &SearchQuery {
            text: text.into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn titles(hits: &[SearchHit]) -> Vec<&str> {
    hits.iter().map(|h| h.title.as_str()).collect()
}

/// Gives every waiting piece a dummy vector.
fn fill(db: &Db) {
    let done: Vec<_> = vectors::next_jobs(db.conn(), 10_000)
        .unwrap()
        .into_iter()
        .map(|j| (j, vec![1.0, 0.0]))
        .collect();
    vectors::store_vectors(db.conn(), &done).unwrap();
}

fn waiting(db: &Db) -> u32 {
    db.conn()
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE vector IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

// ---------------------------------------------------------------- the matcher

fn cases() -> serde_json::Value {
    serde_json::from_str(include_str!("../../../../app/src/lib/glossary-cases.json")).unwrap()
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

/// FR-KN-GL-4: the cases shared with the UI's matcher.
#[test]
fn shared_matching_cases() {
    let file = cases();
    let all: Vec<Rc<Entry>> = file["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            Rc::new(Entry {
                id: e["id"].as_str().unwrap().into(),
                category_id: e["categoryId"].as_str().map(String::from),
                term: e["term"].as_str().unwrap().into(),
                other_names: strings(&e["otherNames"]),
                description: String::new(),
                created_at: String::new(),
                updated_at: String::new(),
            })
        })
        .collect();
    let mut checked = 0;
    for c in file["cases"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        let category = c["category"].as_str().unwrap_or("");
        let scope = Scope::new(applicable(&all, category));
        let ids: Vec<&str> = scope
            .find(text)
            .iter()
            .map(|(e, _)| e.id.as_str())
            .collect();
        assert_eq!(ids, strings(&c["ids"]), "text {text:?} in {category:?}");
        checked += 1;
    }
    assert!(checked >= 20);
}

fn matcher(names: &[&str]) -> Matcher {
    let list: Vec<(usize, String)> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (i, (*n).to_string()))
        .collect();
    Matcher::new(&list)
}

#[test]
fn offsets_are_byte_offsets_in_the_original_text() {
    let m = matcher(&["MMU", "Mobile Manufacturing Unit"]);
    let text = "更新 MMU-07 and the mobile\n  manufacturing unit.";
    let found = m.find(text);
    assert_eq!(found.len(), 2);
    assert_eq!(&text[found[0].start..found[0].end], "MMU-07");
    assert_eq!(
        &text[found[1].start..found[1].end],
        "mobile\n  manufacturing unit"
    );
}

#[test]
fn longer_valid_name_wins_and_shorter_one_survives_a_failed_boundary() {
    let m = matcher(&["staging", "staging bucket"]);
    // "staging bucketx" fails the boundary of the long name; "staging" alone still matches.
    let text = "the staging bucketx";
    let found = m.find(text);
    assert_eq!(found.len(), 1);
    assert_eq!(&text[found[0].start..found[0].end], "staging");
    let found = m.find("the staging bucket");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].end - found[0].start, "staging bucket".len());
}

#[test]
fn matching_a_big_page_is_fast() {
    let names: Vec<String> = (0..500)
        .map(|i| match i % 3 {
            0 => format!("TRM{i}"),
            1 => format!("blast hole number {i}"),
            _ => format!("word{i}"),
        })
        .collect();
    let list: Vec<(usize, String)> = names.iter().cloned().enumerate().collect();
    let m = Matcher::new(&list);
    let mut text = String::new();
    while text.len() < 10_000 {
        text.push_str(
            "The blast hole number 7 near TRM3 was checked.\nNothing odd with word2, word4s.\n",
        );
    }
    let start = std::time::Instant::now();
    let rounds = 200;
    let mut found = 0;
    for _ in 0..rounds {
        found += m.find(&text).len();
    }
    let each = start.elapsed() / rounds;
    eprintln!(
        "matching {} bytes against 500 names: {each:?} each ({found} matches)",
        text.len()
    );
    assert!(found > 0);
    // Generous for an unoptimised test build; a release build takes well under a millisecond.
    assert!(each < std::time::Duration::from_millis(25), "{each:?}");
}

// ---------------------------------------------------------------- entries

#[test]
fn create_validates_and_cleans() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let err = |r: Result<GlossaryEntry>| r.unwrap_err().to_string();

    assert_eq!(
        err(create(&mut e.db, entry("   ", &[], ""))),
        "Give the term a name."
    );
    let mmu = create(
        &mut e.db,
        entry(
            "  MMU ",
            &[
                " Mobile Manufacturing Unit ",
                "",
                "MMU",
                "Mobile Manufacturing Unit",
            ],
            "  Truck.  ",
        ),
    )
    .unwrap();
    assert_eq!(mmu.term, "MMU");
    assert_eq!(mmu.other_names, ["Mobile Manufacturing Unit"]);
    assert_eq!(mmu.description, "Truck.");
    assert_eq!(mmu.category_id, None);

    // Unique per scope, ignoring case.
    assert_eq!(
        err(create(&mut e.db, entry("mmu", &[], ""))),
        "“mmu” is already in the global glossary"
    );
    // The same term may exist once more in a category.
    let own = create(&mut e.db, in_category(&a, entry("mmu", &[], "memory"))).unwrap();
    assert_eq!(own.category_id.as_deref(), Some(a.as_str()));
    assert_eq!(
        err(create(&mut e.db, in_category(&a, entry("MMU", &[], "")))),
        "“MMU” is already in the glossary of category Alpha"
    );
    create(&mut e.db, in_category(&b, entry("MMU", &[], ""))).unwrap();

    // The category must exist; archived categories are refused.
    assert!(
        create(&mut e.db, in_category("nope", entry("X1", &[], "")))
            .unwrap_err()
            .to_string()
            .contains("category not found")
    );
    categories::set_archived(e.db.conn(), &b, true).unwrap();
    assert!(err(create(&mut e.db, in_category(&b, entry("Z1", &[], "")))).contains("archived"));
    // An empty categoryId means global.
    let mut global = entry("G9", &[], "");
    global.category_id = Some(String::new());
    assert_eq!(create(&mut e.db, global).unwrap().category_id, None);
}

#[test]
fn update_changes_fields_scope_and_checks_uniqueness() {
    let mut e = env();
    let a = e.a.clone();
    let one = create(&mut e.db, entry("Alpha1", &["A1"], "first")).unwrap();
    let two = create(&mut e.db, entry("Beta2", &[], "")).unwrap();

    let dup = update(
        &mut e.db,
        &two.id,
        GlossaryPatch {
            term: Some("alpha1".into()),
            ..Default::default()
        },
    );
    assert!(
        dup.unwrap_err()
            .to_string()
            .contains("already in the global glossary")
    );

    let changed = update(
        &mut e.db,
        &one.id,
        GlossaryPatch {
            term: Some("Alpha-1".into()),
            other_names: Some(vec!["A-one".into(), " A-one ".into()]),
            description: Some("changed".into()),
            category_id: Some(a.clone()),
        },
    )
    .unwrap();
    assert_eq!(changed.term, "Alpha-1");
    assert_eq!(changed.other_names, ["A-one"]);
    assert_eq!(changed.category_id.as_deref(), Some(a.as_str()));
    assert!(changed.updated_at >= one.updated_at);

    let back = update(
        &mut e.db,
        &one.id,
        GlossaryPatch {
            category_id: Some(String::new()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(back.category_id, None);
    assert_eq!(back.description, "changed", "untouched fields stay");

    delete(&mut e.db, &one.id).unwrap();
    assert!(matches!(
        get(e.db.conn(), &one.id),
        Err(CoreError::NotFound { .. })
    ));
    assert!(delete(&mut e.db, &one.id).is_err());
}

#[test]
fn list_is_sorted_ignoring_case() {
    let mut e = env();
    for t in ["bravo", "Alpha", "charlie", "ALPHA2"] {
        create(&mut e.db, entry(t, &[], "")).unwrap();
    }
    let terms: Vec<String> = list(e.db.conn())
        .unwrap()
        .into_iter()
        .map(|g| g.term)
        .collect();
    assert_eq!(terms, ["Alpha", "ALPHA2", "bravo", "charlie"]);
}

/// D-38: related entries are looked up inside what applies to the entry.
#[test]
fn related_entries_stay_inside_their_scope() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let mmu = create(
        &mut e.db,
        entry("MMU", &[], "Truck that carries the auger."),
    )
    .unwrap();
    let auger = create(&mut e.db, entry("auger", &[], "The arm on an MMU.")).unwrap();
    let unrelated = create(&mut e.db, entry("Zebra", &[], "Stripes.")).unwrap();
    let kestrel = create(
        &mut e.db,
        in_category(&a, entry("Kestrel", &[], "Works with the auger.")),
    )
    .unwrap();
    let other = create(
        &mut e.db,
        in_category(
            &b,
            entry("Osprey", &[], "Compare with Kestrel and the MMU."),
        ),
    )
    .unwrap();

    let by_id = |id: &str| get(e.db.conn(), id).unwrap();
    assert_eq!(by_id(&mmu.id).related_ids, [auger.id.clone()]);
    // A global entry does not see category entries, so Kestrel (naming the auger) is missing here.
    assert_eq!(by_id(&auger.id).related_ids, [mmu.id.clone()]);
    assert!(by_id(&unrelated.id).related_ids.is_empty());
    // A category entry sees the global ones and its own category's.
    assert_eq!(by_id(&kestrel.id).related_ids, [auger.id.clone()]);
    // "Kestrel" belongs to another category: not related to Osprey; the global MMU is.
    assert_eq!(by_id(&other.id).related_ids, [mmu.id.clone()]);
}

#[test]
fn overrides_and_overridden_in() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let global = create(&mut e.db, entry("MMU", &[], "Truck")).unwrap();
    let in_a = create(&mut e.db, in_category(&a, entry("mmu", &[], "Memory"))).unwrap();
    let in_b = create(&mut e.db, in_category(&b, entry("Mmu", &[], "Other"))).unwrap();
    let plain = create(&mut e.db, in_category(&a, entry("Local", &[], ""))).unwrap();

    let g = get(e.db.conn(), &global.id).unwrap();
    assert_eq!(g.overridden_in, {
        let mut v = vec![a.clone(), b.clone()];
        v.sort();
        v
    });
    assert_eq!(g.overrides_id, None);
    let pa = get(e.db.conn(), &in_a.id).unwrap();
    assert_eq!(pa.overrides_id.as_deref(), Some(global.id.as_str()));
    assert!(pa.overridden_in.is_empty());
    assert_eq!(
        get(e.db.conn(), &in_b.id).unwrap().overrides_id.as_deref(),
        Some(global.id.as_str())
    );
    assert_eq!(get(e.db.conn(), &plain.id).unwrap().overrides_id, None);

    // Inside category A the category's entry hides the global one; B and others see theirs.
    let a_terms = terms_in(e.db.conn(), &a, "the MMU").unwrap();
    assert_eq!(a_terms.len(), 1);
    assert_eq!(a_terms[0].description, "Memory");
    let none = terms_in(e.db.conn(), "p-general", "the MMU").unwrap();
    assert_eq!(none[0].description, "Truck");
}

// ---------------------------------------------------------------- uses

#[test]
fn uses_follow_pages_tasks_and_comments() {
    let mut e = env();
    let a = e.a.clone();
    let mmu = create(
        &mut e.db,
        entry("MMU", &["Mobile Manufacturing Unit"], "Truck"),
    )
    .unwrap();
    assert_eq!(mmu.use_count, 0);

    let pg = page(&mut e.db, &a, "Wash", "Washed the MMUs today");
    assert_eq!(uses(&e.db, &mmu.id), 1);
    // Title counts too.
    let titled = page(&mut e.db, &a, "MMU notes", "nothing");
    assert_eq!(uses(&e.db, &mmu.id), 2);

    let t = task(&mut e.db, &a, "Plain task", "Clean up");
    assert_eq!(uses(&e.db, &mmu.id), 2);
    tasks::update(
        &mut e.db,
        &t,
        TaskPatch {
            description: Some("Clean the mobile manufacturing unit".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 3);
    let c = tasks::add_comment(&mut e.db, &t, "still MMU").unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 3);

    // A task whose only mention is in a comment.
    let t2 = task(&mut e.db, &a, "Other", "-");
    let c2 = tasks::add_comment(&mut e.db, &t2, "check the MMU").unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 4);
    tasks::delete_comment(&mut e.db, &c2.id).unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 3);
    tasks::update_comment(&mut e.db, &c.id, "gone").unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 3, "the description still uses it");

    set_body(&mut e.db, &pg, "Washed the truck");
    assert_eq!(uses(&e.db, &mmu.id), 2);
    knowledge::delete_page(&mut e.db, &titled).unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 1);
    tasks::delete(&mut e.db, &t).unwrap();
    assert_eq!(uses(&e.db, &mmu.id), 0);
    let rows: i64 =
        e.db.conn()
            .query_row("SELECT COUNT(*) FROM glossary_uses", [], |r| r.get(0))
            .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn entry_changes_refresh_items_and_category_scope_is_respected() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let in_a = page(&mut e.db, &a, "In alpha", "Seen an MMU there");
    let in_b = page(&mut e.db, &b, "In beta", "Seen an MMU here");

    // A global entry created later reaches existing pages (found through the keyword index).
    let global = create(&mut e.db, entry("MMU", &[], "Truck")).unwrap();
    assert_eq!(global.use_count, 2);

    // A category entry takes over inside its category only.
    let mine = create(&mut e.db, in_category(&a, entry("mmu", &[], "Memory"))).unwrap();
    assert_eq!((mine.use_count, uses(&e.db, &global.id)), (1, 1));
    let used: String =
        e.db.conn()
            .query_row(
                "SELECT entry_id FROM glossary_uses WHERE ref_id = ?1",
                [&in_a],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(used, mine.id);

    // Deleting it gives the page back to the global entry.
    delete(&mut e.db, &mine.id).unwrap();
    assert_eq!(uses(&e.db, &global.id), 2);
    // Moving the global entry's name away frees both pages.
    update(
        &mut e.db,
        &global.id,
        GlossaryPatch {
            term: Some("Truck".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(uses(&e.db, &global.id), 0);
    // Adding the old name as another name brings them back.
    update(
        &mut e.db,
        &global.id,
        GlossaryPatch {
            other_names: Some(vec!["MMU".into()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(uses(&e.db, &global.id), 2);
    let _ = in_b;
}

#[test]
fn keyword_search_finds_items_through_the_definition() {
    let mut e = env();
    let a = e.a.clone();
    page(&mut e.db, &a, "Wash log", "MMU washed once.");
    page(&mut e.db, &a, "Unrelated", "Nothing here.");
    assert!(keyword(&e.db, "mobile manufacturing unit").is_empty());

    let mmu = create(
        &mut e.db,
        entry(
            "MMU",
            &["Mobile Manufacturing Unit"],
            "Truck that mixes explosive",
        ),
    )
    .unwrap();
    let hits = keyword(&e.db, "mobile manufacturing unit");
    // The page (through the entry's line in its extra text) and the entry itself.
    assert_eq!(titles(&hits).len(), 2);
    assert!(titles(&hits).contains(&"Wash log"));
    assert!(
        hits.iter()
            .any(|h| h.kind == SearchKind::Term && h.title == "MMU")
    );
    let by_description = keyword(&e.db, "explosive");
    assert!(titles(&by_description).contains(&"Wash log"));

    update(
        &mut e.db,
        &mmu.id,
        GlossaryPatch {
            description: Some("Truck".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!titles(&keyword(&e.db, "explosive")).contains(&"Wash log"));

    delete(&mut e.db, &mmu.id).unwrap();
    assert!(keyword(&e.db, "mobile manufacturing unit").is_empty());
}

// ---------------------------------------------------------------- vector index

#[test]
fn pieces_embed_the_definitions_they_use_and_only_those_pieces_change() {
    let mut e = env();
    let a = e.a.clone();
    let long = "Plain filler sentence about nothing special at all. ".repeat(40);
    let body =
        format!("# Wash\n\nWashed the MMU today.\n\n# Other\n\n{long}\n\n# Third\n\n{long} again");
    let pg = page(&mut e.db, &a, "Log", &body);
    let mmu = create(
        &mut e.db,
        entry("MMU", &["Mobile Manufacturing Unit"], "Truck"),
    )
    .unwrap();

    let jobs = vectors::next_jobs(e.db.conn(), 100).unwrap();
    let with_def: Vec<_> = jobs
        .iter()
        .filter(|j| j.text.contains("MMU (Mobile Manufacturing Unit): Truck"))
        .collect();
    assert_eq!(
        with_def.len(),
        1,
        "only the piece that says MMU gets the definition"
    );
    assert!(jobs.iter().any(|j| j.title == "Log"
        && j.text.starts_with("Wash\nWashed the MMU today.")
        && j.text.ends_with("MMU (Mobile Manufacturing Unit): Truck")));
    let pieces: i64 =
        e.db.conn()
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE ref_id = ?1",
                [&pg],
                |r| r.get(0),
            )
            .unwrap();
    assert!(pieces >= 3);

    fill(&e.db);
    assert_eq!(waiting(&e.db), 0);
    update(
        &mut e.db,
        &mmu.id,
        GlossaryPatch {
            description: Some("Truck that mixes explosive".into()),
            ..Default::default()
        },
    )
    .unwrap();
    // The one piece that uses it and the entry's own piece wait again; the rest keep vectors.
    assert_eq!(waiting(&e.db), 2);
    delete(&mut e.db, &mmu.id).unwrap();
    assert_eq!(
        waiting(&e.db),
        1,
        "the page piece loses the definition, the term piece is gone"
    );
    let jobs = vectors::next_jobs(e.db.conn(), 100).unwrap();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].text.starts_with("Wash\nWashed the MMU today."));
    assert!(!jobs[0].text.contains("Truck"));

    // The status counts pages and tasks only.
    create(&mut e.db, entry("Solo", &[], "x")).unwrap();
    let st = vectors::status(e.db.conn()).unwrap();
    assert_eq!(st.items, 1);
    assert_eq!(st.chunks as i64, pieces);
}

#[test]
fn terms_are_results_of_their_own() {
    let mut e = env();
    let (a, b) = (e.a.clone(), e.b.clone());
    let global = create(
        &mut e.db,
        entry(
            "auger",
            &["screw feeder"],
            "Arm that delivers the explosive",
        ),
    )
    .unwrap();
    let mine = create(
        &mut e.db,
        in_category(&a, entry("auger", &[], "A hole drill")),
    )
    .unwrap();
    page(&mut e.db, &a, "Hose swap", "swapped the auger hose");

    // Keyword: kind `term`, empty category for global entries.
    let hits = keyword(&e.db, "auger");
    let g = hits.iter().find(|h| h.id == global.id).unwrap();
    assert_eq!((g.kind, g.category_id.as_str()), (SearchKind::Term, ""));
    let m = hits.iter().find(|h| h.id == mine.id).unwrap();
    assert_eq!(m.category_id, a);
    assert!(
        keyword(&e.db, "screw feeder")
            .iter()
            .any(|h| h.id == global.id)
    );

    // `kinds` selects them.
    let only = |kinds, category: Option<&str>| {
        search::search(
            e.db.conn(),
            &SearchQuery {
                text: "auger".into(),
                kinds: Some(kinds),
                category_ids: category.map(|p| vec![p.to_string()]),
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert!(
        only(vec![SearchKind::Page], None)
            .iter()
            .all(|h| h.kind == SearchKind::Page)
    );
    assert_eq!(only(vec![SearchKind::Term], None).len(), 2);
    // A global entry passes any category filter; a category entry only its own category's.
    let in_a: Vec<_> = only(vec![SearchKind::Term], Some(&a))
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert_eq!(in_a.len(), 2);
    let in_b: Vec<_> = only(vec![SearchKind::Term], Some(&b))
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert_eq!(in_b, [global.id.clone()]);

    // Meaning: one `term` piece per entry; the same filter rules.
    fill(&e.db);
    let query = [1.0f32, 0.0];
    let all = SearchQuery::default();
    let hits = vectors::search(e.db.conn(), &query, &all).unwrap();
    assert!(
        hits.iter()
            .any(|h| h.kind == SearchKind::Term && h.id == global.id && h.category_id.is_empty())
    );
    assert!(hits.iter().any(|h| h.kind == SearchKind::Page));
    let term_hit = hits.iter().find(|h| h.id == global.id).unwrap();
    assert_eq!(term_hit.title, "auger");
    assert_eq!(
        term_hit.text,
        "screw feeder\nArm that delivers the explosive"
    );
    let pages_only = SearchQuery {
        kinds: Some(vec![SearchKind::Page]),
        ..Default::default()
    };
    assert!(
        vectors::search(e.db.conn(), &query, &pages_only)
            .unwrap()
            .iter()
            .all(|h| h.kind == SearchKind::Page)
    );
    let terms_in_b = SearchQuery {
        kinds: Some(vec![SearchKind::Term]),
        category_ids: Some(vec![b.clone()]),
        ..Default::default()
    };
    let ids: Vec<_> = vectors::search(e.db.conn(), &query, &terms_in_b)
        .unwrap()
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert_eq!(ids, [global.id.clone()]);
    let terms_in_a = SearchQuery {
        category_ids: Some(vec![a.clone()]),
        ..terms_in_b
    };
    assert_eq!(
        vectors::search(e.db.conn(), &query, &terms_in_a)
            .unwrap()
            .len(),
        2
    );

    // Deleting removes both index rows.
    delete(&mut e.db, &mine.id).unwrap();
    let left: i64 =
        e.db.conn()
            .query_row("SELECT COUNT(*) FROM chunks WHERE kind = 'term'", [], |r| {
                r.get(0)
            })
            .unwrap();
    assert_eq!(left, 1);
    assert!(keyword(&e.db, "auger").iter().all(|h| h.id != mine.id));
}

// ---------------------------------------------------------------- "Used in", spelling

#[test]
fn usage_lists_pages_then_tasks_newest_first_with_excerpts() {
    let mut e = env();
    let a = e.a.clone();
    let mmu = create(&mut e.db, entry("MMU", &[], "Truck")).unwrap();
    let old = page(&mut e.db, &a, "Old page", "short MMU text");
    let long = format!(
        "{} **bold** and [a link](page:abc) before the MMU-07 that was washed\non the weekend, {}",
        "word ".repeat(30),
        "tail ".repeat(30)
    );
    let new = page(&mut e.db, &a, "New page", &long);
    let t = task(&mut e.db, &a, "Wash task", "Wash the MMU");
    // Force a clear order of "newest".
    for (id, ts) in [(&old, "2026-01-01"), (&new, "2026-02-01")] {
        e.db.conn()
            .execute(
                "UPDATE pages SET updated_at = ?1 WHERE id = ?2",
                params![ts, id],
            )
            .unwrap();
    }

    let all = usage(e.db.conn(), &mmu.id, 0, 10).unwrap();
    assert_eq!(all.total, 3);
    let order: Vec<_> = all
        .items
        .iter()
        .map(|h| (h.kind, h.title.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            (SearchKind::Page, "New page"),
            (SearchKind::Page, "Old page"),
            (SearchKind::Task, "Wash task")
        ]
    );
    let hit = &all.items[0];
    assert_eq!(hit.matched, "MMU-07");
    assert_eq!(hit.after.chars().count() <= 51, true);
    assert!(hit.before.starts_with('…'));
    assert!(hit.before.ends_with("link before the "), "{:?}", hit.before);
    assert!(!hit.before.contains('*') && !hit.before.contains("page:abc"));
    assert!(
        hit.after.starts_with(" that was washed on the weekend,"),
        "{:?}",
        hit.after
    );
    assert!(hit.after.ends_with('…'));
    // Short text: nothing cut, so no ellipsis.
    let short = &all.items[1];
    assert_eq!(
        (
            short.before.as_str(),
            short.matched.as_str(),
            short.after.as_str()
        ),
        ("Old page short ", "MMU", " text")
    );
    let task_hit = &all.items[2];
    assert_eq!(task_hit.key.as_deref(), Some("AAA-1"));
    assert_eq!(task_hit.category_id, a);

    // Paging: 3 first, then more.
    let first = usage(e.db.conn(), &mmu.id, 0, 2).unwrap();
    let rest = usage(e.db.conn(), &mmu.id, 2, 10).unwrap();
    assert_eq!((first.items.len(), rest.items.len(), rest.total), (2, 1, 3));
    assert_eq!(rest.items[0].id, t);
    assert!(usage(e.db.conn(), "nope", 0, 3).is_err());
}

#[test]
fn spelling_words_cover_latin_names_only() {
    let mut e = env();
    create(&mut e.db, entry("MMU", &["Mobile Manufacturing Unit"], "")).unwrap();
    create(&mut e.db, entry("固件", &["firmware", "固件版本"], "")).unwrap();
    create(&mut e.db, entry("H2S", &["O'Brien-site"], "")).unwrap();
    let words = spelling_words(e.db.conn()).unwrap();
    assert_eq!(
        words,
        [
            "H2S",
            "MMU",
            "Manufacturing",
            "Mobile",
            "O'Brien",
            "Unit",
            "firmware",
            "site"
        ]
    );
}

// ---------------------------------------------------------------- dev seed, export, upgrade

#[test]
fn dev_seed_adds_missing_entries_once() {
    let mut e = env();
    let a = e.a.clone();
    page(&mut e.db, &a, "Wash", "MMU");
    let seed = |id: &str, term: &str| GlossarySeed {
        id: id.into(),
        category_id: None,
        term: term.into(),
        other_names: vec!["Mobile Manufacturing Unit".into()],
        description: "Truck".into(),
    };
    dev_seed(&mut e.db, vec![seed("s1", "MMU"), seed("s2", "mmu")]).unwrap();
    dev_seed(&mut e.db, vec![seed("s1", "MMU")]).unwrap();
    let all = list(e.db.conn()).unwrap();
    assert_eq!(all.len(), 1, "same term in the same scope is skipped");
    assert_eq!((all[0].id.as_str(), all[0].use_count), ("s1", 1));
    assert!(!keyword(&e.db, "mobile manufacturing").is_empty());
}

#[test]
fn export_and_import_keep_entries_and_rebuild_the_rest() {
    let mut src = env();
    let a = src.a.clone();
    let mmu = create(
        &mut src.db,
        entry("MMU", &["Mobile Manufacturing Unit"], "Truck"),
    )
    .unwrap();
    let mine = create(
        &mut src.db,
        in_category(&a, entry("local", &[], "Only here")),
    )
    .unwrap();
    page(&mut src.db, &a, "Wash", "Washed the MMU");
    let zip = src._dir.path().join("out.zip");
    let summary = crate::export::export_to(&src.db, &zip).unwrap();
    assert!(
        summary
            .tables
            .iter()
            .any(|t| t.table == "glossary_entries" && t.rows == 2)
    );
    assert!(summary.tables.iter().all(|t| t.table != "glossary_uses"));

    let mut dst = env();
    create(&mut dst.db, entry("Other", &[], "to be replaced")).unwrap();
    crate::export::import_from(&mut dst.db, &zip).unwrap();
    let all = list(dst.db.conn()).unwrap();
    assert_eq!(all.len(), 2);
    let got = all.iter().find(|g| g.id == mmu.id).unwrap();
    assert_eq!(got.other_names, ["Mobile Manufacturing Unit"]);
    assert_eq!(got.use_count, 1, "uses are rebuilt");
    assert_eq!(
        all.iter()
            .find(|g| g.id == mine.id)
            .unwrap()
            .category_id
            .as_deref(),
        Some(a.as_str())
    );
    assert!(titles(&keyword(&dst.db, "mobile manufacturing unit")).contains(&"Wash"));
    // Pieces exist for the page (with the definition) and the entries.
    let jobs = vectors::next_jobs(dst.db.conn(), 100).unwrap();
    assert!(jobs.iter().any(|j| j.title == "Wash" && j.text.contains("MMU (Mobile Manufacturing Unit): Truck")));
    assert_eq!(
        jobs.iter()
            .filter(|j| j.title == "MMU" || j.title == "local")
            .count(),
        2
    );
}
