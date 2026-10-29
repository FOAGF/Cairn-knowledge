//! FR-RAG-SR-4: merging keyword and meaning results with reciprocal rank fusion.
//!
//! Each list is already one entry per item, best first. An item's fused score is
//! `sum(1 / (K + rank))` over the lists it appears in (rank from 1), so no score tuning is needed.
//! The snippet is the keyword snippet when the item matched by keyword (it shows the matched
//! words), otherwise a short excerpt of the best piece.
//!
//! Owned by the M4 search-core agent.

use std::collections::HashMap;

use crate::search::{MatchedBy, SearchHit, SearchKind};
use crate::vectors::MeaningHit;

/// The usual RRF constant.
pub const K: f64 = 60.0;

/// Merged results, best first, at most `limit`. `matched_by` tells where each came from.
/// One entry per item; equal scores keep keyword-list order first.
pub fn fuse(keyword: Vec<SearchHit>, meaning: Vec<MeaningHit>, limit: u32) -> Vec<SearchHit> {
    let mut order: Vec<SearchHit> = Vec::with_capacity(keyword.len() + meaning.len());
    let mut index: HashMap<(SearchKind, String), usize> = HashMap::new();
    for (i, mut hit) in keyword.into_iter().enumerate() {
        let key = (hit.kind, hit.id.clone());
        if index.contains_key(&key) {
            continue;
        }
        hit.score = 1.0 / (K + (i + 1) as f64);
        hit.matched_by = MatchedBy::Keyword;
        index.insert(key, order.len());
        order.push(hit);
    }
    for (i, m) in meaning.into_iter().enumerate() {
        let add = 1.0 / (K + (i + 1) as f64);
        match index.get(&(m.kind, m.id.clone())) {
            Some(&at) => {
                let hit = &mut order[at];
                if hit.matched_by == MatchedBy::Meaning {
                    continue; // the same item twice in the meaning list: keep the first rank
                }
                hit.score += add;
                hit.matched_by = MatchedBy::Both;
            }
            None => {
                index.insert((m.kind, m.id.clone()), order.len());
                order.push(SearchHit {
                    kind: m.kind,
                    id: m.id,
                    title: m.title,
                    key: m.key,
                    category_id: m.category_id,
                    snippet: excerpt(&m.text),
                    score: add,
                    matched_by: MatchedBy::Meaning,
                });
            }
        }
    }
    order.sort_by(|a, b| b.score.total_cmp(&a.score)); // stable
    order.truncate(limit as usize);
    order
}

/// Longest excerpt, in characters.
const EXCERPT_CHARS: usize = 160;

/// Piece text as plain words for a snippet: table separator rows, heading and quote markers,
/// emphasis and code marks are dropped; table cells are joined with " · ".
fn plain_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim();
        // |---|:---:| rows carry no words.
        if t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' ')) {
            continue;
        }
        let t = t.trim_start_matches(['#', '>']).trim_start();
        let t = t.trim_matches('|').replace('|', " · ");
        out.extend(
            t.chars()
                .filter(|c| !matches!(c, '«' | '»' | '*' | '`' | '~')),
        );
        out.push(' ');
    }
    out
}

/// A short plain-text excerpt (about 160 characters, cut at a word) of a piece, for meaning-only hits.
/// White space is collapsed, Markdown marks and the `«»` match markers are removed.
pub fn excerpt(text: &str) -> String {
    let plain = plain_words(text);
    let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if plain.chars().count() <= EXCERPT_CHARS {
        return plain;
    }
    let cut = plain
        .char_indices()
        .nth(EXCERPT_CHARS)
        .map_or(plain.len(), |(i, _)| i);
    // Back up to a space if there is one in the second half; Chinese text has none: hard cut.
    let head = &plain[..cut];
    let at_word = plain[cut..].starts_with(' ');
    let end = if at_word {
        cut
    } else {
        match head.rfind(' ') {
            Some(i) if i > head.len() / 2 => i,
            _ => cut,
        }
    };
    format!("{}…", plain[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(id: &str) -> SearchHit {
        SearchHit {
            kind: SearchKind::Page,
            id: id.into(),
            title: format!("T {id}"),
            key: None,
            category_id: "p".into(),
            snippet: format!("«{id}» keyword snippet"),
            score: 9.9,
            matched_by: MatchedBy::Keyword,
        }
    }

    fn mh(id: &str, kind: SearchKind) -> MeaningHit {
        MeaningHit {
            kind,
            id: id.into(),
            title: format!("T {id}"),
            key: None,
            category_id: "p".into(),
            chunk_id: 1,
            heading: "H".into(),
            text: format!("meaning text of {id}"),
            score: 0.5,
        }
    }

    #[test]
    fn rrf_order_both_and_snippets() {
        // keyword: a, b, c ; meaning: c, d, a
        let out = fuse(
            vec![kw("a"), kw("b"), kw("c")],
            vec![
                mh("c", SearchKind::Page),
                mh("d", SearchKind::Page),
                mh("a", SearchKind::Page),
            ],
            10,
        );
        let ids: Vec<&str> = out.iter().map(|h| h.id.as_str()).collect();
        // a: 1/61 + 1/63 ; c: 1/63 + 1/61 (equal, a first) ; b: 1/62 ; d: 1/62
        assert_eq!(ids, ["a", "c", "b", "d"]);
        let by: Vec<MatchedBy> = out.iter().map(|h| h.matched_by).collect();
        assert_eq!(
            by,
            [
                MatchedBy::Both,
                MatchedBy::Both,
                MatchedBy::Keyword,
                MatchedBy::Meaning
            ]
        );
        assert!((out[0].score - (1.0 / 61.0 + 1.0 / 63.0)).abs() < 1e-12);
        assert_eq!(out[0].snippet, "«a» keyword snippet");
        assert_eq!(out[3].snippet, "meaning text of d");
        assert!(!out[3].snippet.contains('«'));
    }

    #[test]
    fn pages_and_tasks_with_the_same_id_are_different_items() {
        let out = fuse(vec![kw("x")], vec![mh("x", SearchKind::Task)], 10);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn limit_and_empty_lists() {
        let out = fuse(vec![kw("a"), kw("b"), kw("c")], vec![], 2);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|h| h.matched_by == MatchedBy::Keyword));
        assert!(fuse(vec![], vec![], 5).is_empty());
        let m = fuse(
            vec![],
            vec![mh("a", SearchKind::Page), mh("b", SearchKind::Page)],
            5,
        );
        assert!(m.iter().all(|h| h.matched_by == MatchedBy::Meaning));
        assert_eq!(m[0].id, "a");
    }

    #[test]
    fn excerpt_cuts_at_a_word() {
        assert_eq!(excerpt("  short \n text "), "short text");
        assert_eq!(excerpt("a «b» c"), "a b c");
        let long = "lorem ipsum ".repeat(40);
        let e = excerpt(&long);
        assert!(e.ends_with('…'));
        assert!(e.chars().count() <= EXCERPT_CHARS + 1);
        assert!(
            e.trim_end_matches('…').ends_with("ipsum")
                || e.trim_end_matches('…').ends_with("lorem")
        );
        assert_eq!(
            excerpt("1. Warm up for **15 minutes**, run `calib`.\n## Next\n> note"),
            "1. Warm up for 15 minutes, run calib. Next note"
        );
        assert_eq!(
            excerpt("| Table | Key |\n|---|:---:|\n| readings | month |"),
            "Table · Key readings · month"
        );
        let zh = "液压软管".repeat(100);
        let e = excerpt(&zh);
        assert_eq!(e.chars().count(), EXCERPT_CHARS + 1);
    }
}
