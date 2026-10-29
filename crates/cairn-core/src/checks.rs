//! The save-pipeline checks (FR-KN-SV-1..4): reading the `check` reply (topics, group, tags,
//! glossary suggestions) with the app-side rules of `prompts/README.md` "Local checks", and
//! ranking the duplicate check (FR-KN-DU-1).

use std::collections::HashSet;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use specta::Type;

use crate::Result;
use crate::conversations::{Confidence, TermSuggestion};
use crate::glossary;
use crate::hybrid;
use crate::knowledge::Group;
use crate::search::SearchKind;
use crate::vectors::MeaningHit;

/// A group suggestion (FR-KN-GR-2): an existing group of the category, or a new name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "kind")]
pub enum GroupPick {
    Existing { group_id: String, name: String },
    New { name: String },
}

/// The `check` reply after the README's app-side rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    /// Trimmed, empty ones dropped.
    pub topics: Vec<String>,
    /// Anything but an explicit `false` counts as one topic.
    pub one_topic: bool,
    pub group: Option<GroupPick>,
    /// Trimmed, lowercased, de-duplicated, not the group name or the title, at most 4.
    pub tags: Vec<String>,
    /// After [`filter_terms`].
    pub terms: Vec<TermSuggestion>,
}

/// A term as the model wrote it, before the filter.
#[derive(Debug, Clone, PartialEq)]
pub struct RawTerm {
    pub term: String,
    pub meaning: String,
    /// As written; anything outside the four values becomes `low`.
    pub confidence: String,
}

/// What [`filter_terms`] kept, and what it dropped with the reason (logged at debug level, and
/// printed by the harness).
#[derive(Debug, Clone, PartialEq)]
pub struct TermFilter {
    pub kept: Vec<TermSuggestion>,
    pub dropped: Vec<(String, &'static str)>,
}

const MAX_TAGS: usize = 4;

/// Reads a `check` reply object (`replies::json_object`): topics, one_topic, group (matched to
/// `groups` case-insensitively, whatever `existing` says), tags, and the raw terms for
/// [`filter_terms`].
///
/// `one_topic` is false only for an explicit `false` that comes with at least two topics: with
/// fewer, there is nothing to choose between and the page counts as one topic.
pub fn read_check(reply: &Map<String, Value>, groups: &[Group], title: &str) -> (CheckReport, Vec<RawTerm>) {
    let topics: Vec<String> = reply
        .get("topics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    let one_topic = topics.len() < 2 || reply.get("one_topic") != Some(&Value::Bool(false));

    let group = read_group(reply.get("group"), groups);
    let group_name = match &group {
        Some(GroupPick::Existing { name, .. } | GroupPick::New { name }) => name.to_lowercase(),
        None => String::new(),
    };
    let title = title.trim().to_lowercase();
    let mut tags: Vec<String> = Vec::new();
    for tag in reply
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() || tag == group_name || tag == title || tags.contains(&tag) {
            continue;
        }
        tags.push(tag);
    }
    tags.truncate(MAX_TAGS);

    let raw_terms = reply
        .get("terms")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .filter_map(|o| {
            let text = |key: &str| o.get(key).and_then(Value::as_str).map(str::to_string);
            Some(RawTerm {
                term: text("term")?,
                meaning: text("meaning").unwrap_or_default(),
                confidence: text("confidence").unwrap_or_default(),
            })
        })
        .collect();

    let report = CheckReport {
        topics,
        one_topic,
        group,
        tags,
        terms: Vec::new(),
    };
    (report, raw_terms)
}

/// The `group` field: an object `{name, existing}` (a bare string is read the same way). A name
/// matching a group of the category case-insensitively is that group, whatever `existing` says.
fn read_group(value: Option<&Value>, groups: &[Group]) -> Option<GroupPick> {
    let name = match value? {
        Value::Object(o) => o.get("name")?.as_str()?,
        Value::String(s) => s.as_str(),
        _ => return None,
    }
    .trim();
    if name.is_empty() {
        return None;
    }
    let found = groups
        .iter()
        .find(|g| g.name.trim().to_lowercase() == name.to_lowercase());
    Some(match found {
        Some(g) => GroupPick::Existing {
            group_id: g.id.clone(),
            name: g.name.clone(),
        },
        None => GroupPick::New { name: name.to_string() },
    })
}

/// README "Local checks" › `check` › `terms`, steps 1–8, for a page of `category_id`: drop terms
/// that don't occur in `page` (FR-KN-GL-4), that are or contain an entry that applies in the
/// category, that were ignored there, `unknown` ones written as everyday lowercase words,
/// quantities and dates, phrases of more than 4 words, duplicates, and terms in `shown`.
pub fn filter_terms(
    conn: &Connection,
    category_id: &str,
    page: &str,
    terms: Vec<RawTerm>,
    shown: &[String],
) -> Result<TermFilter> {
    let names = glossary::applicable_names(conn, category_id)?;
    let ignored: HashSet<String> = glossary::ignored_terms(conn, category_id)?
        .iter()
        .map(|t| t.to_lowercase())
        .collect();
    let shown: HashSet<String> = shown.iter().map(|t| t.trim().to_lowercase()).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = TermFilter {
        kept: Vec::new(),
        dropped: Vec::new(),
    };
    for raw in terms {
        let term = raw.term.trim().to_string();
        if term.is_empty() {
            continue;
        }
        let confidence = match raw.confidence.trim().to_lowercase().as_str() {
            "high" => Confidence::High,
            "medium" => Confidence::Medium,
            "unknown" => Confidence::Unknown,
            _ => Confidence::Low,
        };
        let key = term.to_lowercase();
        let why = if !glossary::any_name_in(std::slice::from_ref(&term), page) {
            Some("not in the text (FR-KN-GL-4)")
        } else if glossary::any_name_in(&names, &term) {
            Some("already a glossary entry, or contains one")
        } else if ignored.contains(&key) {
            Some("ignored in this category")
        } else if confidence == Confidence::Unknown && !looks_like_a_name(&term) {
            Some("unknown confidence, but written as everyday lowercase words")
        } else if is_quantity(&term) || term.split_whitespace().count() > 4 {
            Some("a quantity or date, or a phrase of more than 4 words")
        } else if seen.contains(&key) {
            Some("duplicate")
        } else if shown.contains(&key) {
            Some("already shown for this draft")
        } else {
            None
        };
        match why {
            Some(reason) => out.dropped.push((term, reason)),
            None => {
                seen.insert(key);
                out.kept.push(TermSuggestion {
                    term,
                    meaning: raw.meaning.trim().to_string(),
                    confidence,
                });
            }
        }
    }
    Ok(out)
}

/// A capital letter, a digit or a non-ASCII character: how a name or code is written.
fn looks_like_a_name(term: &str) -> bool {
    term.chars()
        .any(|c| c.is_uppercase() || c.is_ascii_digit() || !c.is_ascii())
}

/// A number with at most a short unit after it (`40 mm`, `2026-09-10`, `12.5%`).
fn is_quantity(term: &str) -> bool {
    let letters = term.chars().rev().take_while(char::is_ascii_alphabetic).count();
    if letters > 4 {
        return false;
    }
    let core = &term[..term.len() - letters];
    !core.is_empty()
        && core
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_whitespace() || ".,:/%°+-".contains(c))
}

/// One page found by the duplicate check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateHit {
    pub page_id: String,
    pub title: String,
    pub category_id: String,
    /// Similarity 0..1 (best piece pair, or the keyword fallback's score).
    pub score: f32,
    /// At or above Settings › Knowledge "likely the same topic" (FR-KN-DU-1).
    pub likely: bool,
    /// Plain text of the best-matching piece, short.
    pub snippet: String,
}

/// The pages to show (FR-KN-DU-1): one hit per page, at most `limit` (3), "likely" ones first.
/// Among the likely ones, pages of `category_id` come first; the others ("Related") go by score
/// only, so a weak match in the same category never pushes out a closer one from another category.
pub fn rank_duplicates(hits: Vec<MeaningHit>, category_id: &str, threshold: f32, limit: usize) -> Vec<DuplicateHit> {
    // The best piece of each page.
    let mut best: Vec<MeaningHit> = Vec::new();
    for hit in hits.into_iter().filter(|h| h.kind == SearchKind::Page) {
        match best.iter_mut().find(|b| b.id == hit.id) {
            Some(b) if hit.score > b.score => *b = hit,
            Some(_) => {}
            None => best.push(hit),
        }
    }
    let mut ranked: Vec<DuplicateHit> = best
        .into_iter()
        .map(|h| DuplicateHit {
            page_id: h.id,
            title: h.title,
            category_id: h.category_id,
            score: h.score,
            likely: h.score >= threshold,
            snippet: hybrid::excerpt(&h.text),
        })
        .collect();
    ranked.sort_by(|a, b| {
        let own = |h: &DuplicateHit| h.likely && h.category_id == category_id;
        b.likely
            .cmp(&a.likely)
            .then(own(b).cmp(&own(a)))
            .then(b.score.total_cmp(&a.score))
            .then_with(|| a.page_id.cmp(&b.page_id))
    });
    ranked.truncate(limit);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::glossary::{NewGlossaryEntry, create, ignore_term};
    use serde_json::json;

    fn object(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn group(id: &str, name: &str) -> Group {
        Group {
            id: id.into(),
            category_id: "p".into(),
            parent_id: None,
            name: name.into(),
            description: String::new(),
            path: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn read_check_cleans_topics_and_one_topic() {
        let (r, _) = read_check(&object(json!({"topics": [" a ", "", 3, "b"], "one_topic": false})), &[], "T");
        assert_eq!(r.topics, ["a", "b"]);
        assert!(!r.one_topic);
        // Anything but an explicit false is one topic.
        for v in [json!("no"), json!(null), json!(0)] {
            let (r, _) = read_check(&object(json!({"topics": ["a", "b"], "one_topic": v})), &[], "T");
            assert!(r.one_topic);
        }
        let (r, _) = read_check(&object(json!({"topics": ["a", "b"]})), &[], "T");
        assert!(r.one_topic);
        // No topics, or a single one: the page counts as one topic.
        let (r, _) = read_check(&object(json!({"topics": [], "one_topic": false})), &[], "T");
        assert!(r.one_topic && r.topics.is_empty());
        let (r, _) = read_check(&object(json!({"topics": ["a"], "one_topic": false})), &[], "T");
        assert!(r.one_topic);
        let (r, terms) = read_check(&Map::new(), &[], "T");
        assert!(r.one_topic && r.group.is_none() && r.tags.is_empty() && terms.is_empty());
    }

    #[test]
    fn read_check_matches_groups_case_insensitively() {
        let groups = [group("g1", "Hose Safety"), group("g2", "Other")];
        let pick = |v: Value| read_check(&object(json!({ "group": v })), &groups, "T").0.group;
        let existing = Some(GroupPick::Existing {
            group_id: "g1".into(),
            name: "Hose Safety".into(),
        });
        assert_eq!(pick(json!({"name": "hose safety", "existing": true})), existing);
        // `existing` says false, but the name is a group.
        assert_eq!(pick(json!({"name": " HOSE SAFETY ", "existing": false})), existing);
        // `existing: true` for a name that is not a group: a new name.
        assert_eq!(
            pick(json!({"name": "Pumps", "existing": true})),
            Some(GroupPick::New { name: "Pumps".into() })
        );
        assert_eq!(pick(json!("pumps")), Some(GroupPick::New { name: "pumps".into() }));
        assert_eq!(pick(json!(null)), None);
        assert_eq!(pick(json!({"name": "  "})), None);
        assert_eq!(pick(json!({"existing": true})), None);
        assert_eq!(pick(json!(5)), None);
    }

    #[test]
    fn read_check_tags() {
        let groups = [group("g1", "Hose Safety")];
        let reply = object(json!({
            "group": {"name": "hose safety", "existing": true},
            "tags": [" Hoses ", "hoses", "HOSE SAFETY", "My Title", "a", "b", "c", "d", 7, ""],
        }));
        let (r, _) = read_check(&reply, &groups, "my title");
        assert_eq!(r.tags, ["hoses", "a", "b", "c"]);
        // Without a group, only the title is excluded.
        let (r, _) = read_check(&object(json!({"tags": ["Hose Safety", "x"]})), &groups, "T");
        assert_eq!(r.tags, ["hose safety", "x"]);
    }

    #[test]
    fn read_check_raw_terms() {
        let reply = object(json!({"terms": [
            {"term": " MMU ", "meaning": "unit", "confidence": "HIGH"},
            {"term": "X"},
            {"meaning": "no term"},
            {"term": 5},
            "text",
        ]}));
        let (_, terms) = read_check(&reply, &[], "T");
        assert_eq!(terms.len(), 2);
        assert_eq!(terms[0].term, " MMU ");
        assert_eq!(terms[1].meaning, "");
        assert_eq!(terms[1].confidence, "");
    }

    fn setup() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        for (id, key) in [("p1", "ONE"), ("p2", "TWO")] {
            db.conn()
                .execute(
                    "INSERT INTO categories (id, name, key, created_at, updated_at) VALUES (?1, ?1, ?2, 'x', 'x')",
                    [id, key],
                )
                .unwrap();
        }
        let entry = |category: Option<&str>, term: &str, other: &[&str]| NewGlossaryEntry {
            category_id: category.map(str::to_string),
            term: term.into(),
            other_names: Some(other.iter().map(|s| s.to_string()).collect()),
            description: Some("d".into()),
        };
        create(&mut db, entry(None, "MMU", &["Mobile Manufacturing Unit"])).unwrap();
        create(&mut db, entry(Some("p1"), "AN", &[])).unwrap();
        (dir, db)
    }

    fn raw(term: &str, confidence: &str) -> RawTerm {
        RawTerm {
            term: term.into(),
            meaning: " m ".into(),
            confidence: confidence.into(),
        }
    }

    fn run(db: &Db, category: &str, page: &str, terms: Vec<RawTerm>, shown: &[&str]) -> TermFilter {
        let shown: Vec<String> = shown.iter().map(|s| s.to_string()).collect();
        filter_terms(db.conn(), category, page, terms, &shown).unwrap()
    }

    fn reasons(f: &TermFilter) -> Vec<(&str, &str)> {
        f.dropped.iter().map(|(t, r)| (t.as_str(), *r)).collect()
    }

    fn kept(f: &TermFilter) -> Vec<&str> {
        f.kept.iter().map(|t| t.term.as_str()).collect()
    }

    #[test]
    fn filter_keeps_a_good_term_with_trimmed_fields() {
        let (_d, db) = setup();
        let f = run(&db, "p1", "The Boom Clamp holds it.", vec![raw("  Boom Clamp ", "bogus")], &[]);
        assert_eq!(
            f.kept,
            [TermSuggestion {
                term: "Boom Clamp".into(),
                meaning: "m".into(),
                confidence: Confidence::Low
            }]
        );
        assert!(f.dropped.is_empty());
        let f = run(&db, "p1", "Boom Clamp", vec![raw("Boom Clamp", "unknown")], &[]);
        assert_eq!(f.kept[0].confidence, Confidence::Unknown);
    }

    #[test]
    fn filter_drops_terms_not_in_the_text() {
        let (_d, db) = setup();
        let page = "Check the nozzles of PMP-07. Use pumps often. 用RAG检索";
        let terms = ["Valve", "PMP", "Nozzle", "pump", "nozz", "RAG"]
            .iter()
            .map(|t| raw(t, "high"))
            .collect();
        let f = run(&db, "p1", page, terms, &[]);
        // PMP-07 is a code of PMP, plurals count, "nozz" is inside a word, Chinese neighbours are
        // no word characters.
        assert_eq!(kept(&f), ["PMP", "Nozzle", "pump", "RAG"]);
        assert_eq!(f.dropped.len(), 2);
        assert!(f.dropped.iter().all(|(_, r)| r.starts_with("not in the text")));
    }

    #[test]
    fn filter_capitals_match_only_in_capitals() {
        let (_d, db) = setup();
        let f = run(&db, "p1", "the ugv is here", vec![raw("UGV", "high")], &[]);
        assert!(f.kept.is_empty());
        let f = run(&db, "p1", "the UGV is here", vec![raw("UGV", "high")], &[]);
        assert_eq!(f.kept.len(), 1);
    }

    #[test]
    fn filter_drops_glossary_terms_that_apply_in_the_category() {
        let (_d, db) = setup();
        let page = "MMUs and the AN tank, and the Mobile Manufacturing Unit.";
        let terms = vec![raw("MMUs", "high"), raw("AN tank", "high"), raw("Mobile Manufacturing Unit", "high")];
        let f = run(&db, "p1", page, terms.clone(), &[]);
        assert!(f.kept.is_empty());
        assert_eq!(f.dropped.len(), 3);
        // AN is an entry of p1 only: in p2 "AN tank" is a fine suggestion, MMU is global.
        let f = run(&db, "p2", page, terms, &[]);
        assert_eq!(kept(&f), ["AN tank"]);
    }

    #[test]
    fn filter_drops_ignored_terms_of_this_category() {
        let (_d, mut db) = setup();
        ignore_term(&mut db, "p1", "boom clamp").unwrap();
        let terms = vec![raw("Boom Clamp", "high")];
        let f = run(&db, "p1", "Boom Clamp", terms.clone(), &[]);
        assert_eq!(reasons(&f), [("Boom Clamp", "ignored in this category")]);
        assert_eq!(run(&db, "p2", "Boom Clamp", terms, &[]).kept.len(), 1);
    }

    #[test]
    fn filter_unknown_needs_a_name_like_spelling() {
        let (_d, db) = setup();
        let page = "boom clamp, Boom Clamp, KX9, a7 valve, 泵站 and Zürich";
        let terms = ["boom clamp", "Boom Clamp", "KX9", "a7 valve", "泵站", "Zürich"]
            .iter()
            .map(|t| raw(t, "unknown"))
            .collect();
        let f = run(&db, "p1", page, terms, &[]);
        assert_eq!(kept(&f), ["Boom Clamp", "KX9", "a7 valve", "泵站", "Zürich"]);
        assert_eq!(reasons(&f).len(), 1);
        assert_eq!(reasons(&f)[0].0, "boom clamp");
        // The same lowercase words with another confidence are kept.
        assert_eq!(run(&db, "p1", page, vec![raw("boom clamp", "low")], &[]).kept.len(), 1);
    }

    #[test]
    fn filter_drops_quantities_dates_and_long_phrases() {
        let (_d, db) = setup();
        let page = "40 mm, 2026-09-10, 12.5%, 3 bar, 10 metres, 7, a b c d e, a b c d, 40 mm hose";
        let terms = ["40 mm", "2026-09-10", "12.5%", "3 bar", "10 metres", "7", "a b c d e", "a b c d", "40 mm hose"]
            .iter()
            .map(|t| raw(t, "high"))
            .collect();
        let f = run(&db, "p1", page, terms, &[]);
        // "10 metres" has a 6-letter unit: not a short unit, so it stays.
        assert_eq!(kept(&f), ["10 metres", "a b c d", "40 mm hose"]);
        assert_eq!(f.dropped.len(), 6);
    }

    #[test]
    fn filter_deduplicates_and_skips_shown_terms() {
        let (_d, db) = setup();
        let page = "Boom Clamp and boom clamp and Pump Head and Valve Seat";
        let terms = vec![
            raw("Boom Clamp", "high"),
            raw("boom clamp", "low"),
            raw("Pump Head", "high"),
            raw("Valve Seat", "high"),
            raw("  ", "high"),
        ];
        let f = run(&db, "p1", page, terms, &["pump head"]);
        assert_eq!(kept(&f), ["Boom Clamp", "Valve Seat"]);
        assert_eq!(
            reasons(&f),
            [("boom clamp", "duplicate"), ("Pump Head", "already shown for this draft")]
        );
    }

    fn hit(id: &str, category: &str, score: f32) -> MeaningHit {
        MeaningHit {
            kind: SearchKind::Page,
            id: id.into(),
            title: format!("Page {id}"),
            key: None,
            category_id: category.into(),
            chunk_id: 1,
            heading: String::new(),
            text: format!("**text** of {id}"),
            score,
        }
    }

    #[test]
    fn rank_one_hit_per_page_best_piece() {
        let hits = vec![hit("a", "p", 0.5), hit("a", "p", 0.9), hit("a", "p", 0.7), hit("b", "p", 0.6)];
        let r = rank_duplicates(hits, "p", 0.8, 3);
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].page_id.as_str(), r[0].score, r[0].likely), ("a", 0.9, true));
        assert_eq!((r[1].page_id.as_str(), r[1].likely), ("b", false));
        assert_eq!(r[0].snippet, "text of a");
    }

    #[test]
    fn rank_likely_first_same_category_among_them_then_score() {
        let hits = vec![
            hit("far-high", "q", 0.70),
            hit("same-low", "p", 0.60),
            hit("far-likely", "q", 0.85),
            hit("same-likely", "p", 0.81),
            hit("same-high", "p", 0.75),
        ];
        let r = rank_duplicates(hits, "p", 0.8, 10);
        let ids: Vec<_> = r.iter().map(|h| h.page_id.as_str()).collect();
        // Both likely ones first (the same category's before the other's, though its score is
        // lower), then the rest by score only: a weak same-category page doesn't push out a closer
        // one from another category.
        assert_eq!(ids, ["same-likely", "far-likely", "same-high", "far-high", "same-low"]);
    }

    #[test]
    fn rank_limit_threshold_and_kinds() {
        let mut task = hit("t", "p", 0.99);
        task.kind = SearchKind::Task;
        let hits = vec![hit("a", "p", 0.8), hit("b", "p", 0.79), hit("c", "p", 0.1), hit("d", "p", 0.2), task];
        let r = rank_duplicates(hits.clone(), "p", 0.8, 3);
        assert_eq!(r.iter().map(|h| h.page_id.as_str()).collect::<Vec<_>>(), ["a", "b", "d"]);
        // The threshold is inclusive.
        assert!(r[0].likely && !r[1].likely);
        assert!(rank_duplicates(hits, "p", 0.8, 0).is_empty());
        assert!(rank_duplicates(Vec::new(), "p", 0.8, 3).is_empty());
    }
}
