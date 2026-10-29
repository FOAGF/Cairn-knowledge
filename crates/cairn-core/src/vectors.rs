//! The vector index (FR-RAG-ST-2, FR-RAG-IX-1..6): pieces of pages and tasks in the `chunks` table
//! with float32 BLOB vectors, compared in Rust (no SQLite extension in v1).
//!
//! Flow:
//! - `hooks` call [`index_item`] / [`remove_item`] inside every save, so the pieces always match
//!   the data. A piece whose `hash` is unchanged keeps its vector; a new or changed piece gets
//!   `vector = NULL`, which means "waiting".
//! - The indexer (src-tauri) takes waiting pieces with [`next_jobs`], has the sidecar embed them and
//!   saves them with [`store_vectors`] (pieces that changed meanwhile are skipped by their hash).
//! - Searches embed the query in the sidecar and call [`search`]; *Find similar pages* calls
//!   [`similar_items`].
//!
//! Vectors are little-endian float32, normalised, [`IndexModel::dims`] long; the same layout the
//! sidecar sends as base64 (`sidecar/PROTOCOL.md`).
//!
//! Owned by the M4 search-core agent.

use std::collections::HashMap;

use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, params, params_from_iter};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::Result;
use crate::chunking::{self, Piece};
use crate::glossary::{self, Glossary, TermText};
use crate::search::{DEFAULT_LIMIT, SearchKind, SearchQuery};

/// The model that makes the vectors. Stored in `vector_meta`; a different one empties every vector
/// (FR-RAG-EM-2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexModel {
    /// e.g. "embeddinggemma-300m-q4"
    pub id: String,
    /// e.g. 256
    pub dims: u32,
}

/// One piece for the sidecar to embed (document prompt `title: {title} | text: {text}`).
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedJob {
    pub chunk_id: i64,
    /// Hash of the piece when the job was taken; [`store_vectors`] skips the vector if it changed.
    pub hash: String,
    /// Page title, or "KEY title" for tasks.
    pub title: String,
    /// Heading path (if any), the piece text and, with the glossary, its terms.
    pub text: String,
}

/// How far the vector index is (Settings › Search, FR-RAG-IX-5/6).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    /// Pages and tasks.
    pub items: u32,
    /// Items whose every piece has a vector.
    pub items_ready: u32,
    pub chunks: u32,
    pub chunks_ready: u32,
    /// Model and dims recorded in `vector_meta`, if any.
    pub model: Option<String>,
    pub dims: Option<u32>,
}

/// The best-matching piece of one page or task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MeaningHit {
    pub kind: SearchKind,
    pub id: String,
    pub title: String,
    /// Task key for tasks.
    pub key: Option<String>,
    pub category_id: String,
    pub chunk_id: i64,
    /// Heading path of the piece.
    pub heading: String,
    /// The piece text (the caller makes a snippet from it).
    pub text: String,
    /// Cosine similarity, -1..1 (dot product of normalised vectors).
    pub score: f32,
}

/// The text that goes into the vector, one function so nothing drifts: heading path (if any),
/// the piece text and the glossary terms of the piece.
///
/// FR-RAG-IX-3: the glossary passes the lines of the entries found in the piece here; the hash
/// below covers them, so a changed entry re-embeds exactly the pieces that use it.
fn embed_text(heading: &str, text: &str, terms: &[&str]) -> String {
    let mut out = String::with_capacity(heading.len() + text.len() + 2);
    if !heading.is_empty() {
        out.push_str(heading);
        out.push('\n');
    }
    out.push_str(text);
    if !terms.is_empty() {
        out.push('\n');
        out.push_str(&terms.join(", "));
    }
    out
}

fn as_strs(lines: &[String]) -> Vec<&str> {
    lines.iter().map(String::as_str).collect()
}

/// The glossary lines of the entries a piece uses (none for a glossary entry's own piece).
fn piece_terms(
    glossary: &mut Glossary,
    kind: SearchKind,
    category: &str,
    text: &str,
) -> Vec<String> {
    if kind == SearchKind::Term {
        return Vec::new();
    }
    glossary
        .terms_in(category, text)
        .iter()
        .map(TermText::line)
        .collect()
}

/// Hash of everything that goes into one vector (title and [`embed_text`]).
fn piece_hash(title: &str, heading: &str, text: &str, terms: &[&str]) -> String {
    let mut h = Sha256::new();
    h.update(title.as_bytes());
    h.update([0]);
    h.update(embed_text(heading, text, terms).as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn kind_of(s: &str) -> SearchKind {
    match s {
        "page" => SearchKind::Page,
        "term" => SearchKind::Term,
        _ => SearchKind::Task,
    }
}

fn not_found<T>(r: rusqlite::Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    not_found(
        conn.query_row("SELECT value FROM vector_meta WHERE key = ?1", [key], |r| {
            r.get::<_, String>(0)
        }),
    )
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO vector_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Records `model`; if a different model (or dims) was recorded, every vector is emptied so the
/// indexer re-embeds everything. Returns true if vectors were emptied (FR-RAG-EM-2).
pub fn ensure_model(conn: &Connection, model: &IndexModel) -> Result<bool> {
    let dims = model.dims.to_string();
    let old = (meta(conn, "model")?, meta(conn, "dims")?);
    if old.0.as_deref() == Some(model.id.as_str()) && old.1.as_deref() == Some(dims.as_str()) {
        return Ok(false);
    }
    set_meta(conn, "model", &model.id)?;
    set_meta(conn, "dims", &dims)?;
    if old.0.is_some() {
        clear_vectors(conn)?;
        return Ok(true);
    }
    Ok(false)
}

/// The title that goes into the vector, the item's category ("" for a glossary entry) and the
/// pieces of one page, task or glossary entry; `None` if it doesn't exist. A page with an empty
/// body still has one empty piece, so its title is embedded.
fn load_pieces(
    conn: &Connection,
    kind: SearchKind,
    id: &str,
) -> Result<Option<(String, String, Vec<Piece>)>> {
    match kind {
        SearchKind::Page => {
            let row = not_found(conn.query_row(
                "SELECT title, body, category_id FROM pages WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            ))?;
            Ok(row.map(|(title, body, category)| {
                let mut pieces = chunking::split_page(&body);
                if pieces.is_empty() {
                    pieces.push(Piece {
                        heading: String::new(),
                        text: String::new(),
                    });
                }
                (title, category, pieces)
            }))
        }
        // A glossary entry is one piece: its other names and description (FR-RAG-IX-4).
        SearchKind::Term => Ok(glossary::entry_parts(conn, id)?.map(
            |(term, names, description)| {
                let text = [names, description]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let piece = Piece {
                    heading: String::new(),
                    text,
                };
                (term, String::new(), vec![piece])
            },
        )),
        SearchKind::Task => {
            let row = not_found(conn.query_row(
                "SELECT key, title, status, description, category_id FROM tasks WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            ))?;
            let Some((key, title, status, desc, category)) = row else {
                return Ok(None);
            };
            let comments = conn
                .prepare_cached(
                    "SELECT text FROM task_comments WHERE task_id = ?1 ORDER BY created_at, id",
                )?
                .query_map([id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let pieces = chunking::split_task(&key, &title, &status, &desc, &comments);
            Ok(Some((format!("{key} {title}"), category, pieces)))
        }
    }
}

/// Re-cuts one page or task into pieces (chunking.rs), keeping vectors whose hash is unchanged.
/// Called from `hooks` inside the save transaction; cheap (no embedding happens here).
/// A vector is also kept when its piece only moved (the same hash at another position).
pub(crate) fn index_item(conn: &Connection, kind: SearchKind, id: &str) -> Result<()> {
    index_item_with(conn, &mut Glossary::load(conn)?, kind, id)
}

/// [`index_item`] with the glossary already loaded (for batches).
pub(crate) fn index_item_with(
    conn: &Connection,
    glossary: &mut Glossary,
    kind: SearchKind,
    id: &str,
) -> Result<()> {
    let Some((title, category, pieces)) = load_pieces(conn, kind, id)? else {
        return remove_item(conn, kind, id);
    };
    // FR-RAG-IX-3: the glossary lines of the entries each piece uses go into its text, so a
    // changed entry changes the hash of exactly the pieces that use it.
    let lines: Vec<Vec<String>> = pieces
        .iter()
        .map(|p| piece_terms(glossary, kind, &category, &p.text))
        .collect();
    let new: Vec<String> = pieces
        .iter()
        .zip(&lines)
        .map(|(p, l)| piece_hash(&title, &p.heading, &p.text, &as_strs(l)))
        .collect();

    let existing: Vec<(i64, i64, String, bool)> = conn
        .prepare_cached(
            "SELECT seq, id, hash, vector IS NOT NULL FROM chunks
             WHERE kind = ?1 AND ref_id = ?2",
        )?
        .query_map(params![kind.as_str(), id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let at: HashMap<i64, (i64, &str)> = existing
        .iter()
        .map(|(seq, cid, h, _)| (*seq, (*cid, h.as_str())))
        .collect();

    // Vectors of changed positions that an old piece with the same hash can supply.
    let mut reuse: HashMap<&str, Vec<u8>> = HashMap::new();
    for (seq, h) in new.iter().enumerate() {
        let same = at.get(&(seq as i64)).is_some_and(|(_, old)| old == h);
        if same || reuse.contains_key(h.as_str()) {
            continue;
        }
        if let Some((_, cid, _, _)) = existing.iter().find(|(_, _, oh, ready)| oh == h && *ready) {
            let blob: Vec<u8> =
                conn.query_row("SELECT vector FROM chunks WHERE id = ?1", [cid], |r| {
                    r.get(0)
                })?;
            reuse.insert(h.as_str(), blob);
        }
    }

    let mut update = conn.prepare_cached(
        "UPDATE chunks SET heading = ?1, text = ?2, hash = ?3, vector = ?4 WHERE id = ?5",
    )?;
    let mut insert = conn.prepare_cached(
        "INSERT INTO chunks (kind, ref_id, seq, heading, text, hash, vector)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for (seq, (piece, h)) in pieces.iter().zip(&new).enumerate() {
        let seq = seq as i64;
        let vector = reuse.get(h.as_str());
        match at.get(&seq) {
            Some((_, old)) if old == h => {}
            Some((cid, _)) => {
                update.execute(params![piece.heading, piece.text, h, vector, cid])?;
            }
            None => {
                insert.execute(params![
                    kind.as_str(),
                    id,
                    seq,
                    piece.heading,
                    piece.text,
                    h,
                    vector
                ])?;
            }
        }
    }
    conn.prepare_cached("DELETE FROM chunks WHERE kind = ?1 AND ref_id = ?2 AND seq >= ?3")?
        .execute(params![kind.as_str(), id, pieces.len() as i64])?;
    Ok(())
}

/// Removes an item's pieces. Called from `hooks` when a page or task is deleted.
pub(crate) fn remove_item(conn: &Connection, kind: SearchKind, id: &str) -> Result<()> {
    conn.prepare_cached("DELETE FROM chunks WHERE kind = ?1 AND ref_id = ?2")?
        .execute(params![kind.as_str(), id])?;
    Ok(())
}

/// Re-cuts every page and task (after import, a schema upgrade, *Rebuild index*), keeping the
/// vectors of unchanged pieces. Called from `hooks::rebuild_all`.
pub fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM chunks WHERE kind = 'page' AND ref_id NOT IN (SELECT id FROM pages)",
        [],
    )?;
    conn.execute(
        "DELETE FROM chunks WHERE kind = 'task' AND ref_id NOT IN (SELECT id FROM tasks)",
        [],
    )?;
    conn.execute(
        "DELETE FROM chunks WHERE kind = 'term' AND ref_id NOT IN (SELECT id FROM glossary_entries)",
        [],
    )?;
    let mut glossary = Glossary::load(conn)?;
    for (kind, table) in [
        (SearchKind::Page, "pages"),
        (SearchKind::Task, "tasks"),
        (SearchKind::Term, "glossary_entries"),
    ] {
        let ids = conn
            .prepare(&format!("SELECT id FROM {table}"))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for id in ids {
            index_item_with(conn, &mut glossary, kind, &id)?;
        }
    }
    Ok(())
}

/// Empties every vector (the next indexing pass re-embeds everything).
pub fn clear_vectors(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE chunks SET vector = NULL WHERE vector IS NOT NULL",
        [],
    )?;
    Ok(())
}

/// Up to `limit` waiting pieces: pages, then tasks, then glossary entries; newest items (by last
/// change) first, then in reading order within an item.
pub fn next_jobs(conn: &Connection, limit: u32) -> Result<Vec<EmbedJob>> {
    struct Row {
        chunk_id: i64,
        hash: String,
        kind: SearchKind,
        heading: String,
        text: String,
        title: String,
        category: String,
    }
    let rows = conn
        .prepare_cached(
            "SELECT c.id, c.hash, c.kind, c.heading, c.text,
                    COALESCE(p.title, t.key || ' ' || t.title, g.term),
                    COALESCE(p.category_id, t.category_id, '')
             FROM chunks c
             LEFT JOIN pages p ON c.kind = 'page' AND p.id = c.ref_id
             LEFT JOIN tasks t ON c.kind = 'task' AND t.id = c.ref_id
             LEFT JOIN glossary_entries g ON c.kind = 'term' AND g.id = c.ref_id
             WHERE c.vector IS NULL
               AND (p.id IS NOT NULL OR t.id IS NOT NULL OR g.id IS NOT NULL)
             ORDER BY CASE c.kind WHEN 'page' THEN 0 WHEN 'task' THEN 1 ELSE 2 END,
                      COALESCE(p.updated_at, t.updated_at, g.updated_at) DESC, c.ref_id, c.seq
             LIMIT ?1",
        )?
        .query_map([limit], |r| {
            Ok(Row {
                chunk_id: r.get(0)?,
                hash: r.get(1)?,
                kind: kind_of(&r.get::<_, String>(2)?),
                heading: r.get(3)?,
                text: r.get(4)?,
                title: r.get(5)?,
                category: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // FR-RAG-IX-3: each piece's glossary lines join its text, found the same way as in `index_item`.
    let mut glossary = Glossary::load(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let lines = piece_terms(&mut glossary, r.kind, &r.category, &r.text);
            EmbedJob {
                chunk_id: r.chunk_id,
                hash: r.hash,
                title: r.title,
                text: embed_text(&r.heading, &r.text, &as_strs(&lines)),
            }
        })
        .collect())
}

/// Saves vectors for finished jobs. A job whose piece changed or vanished since [`next_jobs`]
/// (different hash) is skipped, and so is a vector whose length differs from the recorded dims.
/// Returns how many were saved.
pub fn store_vectors(conn: &Connection, done: &[(EmbedJob, Vec<f32>)]) -> Result<u32> {
    let dims: Option<usize> = meta(conn, "dims")?.and_then(|d| d.parse().ok());
    let mut stmt =
        conn.prepare_cached("UPDATE chunks SET vector = ?1 WHERE id = ?2 AND hash = ?3")?;
    let mut saved = 0;
    for (job, v) in done {
        if dims.is_some_and(|d| d != v.len()) {
            continue;
        }
        saved += stmt.execute(params![to_blob(v), job.chunk_id, job.hash])? as u32;
    }
    Ok(saved)
}

pub fn status(conn: &Connection) -> Result<IndexStatus> {
    let count = |sql: &str| -> Result<u32> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    Ok(IndexStatus {
        items: count("SELECT (SELECT COUNT(*) FROM pages) + (SELECT COUNT(*) FROM tasks)")?,
        items_ready: count(
            "SELECT COUNT(*) FROM (
                 SELECT 1 FROM chunks WHERE kind IN ('page', 'task')
                 GROUP BY kind, ref_id HAVING COUNT(vector) = COUNT(*))",
        )?,
        chunks: count("SELECT COUNT(*) FROM chunks WHERE kind IN ('page', 'task')")?,
        chunks_ready: count(
            "SELECT COUNT(*) FROM chunks WHERE kind IN ('page', 'task') AND vector IS NOT NULL",
        )?,
        model: meta(conn, "model")?,
        dims: meta(conn, "dims")?.and_then(|d| d.parse().ok()),
    })
}

fn text_at<'a>(r: &'a rusqlite::Row, i: usize) -> rusqlite::Result<&'a str> {
    match r.get_ref(i)? {
        ValueRef::Text(b) => std::str::from_utf8(b).map_err(|e| rusqlite::Error::Utf8Error(i, e)),
        _ => Err(rusqlite::Error::InvalidColumnType(
            i,
            String::new(),
            rusqlite::types::Type::Text,
        )),
    }
}

/// Dot product of a vector with little-endian float32 bytes (4 accumulators so the compiler can
/// vectorise). The caller checks `b.len() == 4 * a.len()`.
fn dot(a: &[f32], b: &[u8]) -> f32 {
    let mut acc = [0f32; 4];
    let mut ca = a.chunks_exact(4);
    let mut cb = b.chunks_exact(16);
    for (x, y) in (&mut ca).zip(&mut cb) {
        for k in 0..4 {
            acc[k] +=
                x[k] * f32::from_le_bytes([y[4 * k], y[4 * k + 1], y[4 * k + 2], y[4 * k + 3]]);
        }
    }
    let mut sum = acc[0] + acc[1] + acc[2] + acc[3];
    for (x, y) in ca.remainder().iter().zip(cb.remainder().chunks_exact(4)) {
        sum += x * f32::from_le_bytes([y[0], y[1], y[2], y[3]]);
    }
    sum
}

/// Pieces compared with each of `queries` (score = best dot product); best piece per item; the
/// filters of keyword search. `exclude` leaves one item out.
fn scan(
    conn: &Connection,
    queries: &[&[f32]],
    filter: &SearchQuery,
    exclude: Option<(SearchKind, &str)>,
) -> Result<Vec<MeaningHit>> {
    let queries: Vec<&[f32]> = queries.iter().copied().filter(|q| !q.is_empty()).collect();
    if queries.is_empty() {
        return Ok(Vec::new());
    }
    // The candidate items are chosen first (small tables, filters applied there); their pieces
    // are then found through the (kind, ref_id) index, so a category filter reads only that
    // category's vectors.
    let kinds = filter
        .kinds
        .clone()
        .unwrap_or_else(|| vec![SearchKind::Page, SearchKind::Task, SearchKind::Term]);
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let mut category_sql = String::new();
    let mut category_args: Vec<Value> = Vec::new();
    if let Some(ids) = &filter.category_ids {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        category_sql = format!(" AND category_id IN ({marks})");
        category_args = ids.iter().cloned().map(Value::Text).collect();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut args: Vec<Value> = Vec::new();
    if kinds.contains(&SearchKind::Page) {
        let archived = if filter.include_archived.unwrap_or(false) {
            ""
        } else {
            " AND archived = 0"
        };
        parts.push(format!(
            "SELECT 'page' AS kind, id FROM pages WHERE 1 = 1{archived}{category_sql}"
        ));
        args.extend(category_args.iter().cloned());
    }
    if kinds.contains(&SearchKind::Task) {
        let closed = if filter.include_closed == Some(false) {
            " AND status NOT IN ('done', 'wont_do')"
        } else {
            ""
        };
        parts.push(format!(
            "SELECT 'task' AS kind, id FROM tasks WHERE 1 = 1{closed}{category_sql}"
        ));
        args.extend(category_args.iter().cloned());
    }
    if kinds.contains(&SearchKind::Term) {
        // A global entry passes any category filter, a category entry only its own (FR-KN-GL-5).
        let scope = match &filter.category_ids {
            Some(ids) => format!(
                " AND (category_id IS NULL OR category_id IN ({}))",
                vec!["?"; ids.len()].join(",")
            ),
            None => String::new(),
        };
        parts.push(format!(
            "SELECT 'term' AS kind, id FROM glossary_entries WHERE 1 = 1{scope}"
        ));
        args.extend(category_args.iter().cloned());
    }
    let mut sql = format!(
        "SELECT c.vector, c.id, c.kind, c.ref_id
         FROM ({}) i
         CROSS JOIN chunks c ON c.kind = i.kind AND c.ref_id = i.id
         WHERE c.vector IS NOT NULL",
        parts.join(" UNION ALL ")
    );
    if let Some((kind, id)) = exclude {
        sql.push_str(" AND NOT (c.kind = ? AND c.ref_id = ?)");
        args.push(Value::Text(kind.as_str().to_string()));
        args.push(Value::Text(id.to_string()));
    }

    // Best piece per item, kept in one map per kind: ref_id -> (score, chunk id).
    let mut best: [HashMap<String, (f32, i64)>; 3] =
        [HashMap::new(), HashMap::new(), HashMap::new()];
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params_from_iter(args))?;
    while let Some(r) = rows.next()? {
        let ValueRef::Blob(blob) = r.get_ref(0)? else {
            continue;
        };
        let mut score = f32::NEG_INFINITY;
        for q in &queries {
            // A vector of another length comes from another model: ignore it.
            if blob.len() == 4 * q.len() {
                score = score.max(dot(q, blob));
            }
        }
        if score == f32::NEG_INFINITY || score.is_nan() {
            continue;
        }
        let cid: i64 = r.get(1)?;
        let map = &mut best[match text_at(r, 2)? {
            "page" => 0,
            "task" => 1,
            _ => 2,
        }];
        let ref_id = text_at(r, 3)?;
        match map.get_mut(ref_id) {
            Some(e) => {
                if score > e.0 {
                    *e = (score, cid);
                }
            }
            None => {
                map.insert(ref_id.to_string(), (score, cid));
            }
        }
    }
    drop(rows);

    let mut top: Vec<(f32, i64, &str, &str)> = Vec::new();
    for (map, kind) in best.iter().zip(["page", "task", "term"]) {
        top.extend(map.iter().map(|(rid, (s, c))| (*s, *c, kind, rid.as_str())));
    }
    // Best first; ties in a fixed order so results don't flicker.
    top.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(b.2)).then(a.3.cmp(b.3)));
    top.truncate(filter.limit.unwrap_or(DEFAULT_LIMIT) as usize);

    let mut detail = conn.prepare_cached(
        "SELECT c.heading, c.text, COALESCE(p.title, t.title, g.term), t.key,
                COALESCE(p.category_id, t.category_id, g.category_id, '')
         FROM chunks c
         LEFT JOIN pages p ON c.kind = 'page' AND p.id = c.ref_id
         LEFT JOIN tasks t ON c.kind = 'task' AND t.id = c.ref_id
         LEFT JOIN glossary_entries g ON c.kind = 'term' AND g.id = c.ref_id
         WHERE c.id = ?1",
    )?;
    let mut out = Vec::with_capacity(top.len());
    for (score, cid, kind, rid) in top {
        out.push(detail.query_row([cid], |r| {
            Ok(MeaningHit {
                kind: kind_of(kind),
                id: rid.to_string(),
                title: r.get(2)?,
                key: r.get(3)?,
                category_id: r.get(4)?,
                chunk_id: cid,
                heading: r.get(0)?,
                text: r.get(1)?,
                score,
            })
        })?);
    }
    Ok(out)
}

/// Pages and tasks whose pieces are closest to `query` (a normalised query vector), best piece per
/// item, best first, with the same filters as keyword search (`kinds`, `category_ids`,
/// `include_archived`, `include_closed`, `limit`; `text` is ignored). Candidates are narrowed in
/// SQL first, then compared in Rust (FR-RAG-ST-2). Pieces without a vector, and vectors of
/// another length than `query`, are ignored (FR-RAG-SR-1).
pub fn search(conn: &Connection, query: &[f32], filter: &SearchQuery) -> Result<Vec<MeaningHit>> {
    scan(conn, &[query], filter, None)
}

/// Other items most similar to one item (*Find similar pages*, the duplicate check): the score of
/// a candidate is the best similarity between any of its pieces and any piece of the item.
pub fn similar_items(
    conn: &Connection,
    kind: SearchKind,
    id: &str,
    filter: &SearchQuery,
) -> Result<Vec<MeaningHit>> {
    let blobs = conn
        .prepare_cached(
            "SELECT vector FROM chunks WHERE kind = ?1 AND ref_id = ?2 AND vector IS NOT NULL",
        )?
        .query_map(params![kind.as_str(), id], |r| r.get::<_, Vec<u8>>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let vectors: Vec<Vec<f32>> = blobs.iter().map(|b| from_blob(b)).collect();
    let refs: Vec<&[f32]> = vectors.iter().map(Vec::as_slice).collect();
    scan(conn, &refs, filter, Some((kind, id)))
}

/// Like [`similar_items`] for text that isn't saved yet (a draft): the caller embeds the draft's
/// pieces and passes their vectors (M5's duplicate check).
pub fn similar_to_vectors(
    conn: &Connection,
    vectors: &[Vec<f32>],
    filter: &SearchQuery,
) -> Result<Vec<MeaningHit>> {
    let refs: Vec<&[f32]> = vectors.iter().map(Vec::as_slice).collect();
    scan(conn, &refs, filter, None)
}

/// Little-endian float32 bytes.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The inverse of [`to_blob`]. Trailing bytes that don't make a whole float are ignored.
pub fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;

    const DIMS: usize = 8;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_at(dir.path()).unwrap();
        (dir, db)
    }

    /// A normalised vector pointing along axis `i`, plus `mix` along axis `j`.
    fn axis(i: usize, j: usize, mix: f32) -> Vec<f32> {
        let mut v = vec![0.0; DIMS];
        v[i] = 1.0;
        v[j] += mix;
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter().map(|x| x / n).collect()
    }

    fn add_category(c: &Connection, id: &str, key: &str) {
        c.execute(
            "INSERT INTO categories (id, name, key, created_at, updated_at) VALUES (?1, ?1, ?2, 'x', 'x')",
            [id, key],
        )
        .unwrap();
    }

    fn add_page(c: &Connection, id: &str, category: &str, title: &str, body: &str) {
        c.execute(
            "INSERT INTO pages (id, category_id, title, body, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'x', ?5)",
            params![id, category, title, body, format!("2026-01-{id:0>2}")],
        )
        .unwrap();
        index_item(c, SearchKind::Page, id).unwrap();
    }

    fn add_task(c: &Connection, id: &str, n: i64, category: &str, title: &str, status: &str) {
        c.execute(
            "INSERT INTO tasks (id, key, number, category_id, title, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'x', ?7)",
            params![
                id,
                format!("GEN-{n}"),
                n,
                category,
                title,
                status,
                format!("2026-02-{n:0>2}")
            ],
        )
        .unwrap();
        index_item(c, SearchKind::Task, id).unwrap();
    }

    /// Embeds every waiting piece with `f(job)`.
    fn fill(c: &Connection, f: impl Fn(&EmbedJob) -> Vec<f32>) -> u32 {
        let done: Vec<_> = next_jobs(c, 1000)
            .unwrap()
            .into_iter()
            .map(|j| {
                let v = f(&j);
                (j, v)
            })
            .collect();
        store_vectors(c, &done).unwrap()
    }

    fn waiting(c: &Connection) -> u32 {
        c.query_row(
            "SELECT COUNT(*) FROM chunks WHERE vector IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn every_item_has_a_piece_even_empty() {
        let (_d, db) = db();
        let c = db.conn();
        add_page(c, "1", "p-general", "Empty page", "");
        add_task(c, "t1", 1, "p-general", "A task", "todo");
        let st = status(c).unwrap();
        assert_eq!(
            (st.items, st.chunks, st.chunks_ready, st.items_ready),
            (2, 2, 0, 0)
        );
        let jobs = next_jobs(c, 10).unwrap();
        assert_eq!(jobs.len(), 2);
        // Pages before tasks; job titles.
        assert_eq!(jobs[0].title, "Empty page");
        assert_eq!(jobs[0].text, "");
        assert_eq!(jobs[1].title, "GEN-1 A task");
        assert_eq!(jobs[1].text, "GEN-1: A task (todo)");
    }

    #[test]
    fn job_text_has_heading_and_order_is_newest_first() {
        let (_d, db) = db();
        let c = db.conn();
        add_page(c, "01", "p-general", "Old", "# Head\n\nbody");
        add_page(c, "09", "p-general", "New", "text");
        add_task(c, "t1", 5, "p-general", "T", "todo");
        let jobs = next_jobs(c, 10).unwrap();
        let titles: Vec<&str> = jobs.iter().map(|j| j.title.as_str()).collect();
        assert_eq!(titles, ["New", "Old", "GEN-5 T"]);
        assert_eq!(jobs[1].text, "Head\nbody");
        assert_eq!(next_jobs(c, 2).unwrap().len(), 2);
    }

    #[test]
    fn unchanged_pieces_keep_vectors_after_an_edit_elsewhere() {
        let (_d, db) = db();
        let c = db.conn();
        let long = "alpha beta gamma delta. ".repeat(60);
        let body = format!("# One\n\n{long}\n\n# Two\n\n{long}\n\n# Three\n\n{long}");
        add_page(c, "1", "p-general", "Page", &body);
        let n = status(c).unwrap().chunks;
        assert!(n >= 3);
        assert_eq!(fill(c, |_| axis(0, 1, 0.1)), n);
        assert_eq!(waiting(c), 0);

        // Edit only the last section.
        let edited = body.replace(&format!("# Three\n\n{long}"), "# Three\n\nchanged text. ");
        c.execute("UPDATE pages SET body = ?1 WHERE id = '1'", [edited])
            .unwrap();
        index_item(c, SearchKind::Page, "1").unwrap();
        let st = status(c).unwrap();
        assert_eq!(waiting(c), 1);
        assert_eq!(st.chunks_ready, st.chunks - 1);
        assert_eq!(st.items_ready, 0);
    }

    #[test]
    fn moved_piece_keeps_its_vector_and_title_change_resets_all() {
        let (_d, db) = db();
        let c = db.conn();
        let long = "alpha beta gamma delta. ".repeat(60);
        let body = format!("# One\n\n{long}\n\n# Two\n\n{long} two");
        add_page(c, "1", "p-general", "Page", &body);
        fill(c, |_| axis(0, 1, 0.0));
        // A new first section moves the others down by one position.
        let moved = format!("# Zero\n\n{long} zero\n\n{body}");
        c.execute("UPDATE pages SET body = ?1 WHERE id = '1'", [moved])
            .unwrap();
        index_item(c, SearchKind::Page, "1").unwrap();
        assert_eq!(waiting(c), 1);
        fill(c, |_| axis(0, 1, 0.0));
        c.execute("UPDATE pages SET title = 'Renamed' WHERE id = '1'", [])
            .unwrap();
        index_item(c, SearchKind::Page, "1").unwrap();
        assert_eq!(waiting(c), status(c).unwrap().chunks);
    }

    #[test]
    fn stale_store_vectors_are_skipped() {
        let (_d, db) = db();
        let c = db.conn();
        add_page(c, "1", "p-general", "Page", "first text");
        let jobs = next_jobs(c, 10).unwrap();
        // The page changes while the sidecar works.
        c.execute("UPDATE pages SET body = 'second text' WHERE id = '1'", [])
            .unwrap();
        index_item(c, SearchKind::Page, "1").unwrap();
        let done: Vec<_> = jobs.into_iter().map(|j| (j, axis(0, 1, 0.0))).collect();
        assert_eq!(store_vectors(c, &done).unwrap(), 0);
        assert_eq!(waiting(c), 1);
        // A deleted piece is skipped too.
        let jobs = next_jobs(c, 10).unwrap();
        remove_item(c, SearchKind::Page, "1").unwrap();
        let done: Vec<_> = jobs.into_iter().map(|j| (j, axis(0, 1, 0.0))).collect();
        assert_eq!(store_vectors(c, &done).unwrap(), 0);
    }

    #[test]
    fn delete_and_comments_update_pieces() {
        let (_d, db) = db();
        let c = db.conn();
        add_task(c, "t1", 1, "p-general", "Task", "todo");
        add_page(c, "1", "p-general", "Page", "x");
        assert_eq!(status(c).unwrap().chunks, 2);
        c.execute(
            "INSERT INTO task_comments (id, task_id, text, created_at) VALUES ('c1', 't1', 'a comment', 'x')",
            [],
        )
        .unwrap();
        index_item(c, SearchKind::Task, "t1").unwrap();
        assert_eq!(status(c).unwrap().chunks, 3);
        remove_item(c, SearchKind::Task, "t1").unwrap();
        remove_item(c, SearchKind::Page, "1").unwrap();
        assert_eq!(status(c).unwrap().chunks, 0);
        // A deleted row also empties the pieces when re-indexed.
        add_page(c, "2", "p-general", "Gone", "x");
        c.execute("DELETE FROM pages WHERE id = '2'", []).unwrap();
        index_item(c, SearchKind::Page, "2").unwrap();
        assert_eq!(status(c).unwrap().chunks, 0);
    }

    #[test]
    fn rebuild_drops_orphans_and_keeps_vectors() {
        let (_d, db) = db();
        let c = db.conn();
        add_page(c, "1", "p-general", "Page", "x");
        fill(c, |_| axis(0, 1, 0.0));
        c.execute(
            "INSERT INTO chunks (kind, ref_id, seq, text, hash) VALUES ('page', 'ghost', 0, 't', 'h')",
            [],
        )
        .unwrap();
        rebuild(c).unwrap();
        let st = status(c).unwrap();
        assert_eq!((st.chunks, st.chunks_ready, st.items_ready), (1, 1, 1));
    }

    #[test]
    fn ensure_model_clears_on_change() {
        let (_d, db) = db();
        let c = db.conn();
        add_page(c, "1", "p-general", "Page", "x");
        let m = IndexModel {
            id: "m1".into(),
            dims: DIMS as u32,
        };
        assert!(!ensure_model(c, &m).unwrap());
        fill(c, |_| axis(0, 1, 0.0));
        assert_eq!(waiting(c), 0);
        assert!(!ensure_model(c, &m).unwrap());
        assert_eq!(waiting(c), 0);
        let st = status(c).unwrap();
        assert_eq!(
            (st.model.as_deref(), st.dims),
            (Some("m1"), Some(DIMS as u32))
        );
        // Same name with other dims empties the vectors.
        let m4 = IndexModel {
            id: "m1".into(),
            dims: 4,
        };
        assert!(ensure_model(c, &m4).unwrap());
        assert_eq!(waiting(c), 1);
        // A vector of the wrong length is refused now.
        let jobs = next_jobs(c, 10).unwrap();
        assert_eq!(
            store_vectors(c, &[(jobs[0].clone(), vec![0.0; DIMS])]).unwrap(),
            0
        );
        assert_eq!(
            store_vectors(c, &[(jobs[0].clone(), vec![0.5; 4])]).unwrap(),
            1
        );
        // Another name too.
        let other = IndexModel {
            id: "m2".into(),
            dims: 4,
        };
        assert!(ensure_model(c, &other).unwrap());
        assert_eq!(waiting(c), 1);
    }

    fn corpus(c: &Connection) {
        add_category(c, "p2", "TWO");
        add_page(c, "1", "p-general", "Hoses", "hoses");
        add_page(c, "2", "p2", "Pumps", "pumps");
        add_page(c, "3", "p-general", "Archived", "old");
        c.execute("UPDATE pages SET archived = 1 WHERE id = '3'", [])
            .unwrap();
        add_task(c, "t1", 1, "p-general", "Open task", "todo");
        add_task(c, "t2", 2, "p2", "Done task", "done");
        fill(c, |j| match j.title.as_str() {
            "Hoses" => axis(0, 1, 0.0),
            "Pumps" => axis(0, 1, 0.5),
            "Archived" => axis(0, 2, 0.1),
            "GEN-1 Open task" => axis(1, 0, 0.3),
            _ => axis(0, 3, 1.0),
        });
    }

    fn ids(h: &[MeaningHit]) -> Vec<String> {
        h.iter().map(|x| x.id.clone()).collect()
    }

    #[test]
    fn search_filters() {
        let (_d, db) = db();
        let c = db.conn();
        corpus(c);
        let query = axis(0, 1, 0.0);
        let f = |f: SearchQuery| search(c, &query, &f).unwrap();
        let all = f(SearchQuery::default());
        // Archived page hidden by default; everything else ranked by similarity.
        assert_eq!(ids(&all), ["1", "2", "t2", "t1"]);
        assert!((all[0].score - 1.0).abs() < 1e-5);
        assert_eq!(all[2].key.as_deref(), Some("GEN-2"));
        assert_eq!(all[0].heading, "");

        let a = f(SearchQuery {
            include_archived: Some(true),
            ..Default::default()
        });
        assert_eq!(a.len(), 5);
        let open = f(SearchQuery {
            include_closed: Some(false),
            ..Default::default()
        });
        assert_eq!(ids(&open), ["1", "2", "t1"]);
        let pages = f(SearchQuery {
            kinds: Some(vec![SearchKind::Page]),
            ..Default::default()
        });
        assert_eq!(ids(&pages), ["1", "2"]);
        let tasks = f(SearchQuery {
            kinds: Some(vec![SearchKind::Task]),
            ..Default::default()
        });
        assert_eq!(ids(&tasks), ["t2", "t1"]);
        let p2 = f(SearchQuery {
            category_ids: Some(vec!["p2".into()]),
            ..Default::default()
        });
        assert_eq!(ids(&p2), ["2", "t2"]);
        let one = f(SearchQuery {
            limit: Some(1),
            ..Default::default()
        });
        assert_eq!(ids(&one), ["1"]);
        assert!(
            f(SearchQuery {
                kinds: Some(vec![]),
                ..Default::default()
            })
            .is_empty()
        );
        assert!(
            f(SearchQuery {
                category_ids: Some(vec![]),
                ..Default::default()
            })
            .is_empty()
        );
        // Another vector length is ignored, not an error.
        assert!(
            search(c, &[1.0, 0.0], &SearchQuery::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn only_pieces_with_vectors_count_and_best_piece_wins() {
        let (_d, db) = db();
        let c = db.conn();
        let long = "alpha beta gamma delta. ".repeat(60);
        add_page(
            c,
            "1",
            "p-general",
            "Page",
            &format!("# A\n\n{long}\n\n# B\n\n{long} b"),
        );
        let jobs = next_jobs(c, 10).unwrap();
        assert!(jobs.len() >= 2);
        // Only the second piece has a vector.
        store_vectors(c, &[(jobs[1].clone(), axis(2, 3, 0.0))]).unwrap();
        let hits = search(c, &axis(2, 3, 0.0), &SearchQuery::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, jobs[1].chunk_id);
        assert_eq!(hits[0].heading, "B");
        // Both pieces: the better one is reported, once.
        store_vectors(c, &[(jobs[0].clone(), axis(2, 4, 0.0))]).unwrap();
        let hits = search(c, &axis(2, 4, 0.2), &SearchQuery::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, jobs[0].chunk_id);
    }

    #[test]
    fn similar_items_exclude_self() {
        let (_d, db) = db();
        let c = db.conn();
        corpus(c);
        let hits = similar_items(c, SearchKind::Page, "1", &SearchQuery::default()).unwrap();
        assert_eq!(ids(&hits), ["2", "t2", "t1"]);
        let hits = similar_to_vectors(c, &[axis(1, 0, 0.3)], &SearchQuery::default()).unwrap();
        assert_eq!(hits[0].id, "t1");
        // An item without vectors has no similar items.
        add_page(c, "9", "p-general", "New", "new");
        assert!(
            similar_items(c, SearchKind::Page, "9", &SearchQuery::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn blob_round_trip_and_dot() {
        let v = vec![0.25f32, -1.5, 3.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let b = to_blob(&v);
        assert_eq!(from_blob(&b), v);
        let expect: f32 = v.iter().map(|x| x * x).sum();
        assert!((dot(&v, &b) - expect).abs() < 1e-4);
    }

    /// 50k pieces x 256 dims (target: well under 100 ms). Run with
    /// `cargo test -p cairn-core --release -- --ignored --nocapture bench_search`.
    #[test]
    #[ignore]
    fn bench_search_50k() {
        let (_d, db) = db();
        let c = db.conn();
        let dims = 256;
        add_category(c, "p2", "TWO");
        let mut state: u64 = 88172645463325252;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 2000) as f32 / 1000.0 - 1.0
        };
        let unit = |next: &mut dyn FnMut() -> f32| -> Vec<f32> {
            let mut v: Vec<f32> = (0..dims).map(|_| next()).collect();
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= n);
            v
        };
        c.execute_batch("BEGIN").unwrap();
        for i in 0..10_000 {
            let id = format!("pg{i}");
            let category = if i % 10 == 0 { "p2" } else { "p-general" };
            c.execute(
                "INSERT INTO pages (id, category_id, title, body, created_at, updated_at) VALUES (?1, ?2, ?1, '', 'x', 'x')",
                params![id, category],
            )
            .unwrap();
            for seq in 0..5 {
                let v = unit(&mut next);
                c.execute(
                    "INSERT INTO chunks (kind, ref_id, seq, heading, text, hash, vector)
                     VALUES ('page', ?1, ?2, 'h', 'some piece text', 'h', ?3)",
                    params![id, seq, to_blob(&v)],
                )
                .unwrap();
            }
        }
        c.execute_batch("COMMIT").unwrap();
        let query = unit(&mut next);
        for (name, f) in [
            ("no filter", SearchQuery::default()),
            (
                "one category (10%)",
                SearchQuery {
                    category_ids: Some(vec!["p2".into()]),
                    ..Default::default()
                },
            ),
        ] {
            let mut best = u128::MAX;
            for _ in 0..5 {
                let t = std::time::Instant::now();
                let hits = search(c, &query, &f).unwrap();
                best = best.min(t.elapsed().as_micros());
                assert!(!hits.is_empty());
            }
            println!(
                "search 50k x {dims} ({name}): best of 5 = {:.1} ms",
                best as f64 / 1000.0
            );
        }
    }
}

#[cfg(test)]
mod api_tests {
    //! The real save paths keep the pieces in step (through `hooks`).
    use super::*;
    use crate::Db;
    use crate::knowledge::{self, NewPage, PagePatch};
    use crate::tasks::{self, NewTask, TaskPatch};

    #[test]
    fn saves_comments_and_deletes_update_pieces() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_at(dir.path()).unwrap();
        let page = knowledge::create_page(
            &mut db,
            NewPage {
                category_id: "p-general".into(),
                title: "Hoses".into(),
                body: "# Pre-start\n\nCheck them.".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::create(
            &mut db,
            NewTask {
                category_id: "p-general".into(),
                title: "Fix pump".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(status(db.conn()).unwrap().chunks, 2);

        let comment = tasks::add_comment(&mut db, &task.id, "Ordered a seal").unwrap();
        assert_eq!(status(db.conn()).unwrap().chunks, 3);
        tasks::delete_comment(&mut db, &comment.id).unwrap();
        assert_eq!(status(db.conn()).unwrap().chunks, 2);

        knowledge::update_page(
            &mut db,
            &page.id,
            PagePatch {
                body: Some("# Pre-start\n\nCheck them twice.".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let text: String = db
            .conn()
            .query_row(
                "SELECT text FROM chunks WHERE ref_id = ?1",
                [&page.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(text, "Check them twice.");

        tasks::update(
            &mut db,
            &task.id,
            TaskPatch {
                title: Some("Fix the pump".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let jobs = next_jobs(db.conn(), 10).unwrap();
        assert!(jobs.iter().any(|j| j.title.ends_with("Fix the pump")));

        tasks::delete(&mut db, &task.id).unwrap();
        knowledge::delete_page(&mut db, &page.id).unwrap();
        assert_eq!(status(db.conn()).unwrap().chunks, 0);
    }
}
