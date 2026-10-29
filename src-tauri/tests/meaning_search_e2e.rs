//! End to end: the real `embed` sidecar (`sidecar/.venv` + EmbeddingGemma) and the real core index.
//! Pages are cut into pieces, embedded the way the indexer does it, then found by meaning
//! (FR-RAG-IX-5, FR-RAG-SR-1/4/5). Skipped (passes with a note) when the dev sidecar isn't set up:
//! run `scripts/setup-sidecar-dev.sh` first.

use std::sync::Arc;

use cairn_core::glossary::{self, NewGlossaryEntry};
use cairn_core::knowledge::{self, NewPage};
use cairn_core::search::{self, MatchedBy, SearchKind, SearchQuery};
use cairn_core::{Db, hybrid, vectors};
use cairn_lib::sidecar::{EMBED_DIMS, EMBED_MODEL_ID, SidecarConfig, Sidecars};

/// Work notes written the terse way; none mentions the words of the queries below.
const PAGES: &[(&str, &str)] = &[
    (
        "MMU-07 auger leak",
        "Auger on MMU-07 dropped pressure on day shift. Swapped the swivel hose, tested OK. Check the boom seal at next PM.",
    ),
    (
        "Laptop VPN",
        "Install the VPN client, sign in with the work account, reconnect after sleep.",
    ),
    (
        "Leave requests",
        "Submit an HR portal request two weeks ahead and the manager approves it.",
    ),
    (
        "Emulsion sensitisation",
        "Gassing solution is added at the collar to bring the emulsion density down; cup density check after 20 min.",
    ),
    (
        "Quarterly budget",
        "Q3 budget review: maintenance spend over by 8 percent, mostly hoses and seals.",
    ),
];

fn page_id(db: &Db, title: &str) -> String {
    db.conn()
        .query_row("SELECT id FROM pages WHERE title = ?1", [title], |r| {
            r.get(0)
        })
        .unwrap()
}

/// Embeds every waiting piece (pages, tasks and glossary entries) the way the indexer does.
async fn embed_waiting(db: &Db, sidecars: &Sidecars) {
    loop {
        let jobs = vectors::next_jobs(db.conn(), 8).unwrap();
        if jobs.is_empty() {
            break;
        }
        let items = jobs
            .iter()
            .map(|j| (j.title.clone(), j.text.clone()))
            .collect();
        let vecs = sidecars.embed_documents(items).await.unwrap();
        let done: Vec<_> = jobs.into_iter().zip(vecs).collect();
        vectors::store_vectors(db.conn(), &done).unwrap();
    }
}

#[tokio::test]
async fn pages_are_found_by_meaning_through_the_real_sidecar() {
    let config = SidecarConfig::resolve(5, None);
    if config.python.is_none() || config.model_dir.is_none() {
        eprintln!("skipped: the dev sidecar is not set up (scripts/setup-sidecar-dev.sh)");
        return;
    }
    let sidecars = Sidecars::new(config, Arc::new(|_| {}));
    let dir = tempfile::tempdir().unwrap();
    let mut db = Db::open_at(dir.path()).unwrap();
    for (title, body) in PAGES {
        knowledge::create_page(
            &mut db,
            NewPage {
                category_id: "p-general".into(),
                title: (*title).into(),
                body: (*body).into(),
                ..Default::default()
            },
        )
        .unwrap();
    }

    // What the indexer does: record the model, then embed waiting pieces in small batches.
    let model = vectors::IndexModel {
        id: EMBED_MODEL_ID.into(),
        dims: EMBED_DIMS,
    };
    vectors::ensure_model(db.conn(), &model).unwrap();
    embed_waiting(&db, &sidecars).await;
    let status = vectors::status(db.conn()).unwrap();
    assert_eq!(status.items, PAGES.len() as u32);
    assert_eq!(status.items_ready, status.items, "every page indexed");

    let filter = SearchQuery {
        kinds: Some(vec![SearchKind::Page]),
        ..Default::default()
    };
    let cases = [
        (
            "how do I connect to the office network from home",
            "Laptop VPN",
        ),
        ("asking for time off", "Leave requests"),
        ("making the explosive less dense", "Emulsion sensitisation"),
        (
            "hydraulic problem on the explosives truck",
            "MMU-07 auger leak",
        ),
    ];
    for (query, expected) in cases {
        let q = sidecars.embed_query(query).await.unwrap();
        assert_eq!(q.len(), EMBED_DIMS as usize);
        let meaning = vectors::search(db.conn(), &q, &filter).unwrap();
        let scores: Vec<_> = meaning
            .iter()
            .map(|h| format!("{} {:.2}", h.title, h.score))
            .collect();
        eprintln!("{query:?} -> {scores:?}");
        assert_eq!(meaning[0].title, expected, "query {query:?}");

        // Hybrid (FR-RAG-SR-4): the keyword half finds nothing for these words, so the
        // expected page comes from meaning only, first.
        let keyword = search::search(
            db.conn(),
            &SearchQuery {
                text: query.into(),
                ..filter.clone()
            },
        )
        .unwrap();
        let fused = hybrid::fuse(keyword, meaning, 10);
        assert_eq!(fused[0].title, expected);
        assert_eq!(fused[0].matched_by, MatchedBy::Meaning);
    }

    // Find similar pages: the auger note is closer to the hose budget note than to leave requests.
    let auger = page_id(&db, "MMU-07 auger leak");
    let similar = vectors::similar_items(db.conn(), SearchKind::Page, &auger, &filter).unwrap();
    assert!(similar.iter().all(|h| h.id != auger), "excludes itself");
    let rank = |t: &str| similar.iter().position(|h| h.title == t).unwrap();
    assert!(rank("Quarterly budget") < rank("Leave requests"));

    // The glossary (FR-RAG-IX-3): the notes only say "MMU" and "auger"; a query written
    // from the definitions should find the auger note, and find it better once the entries exist.
    let query = "problem with the arm that delivers explosives into the hole";
    let q = sidecars.embed_query(query).await.unwrap();
    // (rank, score of the note, best score among the other pages)
    let score_of = |db: &Db, title: &str| -> (usize, f32, f32) {
        let hits = vectors::search(db.conn(), &q, &filter).unwrap();
        let rank = hits.iter().position(|h| h.title == title).unwrap();
        let other = hits
            .iter()
            .filter(|h| h.title != title)
            .map(|h| h.score)
            .fold(f32::MIN, f32::max);
        (rank, hits[rank].score, other)
    };
    let (rank_without, without, other_without) = score_of(&db, "MMU-07 auger leak");
    for (term, others, description) in [
        (
            "MMU",
            vec!["Mobile Manufacturing Unit"],
            "truck that mixes bulk explosive on site and pumps it into the blast holes",
        ),
        (
            "auger",
            vec![],
            "the arm on an MMU that delivers the explosive into the blast hole",
        ),
    ] {
        glossary::create(
            &mut db,
            NewGlossaryEntry {
                term: term.into(),
                other_names: Some(others.into_iter().map(String::from).collect()),
                description: Some(description.into()),
                ..Default::default()
            },
        )
        .unwrap();
    }
    // Only the pieces that use a term (and the new entries) are waiting.
    let waiting = vectors::next_jobs(db.conn(), 100).unwrap();
    assert!(waiting.iter().any(|j| j.title == "MMU-07 auger leak"));
    assert!(
        waiting
            .iter()
            .all(|j| !j.title.starts_with("Leave") && !j.title.starts_with("Quarterly"))
    );
    embed_waiting(&db, &sidecars).await;
    let (rank_with, with, other_with) = score_of(&db, "MMU-07 auger leak");
    eprintln!(
        "GLOSSARY e2e: auger note for {query:?}: without glossary rank {} score {without:.3} (best other {other_without:.3}); with glossary rank {} score {with:.3} (best other {other_with:.3})",
        rank_without + 1,
        rank_with + 1
    );
    assert_eq!(rank_with, 0, "the auger note ranks first with the glossary");
    assert!(with > without, "the definitions raise the score");

    // The entries are results too: the query is close to the auger definition.
    let terms = vectors::search(
        db.conn(),
        &q,
        &SearchQuery {
            kinds: Some(vec![SearchKind::Term]),
            ..Default::default()
        },
    )
    .unwrap();
    eprintln!(
        "GLOSSARY e2e: term hits {:?}",
        terms
            .iter()
            .map(|h| format!("{} {:.3}", h.title, h.score))
            .collect::<Vec<_>>()
    );
    assert_eq!(terms[0].title, "auger");
}
