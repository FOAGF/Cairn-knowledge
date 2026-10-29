//! The sources of an answer (FR-LLM-AS-1, FR-RAG-SR-2, D-44): which pieces of which pages and
//! tasks are sent with a question.
//!
//! Rules (spec FR-LLM-AS-1; `poc/i-assistant/relevance_check.py`):
//! - Keyword search (all words ANDed, `search::search`; any word when that finds nothing) and
//!   search by meaning (the query vector, `vectors`) over the filtered candidates (scope ∩ plan
//!   filters).
//! - **Minimum relevance:** a result found only by meaning needs a similarity of at least
//!   `rule.floor` (Settings, default 0.40) and within `rule.margin` (0.30) of the best result. A
//!   keyword result that matched only through its glossary lines (the index's `extra` column) must
//!   pass the same meaning rule. Other keyword results pass.
//! - **Pinned items** (the plan's `refs`, the items cited in the previous answer, the items the
//!   message references) always pass and come first.
//! - **A pinned item is sent whole** (FR-LLM-AS-14, D-83): all its pieces in order (a task's
//!   description, then its comments with their dates). One item takes at most half of
//!   `budget_tokens`; a longer one keeps its first piece, then the pieces that best match (the
//!   earlier one on a tie), and its last piece sent says what was left out ([`LEFT_OUT`]).
//! - Pieces, best first, fill `budget_tokens` (`chunking::estimate_tokens`); a few pieces of one
//!   item may go when they all pass; a keyword-only item sends its best piece by meaning (or its
//!   first piece when there is no vector).
//! - Items that passed but did not fit, and the best others found, are *N more searched*.
//! - Snippets come from the piece's own text, never its glossary lines.
//! - Glossary entries the search finds are not sources; their ids go to `terms` for the glossary
//!   block (FR-KN-GL-5, README `sources`).
//!
//! Without a query vector (the search model is not ready), only keyword results are used.
//!
//! How "matched only through its glossary lines" is told: the index's `extra` column mixes the
//! item's own words (task key, labels, tags, source info) with the glossary lines of the terms it
//! uses. For a keyword hit, the words of the query are looked up in the item's own text (title,
//! its pieces, key, labels, tags, source info) with `search::words_in`; when a word (any word, in
//! the any-word retry) is found nowhere there, the hit is glossary-only.
//!
//! Owned by the M6 answers agent.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};

use crate::Result;
use crate::assistant::find;
use crate::assistant::formats::{ItemInfo, PiecePart, SourcePiece};
use crate::assistant::items::{self, ItemRef};
use crate::assistant::parts::{SourceKind, SourceRef};
use crate::assistant::plan::PlanFilters;
use crate::chunking::{self, estimate_tokens};
use crate::hybrid;
use crate::search::{self, SearchHit, SearchKind, SearchQuery};
use crate::vectors::{self, MeaningHit};

/// Appended to the last piece sent of a pinned item that was cut (D-83). `{parts}` names the parts
/// left out as the sources' second line does (`Comment 2026-09-01`, `Section: Setup > Proxy`), and
/// `{count}` counts those names. Written by an Opus sub-agent (the owner's rule for prompt text).
const LEFT_OUT: &str = "[Note from the app: this item is too long to send in full, so it was sent without {count} of its parts: {parts}. Do not guess what is missing; if it may hold what you need, say what was not sent, so the user can paste it.]";

/// Items asked of each search.
const SEARCH_LIMIT: u32 = 40;
/// A budget above this is "no budget" (a find lists every match): the searches look deeper.
const UNBOUNDED_BUDGET: usize = 1_000_000;
const DEEP_SEARCH_LIMIT: u32 = 500;
/// Pieces of one item, the best one included.
const MAX_PIECES_PER_ITEM: usize = 3;
/// Items shown under *N more searched* besides those that did not fit.
const MORE_OTHERS: usize = 5;
/// Glossary entries at most added by the search.
const MAX_TERMS: usize = 5;
/// What a source's head line adds to its text, in tokens.
const HEAD_TOKENS: usize = 40;

/// FR-LLM-AS-1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelevanceRule {
    /// `Settings::assistant_min_relevance`.
    pub floor: f32,
    /// `settings::RELEVANCE_MARGIN`.
    pub margin: f32,
}

impl RelevanceRule {
    /// Whether `score` passes when the best score is `best`.
    pub fn passes(&self, score: f32, best: f32) -> bool {
        score >= self.floor && score >= best - self.margin
    }
}

#[derive(Debug, Clone)]
pub struct Retrieval<'a> {
    /// The plan's search words; `""` = only the pinned items.
    pub words: &'a str,
    /// The words embedded by the search model; `None` when it is unavailable.
    pub query_vector: Option<&'a [f32]>,
    /// The conversation's scope; empty = every category (or *Search all categories*).
    pub scope_category_ids: &'a [String],
    pub filters: &'a PlanFilters,
    pub rule: RelevanceRule,
    pub budget_tokens: usize,
    /// Always sent, first, in this order.
    pub pinned: &'a [ItemRef],
}

#[derive(Debug, Clone, Default)]
pub struct Retrieved {
    /// The sources for the prompt, in order (`[1]` first).
    pub pieces: Vec<SourcePiece>,
    /// The same sources for `AnswerPart::sources`.
    pub sources: Vec<SourceRef>,
    /// *N more searched*.
    pub more: Vec<SourceRef>,
    /// The items behind the sources, distinct, in order (for `ask-action` candidates).
    pub items: Vec<ItemRef>,
    /// Glossary entries the search found (they join the glossary block).
    pub term_ids: Vec<String>,
}

/// One piece of an item as stored, classified.
struct Piece {
    part: PiecePart,
    text: String,
    /// Similarity to the query vector; `None` without a vector on either side.
    sim: Option<f32>,
    /// How many query words the text has (the order when there is no similarity).
    word_hits: usize,
}

struct Loaded {
    pieces: Vec<Piece>,
    /// The item's own words for the glossary-only check.
    own_text: String,
}

fn dot(q: &[f32], v: &[f32]) -> Option<f32> {
    (q.len() == v.len()).then(|| q.iter().zip(v).map(|(a, b)| a * b).sum())
}

/// The date of an ISO timestamp in the computer's time zone (`YYYY-MM-DD`).
fn local_day(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(t) => t
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d")
            .to_string(),
        Err(_) => iso.chars().take(10).collect(),
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .chars()
        .take(80)
        .collect()
}

type StoredPiece = (String, String, Option<Vec<f32>>);

/// An item's stored pieces with their vectors (cut again from the live text when it has none).
fn stored_pieces(
    conn: &Connection,
    kind: SourceKind,
    id: &str,
) -> Result<Vec<StoredPiece>> {
    let k = if kind == SourceKind::Task { "task" } else { "page" };
    let rows = conn
        .prepare_cached(
            "SELECT heading, text, vector FROM chunks WHERE kind = ?1 AND ref_id = ?2 ORDER BY seq",
        )?
        .query_map(params![k, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?.map(|b| vectors::from_blob(&b)),
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The pieces of one page or task, classified (description / comment with date / section path),
/// scored against the query. `None` when the item no longer exists.
fn load(
    conn: &Connection,
    r: &ItemRef,
    words: &str,
    query: Option<&[f32]>,
) -> Result<Option<Loaded>> {
    let score = |text: &str, vector: &Option<Vec<f32>>| {
        (
            query.zip(vector.as_deref()).and_then(|(q, v)| dot(q, v)),
            search::words_in(words, text).iter().filter(|b| **b).count(),
        )
    };
    let mut pieces = Vec::new();
    #[allow(clippy::needless_late_init)]
    let own_text;
    match r.kind {
        SourceKind::Page => {
            let row = conn
                .query_row(
                    "SELECT title, body, source_info,
                            (SELECT group_concat(tag, ' ') FROM page_tags WHERE page_id = pages.id)
                     FROM pages WHERE id = ?1",
                    [&r.id],
                    |x| {
                        Ok((
                            x.get::<_, String>(0)?,
                            x.get::<_, String>(1)?,
                            x.get::<_, Option<String>>(2)?,
                            x.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .ok();
            let Some((title, body, source, tags)) = row else {
                return Ok(None);
            };
            let mut stored = stored_pieces(conn, SourceKind::Page, &r.id)?;
            if stored.is_empty() {
                stored = chunking::split_page(&body)
                    .into_iter()
                    .map(|p| (p.heading, p.text, None))
                    .collect();
            }
            for (heading, text, vector) in stored {
                let (sim, word_hits) = score(&text, &vector);
                let mut path: Vec<String> = heading
                    .split(" › ")
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                // The body's `# title` line is the first heading: the page's own title is not a
                // section ("Section: (top)" in the prompt, no part on the chip).
                if path.first().is_some_and(|h| h.trim().eq_ignore_ascii_case(title.trim())) {
                    path.remove(0);
                }
                pieces.push(Piece { part: PiecePart::Section(path), text, sim, word_hits });
            }
            let mut own = vec![title];
            own.extend(pieces.iter().map(|p| p.text.clone()));
            own.extend(source);
            own.extend(tags);
            own_text = own.join("\n");
        }
        SourceKind::Task => {
            let row = conn
                .query_row(
                    "SELECT key, title, status, description,
                            (SELECT group_concat(label, ' ') FROM task_labels WHERE task_id = tasks.id)
                     FROM tasks WHERE id = ?1",
                    [&r.id],
                    |x| {
                        Ok((
                            x.get::<_, String>(0)?,
                            x.get::<_, String>(1)?,
                            x.get::<_, String>(2)?,
                            x.get::<_, String>(3)?,
                            x.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .ok();
            let Some((key, title, status, desc, labels)) = row else {
                return Ok(None);
            };
            let comments: Vec<(String, String)> = conn
                .prepare_cached(
                    "SELECT created_at, text FROM task_comments WHERE task_id = ?1 ORDER BY created_at, id",
                )?
                .query_map([&r.id], |x| Ok((x.get(0)?, x.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            let mut stored = stored_pieces(conn, SourceKind::Task, &r.id)?;
            let texts: Vec<String> = comments.iter().map(|c| c.1.clone()).collect();
            if stored.is_empty() {
                stored = chunking::split_task(&key, &title, &status, &desc, &texts)
                    .into_iter()
                    .map(|p| (p.heading, p.text, None))
                    .collect();
            }
            // The first pieces are the description's; the rest hold the comments.
            let desc_count = chunking::split_task(&key, &title, &status, &desc, &[]).len();
            let header = format!("{key}: {title} ({status})");
            let marks: Vec<(String, String)> = comments
                .iter()
                .map(|(at, text)| (first_line(&crate::search::strip_markdown(text)), local_day(at)))
                .collect();
            let mut last_day = marks.first().map(|m| m.1.clone()).unwrap_or_default();
            for (i, (_, text, vector)) in stored.into_iter().enumerate() {
                let (sim, word_hits) = score(&text, &vector);
                if i < desc_count {
                    let body = if i == 0 {
                        text.strip_prefix(&header).unwrap_or(&text).trim().to_string()
                    } else {
                        text
                    };
                    pieces.push(Piece { part: PiecePart::Description, text: body, sim, word_hits });
                    continue;
                }
                // The comments that start in this piece, in order.
                let mut found: Vec<(usize, &str)> = marks
                    .iter()
                    .filter(|(line, _)| !line.is_empty())
                    .filter_map(|(line, day)| text.find(line.as_str()).map(|at| (at, day.as_str())))
                    .collect();
                found.sort_by_key(|f| f.0);
                let first_day = found.first().map_or(last_day.clone(), |f| f.1.to_string());
                // A later comment of another day is marked in the text.
                let mut shown = text.clone();
                for (at, day) in found.iter().skip(1).rev() {
                    if *day != first_day {
                        shown.insert_str(*at, &format!("[{day}] "));
                    }
                }
                if let Some(f) = found.last() {
                    last_day = f.1.to_string();
                }
                pieces.push(Piece { part: PiecePart::Comment(first_day), text: shown, sim, word_hits });
            }
            let mut own = vec![key, title];
            own.extend(labels);
            own.extend(pieces.iter().map(|p| p.text.clone()));
            own_text = own.join("\n");
        }
        _ => return Ok(None),
    }
    Ok(Some(Loaded { pieces, own_text }))
}

/// The piece as the chip names it; `None` for the part of a page before its first section.
fn part_label(part: &PiecePart) -> Option<String> {
    Some(match part {
        PiecePart::Description => "Description".into(),
        PiecePart::Comment(day) => format!("Comment {day}"),
        PiecePart::Section(path) if path.is_empty() => return None,
        PiecePart::Section(path) => path.join(" › "),
        PiecePart::File { index, count, .. } => format!("Part {index} of {count}"),
    })
}

fn source_ref(info: &ItemInfo, part: Option<String>, snippet: &str) -> SourceRef {
    match info {
        ItemInfo::Task(t) => SourceRef {
            kind: SourceKind::Task,
            id: Some(t.id.clone()),
            title: t.title.clone(),
            key: Some(t.key.clone()),
            category_id: None,
            part,
            snippet: hybrid::excerpt(snippet),
            url: None,
        },
        ItemInfo::Page(p) => SourceRef {
            kind: SourceKind::Page,
            id: Some(p.id.clone()),
            title: p.title.clone(),
            key: None,
            category_id: None,
            part,
            snippet: hybrid::excerpt(snippet),
            url: None,
        },
    }
}

/// An item as the `more` list shows it, from a search hit.
fn hit_ref(h: &SearchHit, snippet: String) -> SourceRef {
    SourceRef {
        kind: if h.kind == SearchKind::Task { SourceKind::Task } else { SourceKind::Page },
        id: Some(h.id.clone()),
        title: h.title.clone(),
        key: h.key.clone(),
        category_id: Some(h.category_id.clone()),
        part: None,
        snippet: hybrid::excerpt(&snippet),
        url: None,
    }
}

fn kind_of(k: SearchKind) -> SourceKind {
    if k == SearchKind::Task { SourceKind::Task } else { SourceKind::Page }
}

pub fn retrieve(conn: &Connection, r: &Retrieval<'_>) -> Result<Retrieved> {
    let scope: Vec<String> = if r.filters.category_ids.is_empty() {
        r.scope_category_ids.to_vec()
    } else {
        r.filters.category_ids.clone()
    };
    let category_ids = (!scope.is_empty()).then(|| scope.clone());
    let kinds: Vec<SearchKind> = if r.filters.kinds.is_empty() {
        vec![SearchKind::Page, SearchKind::Task]
    } else {
        r.filters.kinds.clone()
    };
    let allowed: Option<HashSet<(SourceKind, String)>> = if find::has_item_filters(r.filters) {
        Some(
            find::rows(conn, &scope, r.filters)?
                .into_iter()
                .map(|row| (row.item.kind, row.item.id))
                .collect(),
        )
    } else {
        None
    };
    let ok = |kind: SearchKind, id: &str| {
        allowed
            .as_ref()
            .is_none_or(|a| a.contains(&(kind_of(kind), id.to_string())))
    };

    let words = r.words.trim();
    let limit = if r.budget_tokens > UNBOUNDED_BUDGET {
        DEEP_SEARCH_LIMIT
    } else {
        SEARCH_LIMIT
    };
    let query = |kinds: Vec<SearchKind>, limit: u32| SearchQuery {
        text: words.to_string(),
        kinds: Some(kinds),
        category_ids: category_ids.clone(),
        include_archived: Some(false),
        include_closed: None,
        limit: Some(limit),
    };

    // ---- what the searches find
    let mut keyword: Vec<SearchHit> = Vec::new();
    let mut any_word = false;
    let mut meaning: Vec<MeaningHit> = Vec::new();
    let mut term_ids: Vec<String> = Vec::new();
    if !words.is_empty() {
        let q = query(kinds.clone(), limit);
        keyword = search::search(conn, &q)?;
        if keyword.is_empty() {
            keyword = search::search_any(conn, &q)?;
            any_word = true;
        }
        keyword.retain(|h| ok(h.kind, &h.id));
        if let Some(v) = r.query_vector {
            meaning = vectors::search(conn, v, &query(kinds.clone(), limit))?;
            meaning.retain(|h| ok(h.kind, &h.id));
        }
        // Glossary entries: keyword hits count, meaning hits must pass the rule among themselves.
        let tq = query(vec![SearchKind::Term], MAX_TERMS as u32);
        for h in search::search(conn, &tq)? {
            term_ids.push(h.id);
        }
        if let Some(v) = r.query_vector {
            let hits = vectors::search(conn, v, &query(vec![SearchKind::Term], MAX_TERMS as u32))?;
            let best = hits.first().map_or(0.0, |h| h.score);
            for h in hits {
                if r.rule.passes(h.score, best) && !term_ids.contains(&h.id) {
                    term_ids.push(h.id);
                }
            }
        }
        term_ids.truncate(MAX_TERMS);
    }

    // ---- which results pass the relevance rule
    let best = meaning.iter().map(|h| h.score).fold(f32::MIN, f32::max);
    let meaning_pass: HashMap<(SourceKind, String), f32> = meaning
        .iter()
        .filter(|h| r.rule.passes(h.score, best))
        .map(|h| ((kind_of(h.kind), h.id.clone()), h.score))
        .collect();
    let mut keyword_pass: Vec<SearchHit> = Vec::new();
    for h in &keyword {
        let item = ItemRef { kind: kind_of(h.kind), id: h.id.clone() };
        let glossary_only = match load(conn, &item, words, None)? {
            Some(l) => {
                let flags = search::words_in(words, &l.own_text);
                if any_word {
                    !flags.iter().any(|b| *b)
                } else {
                    !flags.iter().all(|b| *b)
                }
            }
            None => continue,
        };
        if !glossary_only || meaning_pass.contains_key(&(item.kind, item.id.clone())) {
            keyword_pass.push(h.clone());
        }
    }
    let meaning_ok: Vec<MeaningHit> = meaning
        .iter()
        .filter(|h| meaning_pass.contains_key(&(kind_of(h.kind), h.id.clone())))
        .cloned()
        .collect();
    let fused = hybrid::fuse(keyword_pass.clone(), meaning_ok, limit * 2);

    // ---- the items, pinned first
    let mut order: Vec<ItemRef> = Vec::new();
    let mut seen: HashSet<ItemRef> = HashSet::new();
    let pinned: HashSet<&ItemRef> = r.pinned.iter().collect();
    for p in r.pinned {
        if seen.insert(p.clone()) {
            order.push(p.clone());
        }
    }
    for h in &fused {
        let item = ItemRef { kind: kind_of(h.kind), id: h.id.clone() };
        if seen.insert(item.clone()) {
            order.push(item);
        }
    }

    // ---- pieces of each, the best first
    struct Chosen {
        item: ItemRef,
        /// Indices into `loaded.pieces`, the best first.
        best: usize,
        extra: Vec<usize>,
        /// A pinned item: the pieces sent, in order, and those left out (D-83).
        whole: Option<(Vec<usize>, Vec<usize>)>,
        loaded: Loaded,
    }
    let mut chosen: Vec<Chosen> = Vec::new();
    for item in &order {
        let Some(loaded) = load(conn, item, words, r.query_vector)? else {
            continue;
        };
        if loaded.pieces.is_empty() {
            continue;
        }
        let ps = &loaded.pieces;
        let has_sim = ps.iter().any(|p| p.sim.is_some());
        let mut ranked: Vec<usize> = (0..ps.len()).collect();
        if has_sim {
            ranked.sort_by(|a, b| {
                ps[*b].sim.unwrap_or(f32::MIN).total_cmp(&ps[*a].sim.unwrap_or(f32::MIN))
            });
        } else if ps.iter().any(|p| p.word_hits > 0) {
            ranked.sort_by(|a, b| ps[*b].word_hits.cmp(&ps[*a].word_hits));
        }
        let best_piece = ranked[0];
        let top = best.max(ps[best_piece].sim.unwrap_or(f32::MIN));
        let extra: Vec<usize> = ranked
            .iter()
            .skip(1)
            .copied()
            .filter(|i| ps[*i].sim.is_some_and(|s| r.rule.passes(s, top)))
            .take(MAX_PIECES_PER_ITEM - 1)
            .collect();
        let whole = pinned
            .contains(item)
            .then(|| whole_item(ps, &ranked, r.budget_tokens / 2));
        chosen.push(Chosen { item: item.clone(), best: best_piece, extra, whole, loaded });
    }

    // ---- the budget: every item's best piece first, then the extra pieces
    let cost = |c: &Chosen, i: usize| estimate_tokens(&c.loaded.pieces[i].text) + HEAD_TOKENS;
    let mut used = 0usize;
    let mut fits: Vec<bool> = vec![false; chosen.len()];
    for (n, c) in chosen.iter().enumerate() {
        let need = match &c.whole {
            Some((sent, _)) => sent.iter().map(|i| cost(c, *i)).sum(),
            None => cost(c, c.best),
        };
        if pinned.contains(&c.item) || used + need <= r.budget_tokens {
            used += need;
            fits[n] = true;
        }
    }
    let mut extras: Vec<Vec<usize>> = vec![Vec::new(); chosen.len()];
    for (n, c) in chosen.iter().enumerate() {
        if !fits[n] || c.whole.is_some() {
            continue;
        }
        for &i in &c.extra {
            let need = cost(c, i);
            if used + need <= r.budget_tokens {
                used += need;
                extras[n].push(i);
            }
        }
    }

    // ---- the result
    let kept: Vec<ItemRef> = chosen
        .iter()
        .zip(&fits)
        .filter(|(_, f)| **f)
        .map(|(c, _)| c.item.clone())
        .collect();
    let infos = items::load(conn, &kept)?;
    let by_id: HashMap<&str, &ItemInfo> = infos.iter().map(|i| (i.id(), i)).collect();
    let mut out = Retrieved { term_ids, ..Retrieved::default() };
    for (n, c) in chosen.iter().enumerate() {
        if !fits[n] {
            continue;
        }
        let Some(info) = by_id.get(c.item.id.as_str()) else { continue };
        out.items.push(c.item.clone());
        let picked: Vec<usize> = match &c.whole {
            Some((sent, _)) => sent.clone(),
            None => std::iter::once(c.best).chain(extras[n].iter().copied()).collect(),
        };
        let note = c.whole.as_ref().and_then(|(_, left)| left_out_note(&c.loaded.pieces, left));
        for (k, &i) in picked.iter().enumerate() {
            let p = &c.loaded.pieces[i];
            let mut text = p.text.clone();
            if let Some(note) = note.as_ref().filter(|_| k + 1 == picked.len()) {
                text = format!("{text}\n\n{note}");
            }
            let mut source = source_ref(info, part_label(&p.part), &p.text);
            source.category_id = Some(match info {
                ItemInfo::Task(t) => category_id_of(conn, &t.id, true)?,
                ItemInfo::Page(pg) => category_id_of(conn, &pg.id, false)?,
            });
            out.sources.push(source);
            out.pieces.push(SourcePiece {
                item: Some((*info).clone()),
                part: p.part.clone(),
                text,
            });
        }
    }

    // ---- N more searched: what passed but did not fit, then the best others
    let sent: HashSet<&ItemRef> = out.items.iter().collect();
    let mut more_items: Vec<ItemRef> = Vec::new();
    for (n, c) in chosen.iter().enumerate() {
        if !fits[n] {
            more_items.push(c.item.clone());
        }
    }
    let infos = items::load(conn, &more_items)?;
    for info in &infos {
        let at = chosen.iter().find(|c| c.item.id == info.id());
        let snippet = at.map_or(String::new(), |c| c.loaded.pieces[c.best].text.clone());
        let mut s = source_ref(info, None, &snippet);
        s.category_id = Some(category_id_of(conn, info.id(), matches!(info, ItemInfo::Task(_)))?);
        out.more.push(s);
    }
    let all_fused = hybrid::fuse(keyword.clone(), meaning.clone(), limit * 2);
    let mut others = 0;
    for h in &all_fused {
        let item = ItemRef { kind: kind_of(h.kind), id: h.id.clone() };
        if sent.contains(&item) || more_items.contains(&item) || pinned.contains(&item) {
            continue;
        }
        if others == MORE_OTHERS {
            break;
        }
        others += 1;
        let snippet = h.snippet.clone();
        out.more.push(hit_ref(h, snippet));
    }
    Ok(out)
}

/// The pieces of a pinned item to send (D-83): all of them, in order, when they fit in `cap`
/// tokens; otherwise the first piece, then the others in `ranked` order (best first; the earlier one
/// on a tie) while they fit. Returns the pieces sent, in order, and those left out.
fn whole_item(pieces: &[Piece], ranked: &[usize], cap: usize) -> (Vec<usize>, Vec<usize>) {
    let cost = |i: usize| estimate_tokens(&pieces[i].text) + HEAD_TOKENS;
    if (0..pieces.len()).map(cost).sum::<usize>() <= cap {
        return ((0..pieces.len()).collect(), Vec::new());
    }
    let mut sent = vec![0];
    let mut used = cost(0);
    for &i in ranked.iter().filter(|i| **i != 0) {
        if used + cost(i) <= cap {
            used += cost(i);
            sent.push(i);
        }
    }
    sent.sort_unstable();
    let left = (0..pieces.len()).filter(|i| !sent.contains(i)).collect();
    (sent, left)
}

/// `pieces` without the note on what was left out (D-83), for `ask-page`: the change step already
/// proposed nothing when the content the request points at was left out, and a page writer has no
/// reply to say it in, so the note would only end up in the page.
pub fn without_left_out_notes(pieces: &[SourcePiece]) -> Vec<SourcePiece> {
    let start = format!("\n\n{}", LEFT_OUT.split("{count}").next().unwrap_or(LEFT_OUT));
    pieces
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if let Some(at) = p.text.rfind(&start) {
                p.text.truncate(at);
            }
            p
        })
        .collect()
}

/// The note on what was left out of a pinned item, or `None` when nothing was.
fn left_out_note(pieces: &[Piece], left: &[usize]) -> Option<String> {
    if left.is_empty() {
        return None;
    }
    // As the sources' second line names a piece (`formats`); pieces with the same name count once.
    let mut labels: Vec<String> = Vec::new();
    for &i in left {
        let label = match &pieces[i].part {
            PiecePart::Description => "Description".to_string(),
            PiecePart::Comment(day) => format!("Comment {day}"),
            PiecePart::Section(path) if path.is_empty() => "Section: (top)".to_string(),
            PiecePart::Section(path) => format!("Section: {}", path.join(" > ")),
            PiecePart::File { .. } => continue,
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    if labels.is_empty() {
        return None;
    }
    Some(
        LEFT_OUT
            .replace("{count}", &labels.len().to_string())
            .replace("{parts}", &labels.join(", ")),
    )
}

fn category_id_of(conn: &Connection, id: &str, task: bool) -> Result<String> {
    let sql = if task {
        "SELECT category_id FROM tasks WHERE id = ?1"
    } else {
        "SELECT category_id FROM pages WHERE id = ?1"
    };
    Ok(conn.query_row(sql, [id], |r| r.get(0)).unwrap_or_default())
}

#[cfg(test)]
mod tests;
