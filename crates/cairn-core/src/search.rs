//! Keyword search over pages and tasks (FR-KN-UI-2, keyword part of FR-RAG-SR-1, NFR-I18N-4).
//! Owned by the M3 search agent.
//!
//! The FTS5 table `search_index` (migration 0003) holds one row per page and per task.
//! Text is "spaced": a separator is inserted between Han characters (CJK ideographs) so the
//! unicode61 tokenizer indexes each one; queries are spaced the same way and each query word
//! becomes a phrase, so 1-, 2- and 3+-character Chinese searches all work (M0-c). Ranking: bm25,
//! with title matches weighted above body matches.
//!
//! Indexed text uses U+200B (zero width space, a separator for unicode61) instead of a plain space
//! as the inserted separator. That makes snippets exactly reversible: removing U+200B restores the
//! original text, whatever spaces the user typed. Queries use plain spaces (same tokens).

use rusqlite::{Connection, params, params_from_iter, types::Value};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Result;
use crate::glossary;

/// Inserted between Han characters in indexed text (never occurs in user text: it is removed).
const SEP: char = '\u{200B}';
pub(crate) const DEFAULT_LIMIT: u32 = 20;
/// Column weights for bm25: kind, ref_id (unindexed, ignored), title, body, extra.
const BM25: &str = "bm25(search_index, 0.0, 0.0, 10.0, 1.0, 3.0)";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SearchKind {
    Page,
    Task,
    /// A glossary entry (FR-KN-GL-5): its own result type in the global search and the assistant.
    Term,
}

impl SearchKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            SearchKind::Page => "page",
            SearchKind::Task => "task",
            SearchKind::Term => "term",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    /// Default: both.
    #[specta(optional)]
    pub kinds: Option<Vec<SearchKind>>,
    #[specta(optional)]
    pub category_ids: Option<Vec<String>>,
    /// Archived pages are skipped unless true.
    #[specta(optional)]
    pub include_archived: Option<bool>,
    /// Closed tasks are included unless false (FR-TASK-ST-3: closed tasks can still be searched).
    #[specta(optional)]
    pub include_closed: Option<bool>,
    /// Default 20.
    #[specta(optional)]
    pub limit: Option<u32>,
}

/// Where a search result came from (FR-RAG-SR-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MatchedBy {
    Keyword,
    Meaning,
    Both,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: SearchKind,
    pub id: String,
    /// Page title, or task title.
    pub title: String,
    /// Task key for tasks.
    pub key: Option<String>,
    /// Empty for a global glossary entry.
    pub category_id: String,
    /// A short excerpt around the first match, with matches wrapped in `«` and `»`, unspaced.
    pub snippet: String,
    /// Higher is better.
    pub score: f64,
    /// Keyword search alone always says `keyword`; hybrid search fills it in (FR-RAG-SR-4).
    pub matched_by: MatchedBy,
}

/// Han characters: CJK Unified Ideographs (incl. extension A and B..H) and compatibility ideographs.
pub(crate) fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3007 | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x323AF)
}

fn spaced_with(text: &str, sep: char) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut prev: Option<char> = None;
    for c in text.chars() {
        if c == SEP {
            continue;
        }
        let split = prev.is_some_and(|p| {
            (is_han(p) && is_han(c))
                || (!p.is_whitespace() && !c.is_whitespace() && is_han(p) != is_han(c))
        });
        if split {
            out.push(sep);
        }
        out.push(c);
        prev = Some(c);
    }
    out
}

/// Inserts a space between Han characters (and between Han and other text) for indexing and queries.
pub fn spaced(text: &str) -> String {
    spaced_with(text, ' ')
}

/// Removes the markup that must not be searched: image links, and the targets of page and
/// attachment links (the link text stays).
pub(crate) fn strip_markdown(body: &str) -> String {
    let chars: Vec<char> = body.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while i < n {
        let image = chars[i] == '!' && chars.get(i + 1) == Some(&'[');
        if image || chars[i] == '[' {
            let open = if image { i + 1 } else { i };
            // [text](target) on one line
            let close = (open + 1..n).find(|&k| matches!(chars[k], ']' | '\n' | '['));
            if let Some(close) = close
                && chars[close] == ']'
                && chars.get(close + 1) == Some(&'(')
                && let Some(end) = (close + 2..n).find(|&k| matches!(chars[k], ')' | '\n'))
                && chars[end] == ')'
            {
                let target: String = chars[close + 2..end].iter().collect();
                if image {
                    i = end + 1;
                    continue;
                }
                if target.starts_with("page:") || target.starts_with("attachment:") {
                    out.extend(&chars[open + 1..close]);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn insert_row(
    conn: &Connection,
    kind: SearchKind,
    id: &str,
    title: &str,
    body: &str,
    extra: &str,
) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO search_index (kind, ref_id, title, body, extra) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![
        kind.as_str(),
        id,
        spaced_with(title, SEP),
        spaced_with(body, SEP),
        spaced_with(extra, SEP)
    ])?;
    conn.prepare_cached("INSERT INTO search_ref (rowid, kind, ref_id) VALUES (?1, ?2, ?3)")?
        .execute(params![conn.last_insert_rowid(), kind.as_str(), id])?;
    Ok(())
}

/// Joins the non-empty parts with newlines.
fn join_extra<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let mut extra = String::new();
    for part in parts {
        if !part.is_empty() {
            if !extra.is_empty() {
                extra.push('\n');
            }
            extra.push_str(part);
        }
    }
    extra
}

/// `terms` are the glossary lines of the entries the page uses (FR-RAG-IX-3).
fn page_row(
    title: &str,
    body: &str,
    source_info: Option<&str>,
    tags: Option<&str>,
    terms: &[String],
) -> [String; 3] {
    let extra = join_extra(
        [tags, source_info]
            .into_iter()
            .flatten()
            .chain(terms.iter().map(String::as_str)),
    );
    [title.to_string(), strip_markdown(body), extra]
}

fn task_row(
    key: &str,
    title: &str,
    description: &str,
    labels: Option<&str>,
    comments: Option<&str>,
    terms: &[String],
) -> [String; 3] {
    let mut body = strip_markdown(description);
    if let Some(c) = comments.filter(|c| !c.is_empty()) {
        body.push('\n');
        body.push_str(&strip_markdown(c));
    }
    let first = match labels {
        Some(l) if !l.is_empty() => format!("{key} {l}"),
        _ => key.to_string(),
    };
    let extra = join_extra(std::iter::once(first.as_str()).chain(terms.iter().map(String::as_str)));
    [title.to_string(), body, extra]
}

const PAGE_SELECT: &str = "SELECT id, title, body, source_info,
        (SELECT group_concat(tag, ' ') FROM page_tags WHERE page_id = pages.id) FROM pages";
const TASK_SELECT: &str = "SELECT id, key, title, description,
        (SELECT group_concat(label, ' ') FROM task_labels WHERE task_id = tasks.id),
        (SELECT group_concat(text, char(10)) FROM
            (SELECT text FROM task_comments WHERE task_id = tasks.id ORDER BY created_at))
     FROM tasks";

/// Re-indexes one page (title, body, tags, source info). Called from `hooks`.
pub(crate) fn index_page(conn: &Connection, page_id: &str) -> Result<()> {
    remove(conn, SearchKind::Page, page_id)?;
    let sql = format!("{PAGE_SELECT} WHERE id = ?1");
    let row = conn
        .prepare_cached(&sql)?
        .query_row([page_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    if let Some((id, title, body, source, tags)) = row {
        let terms = glossary::used_lines(conn, SearchKind::Page, &id)?;
        let [t, b, x] = page_row(&title, &body, source.as_deref(), tags.as_deref(), &terms);
        insert_row(conn, SearchKind::Page, &id, &t, &b, &x)?;
    }
    Ok(())
}

/// Re-indexes one task (key, title, description, labels, comments). Called from `hooks`.
pub(crate) fn index_task(conn: &Connection, task_id: &str) -> Result<()> {
    remove(conn, SearchKind::Task, task_id)?;
    let sql = format!("{TASK_SELECT} WHERE id = ?1");
    let row = conn
        .prepare_cached(&sql)?
        .query_row([task_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    if let Some((id, key, title, desc, labels, comments)) = row {
        let terms = glossary::used_lines(conn, SearchKind::Task, &id)?;
        let [t, b, x] = task_row(
            &key,
            &title,
            &desc,
            labels.as_deref(),
            comments.as_deref(),
            &terms,
        );
        insert_row(conn, SearchKind::Task, &id, &t, &b, &x)?;
    }
    Ok(())
}

/// Indexes one glossary entry as a result of its own (FR-KN-GL-5): the term is the title, the
/// description the body, the other names the extra text.
pub(crate) fn index_term(conn: &Connection, id: &str) -> Result<()> {
    remove(conn, SearchKind::Term, id)?;
    if let Some((term, names, description)) = glossary::entry_parts(conn, id)? {
        insert_row(conn, SearchKind::Term, id, &term, &description, &names)?;
    }
    Ok(())
}

pub(crate) fn remove(conn: &Connection, kind: SearchKind, id: &str) -> Result<()> {
    let rowid: Option<i64> = conn
        .prepare_cached("SELECT rowid FROM search_ref WHERE ref_id = ?1 AND kind = ?2")?
        .query_row(params![id, kind.as_str()], |r| r.get(0))
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    if let Some(rowid) = rowid {
        conn.prepare_cached("DELETE FROM search_index WHERE rowid = ?1")?
            .execute([rowid])?;
        conn.prepare_cached("DELETE FROM search_ref WHERE rowid = ?1")?
            .execute([rowid])?;
    }
    Ok(())
}

/// Turns user text into an FTS5 query: every word is a quoted phrase (so no user syntax reaches
/// FTS5), the last Latin word of 2+ characters is a prefix, words are ANDed. `None` if nothing to
/// search for.
#[cfg(test)]
fn build_match(text: &str) -> Option<String> {
    build_match_with(text, " AND ")
}

/// The words of a query, spaced as the index is, with whether the word may be a prefix (a Latin
/// word of 2+ characters; the caller only uses it for the last one).
fn query_phrases(text: &str) -> Vec<(String, bool)> {
    let mut phrases: Vec<(String, bool)> = Vec::new();
    for word in text.split_whitespace() {
        let sp = spaced(word);
        if !sp.chars().any(char::is_alphanumeric) {
            continue;
        }
        let prefixable =
            !word.chars().any(is_han) && word.chars().filter(|c| c.is_alphanumeric()).count() >= 2;
        phrases.push((sp, prefixable));
    }
    phrases
}

/// `build_match` with the words joined by `joiner` (`" AND "` or `" OR "`).
fn build_match_with(text: &str, joiner: &str) -> Option<String> {
    let phrases = query_phrases(text);
    let last = phrases.len().checked_sub(1)?;
    let parts: Vec<String> = phrases
        .iter()
        .enumerate()
        .map(|(i, (p, prefix))| {
            let p = p.replace('"', "\"\"");
            if i == last && *prefix {
                format!("\"{p}\"*")
            } else {
                format!("\"{p}\"")
            }
        })
        .collect();
    Some(parts.join(joiner))
}

/// Lower-case word tokens as the index splits them (Han characters one each).
fn tokens(text: &str) -> Vec<String> {
    spaced(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Which words of `query` occur in `text` the way the keyword index would find them (whole words
/// in a row; the last word may also be a prefix). One flag per query word, in order; empty when
/// the query has no words. The assistant uses it to tell a hit on an item's own text from one
/// that matched only through its glossary lines (D-44).
pub fn words_in(query: &str, text: &str) -> Vec<bool> {
    let words = tokens(text);
    let phrases = query_phrases(query);
    let last = phrases.len().saturating_sub(1);
    phrases
        .into_iter()
        .enumerate()
        .map(|(i, (phrase, prefixable))| {
            let p = tokens(&phrase);
            let Some((end, init)) = p.split_last() else {
                return false;
            };
            let prefix = prefixable && i == last;
            words.windows(p.len()).any(|w| {
                w[..init.len()] == *init
                    && if prefix {
                        w[init.len()].starts_with(end.as_str())
                    } else {
                        w[init.len()] == *end
                    }
            })
        })
        .collect()
}

/// Removes the separators and merges neighbouring match markers (`«中»«文»` becomes `«中文»`).
fn clean_snippet(raw: &str) -> String {
    let no_sep: String = raw.chars().filter(|&c| c != SEP).collect();
    let merged = no_sep.replace("»«", "");
    merged.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn search(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    search_with(conn, query, " AND ")
}

/// Like [`search`], but an item needs only one of the words (the assistant's retry when all the
/// words together find nothing, README "ask-plan").
pub fn search_any(conn: &Connection, query: &SearchQuery) -> Result<Vec<SearchHit>> {
    search_with(conn, query, " OR ")
}

fn search_with(conn: &Connection, query: &SearchQuery, joiner: &str) -> Result<Vec<SearchHit>> {
    let Some(m) = build_match_with(&query.text, joiner) else {
        return Ok(Vec::new());
    };
    // The FTS ranking runs alone in a materialised CTE (joining inside the FTS query made it
    // ~15x slower); pages and tasks are then looked up by primary key for the filters.
    let mut sql = format!(
        "WITH h AS MATERIALIZED (
             SELECT rowid AS rid, -{BM25} AS score
             FROM search_index WHERE search_index MATCH ?1)
         SELECT h.rid, r.kind, r.ref_id, COALESCE(p.title, t.title, g.term), t.key,
                COALESCE(p.category_id, t.category_id, g.category_id, ''), h.score
         FROM h
         JOIN search_ref r ON r.rowid = h.rid
         LEFT JOIN pages p ON r.kind = 'page' AND p.id = r.ref_id
         LEFT JOIN tasks t ON r.kind = 'task' AND t.id = r.ref_id
         LEFT JOIN glossary_entries g ON r.kind = 'term' AND g.id = r.ref_id
         WHERE (p.id IS NOT NULL OR t.id IS NOT NULL OR g.id IS NOT NULL)"
    );
    let mut args: Vec<Value> = vec![Value::Text(m.clone())];
    if let Some(kinds) = &query.kinds {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let list: Vec<String> = kinds.iter().map(|k| format!("'{}'", k.as_str())).collect();
        sql.push_str(&format!(" AND r.kind IN ({})", list.join(",")));
    }
    if let Some(ids) = &query.category_ids {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        // A global glossary entry passes any category filter (FR-KN-GL-5).
        sql.push_str(&format!(
            " AND (g.category_id IS NULL AND g.id IS NOT NULL
                   OR COALESCE(p.category_id, t.category_id, g.category_id) IN ({marks}))"
        ));
        args.extend(ids.iter().cloned().map(Value::Text));
    }
    if !query.include_archived.unwrap_or(false) {
        sql.push_str(" AND (p.id IS NULL OR p.archived = 0)");
    }
    if query.include_closed == Some(false) {
        sql.push_str(" AND (t.id IS NULL OR t.status NOT IN ('done', 'wont_do'))");
    }
    sql.push_str(" ORDER BY h.score DESC LIMIT ?");
    args.push(Value::Integer(i64::from(
        query.limit.unwrap_or(DEFAULT_LIMIT),
    )));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(args), |r| {
            let kind: String = r.get(1)?;
            Ok((
                r.get::<_, i64>(0)?,
                SearchHit {
                    kind: match kind.as_str() {
                        "page" => SearchKind::Page,
                        "term" => SearchKind::Term,
                        _ => SearchKind::Task,
                    },
                    id: r.get(2)?,
                    title: r.get(3)?,
                    key: r.get(4)?,
                    category_id: r.get(5)?,
                    snippet: String::new(),
                    score: r.get(6)?,
                    matched_by: MatchedBy::Keyword,
                },
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut snip = conn.prepare(
        "SELECT snippet(search_index, -1, '«', '»', '…', 24) FROM search_index
         WHERE search_index MATCH ?1 AND rowid = ?2",
    )?;
    let mut hits = Vec::with_capacity(rows.len());
    for (rowid, mut hit) in rows {
        let raw: String = snip.query_row(params![m, rowid], |r| r.get(0))?;
        hit.snippet = clean_snippet(&raw);
        hits.push(hit);
    }
    Ok(hits)
}

/// Rebuilds the whole index (after import — FR-DATA-7 — and on demand).
pub fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM search_index", [])?;
    conn.execute("DELETE FROM search_ref", [])?;
    let mut stmt = conn.prepare(PAGE_SELECT)?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: String = r.get(0)?;
        let terms = glossary::used_lines(conn, SearchKind::Page, &id)?;
        let [t, b, x] = page_row(
            &r.get::<_, String>(1)?,
            &r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?.as_deref(),
            r.get::<_, Option<String>>(4)?.as_deref(),
            &terms,
        );
        insert_row(conn, SearchKind::Page, &id, &t, &b, &x)?;
    }
    let mut stmt = conn.prepare(TASK_SELECT)?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let id: String = r.get(0)?;
        let terms = glossary::used_lines(conn, SearchKind::Task, &id)?;
        let [t, b, x] = task_row(
            &r.get::<_, String>(1)?,
            &r.get::<_, String>(2)?,
            &r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?.as_deref(),
            r.get::<_, Option<String>>(5)?.as_deref(),
            &terms,
        );
        insert_row(conn, SearchKind::Task, &id, &t, &b, &x)?;
    }
    drop(rows);
    drop(stmt);
    let ids = conn
        .prepare("SELECT id FROM glossary_entries")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for id in ids {
        index_term(conn, &id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Db;
    use crate::categories::{self, NewCategory, CategoryColor};
    use crate::tasks::{self, NewTask, TaskPatch, TaskStatus};

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

    fn add_page(
        c: &Connection,
        category: &str,
        title: &str,
        body: &str,
        tags: &[&str],
        archived: bool,
    ) -> String {
        let id = crate::util::new_id();
        c.execute(
            "INSERT INTO pages (id, category_id, title, body, source_info, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'from the wiki', ?5, 'x', 'x')",
            params![id, category, title, body, archived],
        )
        .unwrap();
        for t in tags {
            c.execute("INSERT INTO page_tags VALUES (?1, ?2)", params![id, t])
                .unwrap();
        }
        index_page(c, &id).unwrap();
        id
    }

    fn task(db: &mut Db, category: &str, title: &str, desc: &str) -> tasks::Task {
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
    }

    fn q(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            ..Default::default()
        }
    }

    fn ids(c: &Connection, query: &SearchQuery) -> Vec<String> {
        search(c, query)
            .unwrap()
            .into_iter()
            .map(|h| h.id)
            .collect()
    }

    #[test]
    fn spaced_han() {
        assert_eq!(spaced("中文abc"), "中 文 abc");
        assert_eq!(spaced("abc中文def"), "abc 中 文 def");
        assert_eq!(spaced("hello world"), "hello world");
        assert_eq!(spaced("中 文"), "中 文");
        assert_eq!(spaced("版本v2.0（测试）"), "版 本 v2.0（ 测 试 ）");
        assert_eq!(spaced("\u{3400}\u{F900}"), "\u{3400} \u{F900}");
    }

    #[test]
    fn match_building() {
        assert_eq!(build_match("  "), None);
        assert_eq!(build_match("- * :"), None);
        assert_eq!(build_match("foo bar").unwrap(), "\"foo\" AND \"bar\"*");
        assert_eq!(build_match("a").unwrap(), "\"a\"");
        assert_eq!(build_match("中文").unwrap(), "\"中 文\"");
        assert_eq!(
            build_match("say \"hi\"").unwrap(),
            "\"say\" AND \"\"\"hi\"\"\"*"
        );
    }

    #[test]
    fn strips_links_and_images() {
        let s = strip_markdown(
            "See [Hybrid search](page:0192ab) and ![fig](attachment:xx) and [ext](https://a.b) [x]",
        );
        assert_eq!(s, "See Hybrid search and  and [ext](https://a.b) [x]");
    }

    #[test]
    fn english_search_and_ranking() {
        let mut e = env();
        let c = e.db.conn();
        let body_hit = add_page(
            c,
            &e.a,
            "Notes",
            "The pipeline partitions tables by month.",
            &[],
            false,
        );
        let title_hit = add_page(
            c,
            &e.a,
            "Pipeline design",
            "Something else entirely.",
            &[],
            false,
        );
        let hits = search(c, &q("pipeline")).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, title_hit);
        assert_eq!(hits[1].id, body_hit);
        assert!(hits[0].score > hits[1].score);
        assert!(
            hits[1].snippet.contains("«pipeline»"),
            "{}",
            hits[1].snippet
        );
        // AND of words, diacritics and case folded
        assert_eq!(ids(c, &q("pipeline MONTH")), vec![body_hit.clone()]);
        assert!(ids(c, &q("pipeline zzz")).is_empty());
        assert_eq!(ids(c, &q("pipelïne month")), vec![body_hit]);
        let _ = &mut e;
    }

    #[test]
    fn prefix_typing() {
        let e = env();
        let c = e.db.conn();
        let p = add_page(c, &e.a, "Deployment checklist", "", &[], false);
        for typed in ["dep", "deploy", "deployment", "Deployment chec"] {
            assert_eq!(ids(c, &q(typed)), vec![p.clone()], "{typed}");
        }
        assert!(ids(c, &q("d")).is_empty() || ids(c, &q("d")) == vec![p.clone()]);
        assert!(ids(c, &q("deployments")).is_empty());
    }

    #[test]
    fn chinese_queries() {
        let e = env();
        let c = e.db.conn();
        let p1 = add_page(
            c,
            &e.a,
            "数据管道设计",
            "我们按月对表进行分区，提高查询速度。",
            &[],
            false,
        );
        let p2 = add_page(
            c,
            &e.a,
            "Mixed",
            "Use the 管道 for data版本2 releases",
            &[],
            false,
        );
        for qs in ["管", "数据", "管道", "数据管", "分区", "查询速度", "版本"] {
            let got = ids(c, &q(qs));
            assert!(!got.is_empty(), "{qs}");
        }
        assert_eq!(ids(c, &q("分区")), vec![p1.clone()]);
        assert_eq!(ids(c, &q("查询速度")), vec![p1.clone()]);
        // adjacency matters: 管 and 道 in the wrong order does not match the phrase
        assert!(ids(c, &q("道管")).is_empty());
        let mut both = ids(c, &q("管道"));
        both.sort();
        let mut want = vec![p1, p2.clone()];
        want.sort();
        assert_eq!(both, want);
        assert_eq!(ids(c, &q("版本2")), vec![p2.clone()]);
        assert_eq!(ids(c, &q("data版本")), vec![p2]);
    }

    #[test]
    fn snippets_are_unspaced() {
        let e = env();
        let c = e.db.conn();
        add_page(
            c,
            &e.a,
            "T",
            "我们按月对表进行分区，提高查询速度。 and Latin words",
            &[],
            false,
        );
        let h = &search(c, &q("分区")).unwrap()[0];
        assert!(
            h.snippet.contains("按月对表进行«分区»，提高"),
            "{}",
            h.snippet
        );
        assert!(!h.snippet.contains('\u{200B}'));
        let h = &search(c, &q("Latin")).unwrap()[0];
        assert!(
            h.snippet.contains("速度。 and «Latin» words"),
            "{}",
            h.snippet
        );
        // a user's own space between Han characters is kept
        add_page(c, &e.a, "T2", "好 的 plan", &[], false);
        let h = &search(c, &q("plan")).unwrap()[0];
        assert!(h.snippet.contains("好 的 «plan»"), "{}", h.snippet);
    }

    #[test]
    fn special_characters_do_not_break() {
        let e = env();
        let c = e.db.conn();
        let p = add_page(
            c,
            &e.a,
            "Notes",
            "a-b c:d (e) \"quoted\" *star*",
            &[],
            false,
        );
        for qs in [
            "\"",
            "*",
            "-",
            ":",
            "(",
            "\"quoted\"",
            "a-b",
            "c:d",
            "(e)",
            "foo\" OR \"bar",
            "NOT",
            "AND",
            "x*y",
            "^",
            "NEAR(a b)",
            "'",
            "\\",
        ] {
            search(c, &q(qs)).unwrap_or_else(|e| panic!("{qs}: {e}"));
        }
        assert_eq!(ids(c, &q("\"quoted\"")), vec![p.clone()]);
        assert_eq!(ids(c, &q("a-b")), vec![p]);
        assert!(search(c, &q("")).unwrap().is_empty());
        assert!(search(c, &q(" \t ")).unwrap().is_empty());
    }

    #[test]
    fn filters() {
        let mut e = env();
        let (a, b) = (e.a.clone(), e.b.clone());
        let pa = add_page(e.db.conn(), &a, "Zebra page", "", &[], false);
        let pb = add_page(e.db.conn(), &b, "Zebra other", "", &[], false);
        let pz = add_page(e.db.conn(), &a, "Zebra old", "", &[], true);
        let t1 = task(&mut e.db, &a, "Zebra task", "");
        let t2 = task(&mut e.db, &a, "Zebra closed", "");
        tasks::update(
            &mut e.db,
            &t2.id,
            TaskPatch {
                status: Some(TaskStatus::Done),
                ..Default::default()
            },
        )
        .unwrap();
        let c = e.db.conn();
        let all = |qq: &SearchQuery| {
            let mut v = ids(c, qq);
            v.sort();
            v
        };
        let sorted = |mut v: Vec<String>| {
            v.sort();
            v
        };
        let base = q("zebra");
        assert_eq!(
            all(&base),
            sorted(vec![pa.clone(), pb.clone(), t1.id.clone(), t2.id.clone()])
        );
        // archived
        let mut qq = base.clone();
        qq.include_archived = Some(true);
        assert_eq!(all(&qq).len(), 5);
        assert!(all(&qq).contains(&pz));
        // closed
        let mut qq = base.clone();
        qq.include_closed = Some(false);
        assert_eq!(
            all(&qq),
            sorted(vec![pa.clone(), pb.clone(), t1.id.clone()])
        );
        // kinds
        let mut qq = base.clone();
        qq.kinds = Some(vec![SearchKind::Task]);
        assert_eq!(all(&qq), sorted(vec![t1.id.clone(), t2.id.clone()]));
        qq.kinds = Some(vec![SearchKind::Page]);
        assert_eq!(all(&qq), sorted(vec![pa.clone(), pb.clone()]));
        qq.kinds = Some(vec![]);
        assert!(all(&qq).is_empty());
        // category
        let mut qq = base.clone();
        qq.category_ids = Some(vec![b.clone()]);
        assert_eq!(all(&qq), vec![pb.clone()]);
        qq.category_ids = Some(vec![a.clone(), b.clone()]);
        assert_eq!(all(&qq).len(), 4);
        // limit
        let mut qq = base.clone();
        qq.limit = Some(2);
        assert_eq!(ids(c, &qq).len(), 2);
        // hit fields
        let h = search(c, &q("zebra task")).unwrap();
        assert_eq!(h[0].kind, SearchKind::Task);
        assert_eq!(h[0].key.as_deref(), Some("AAA-1"));
        assert_eq!(h[0].category_id, a);
        assert_eq!(h[0].title, "Zebra task");
    }

    #[test]
    fn pages_tags_source_and_links() {
        let e = env();
        let c = e.db.conn();
        let p = add_page(
            c,
            &e.a,
            "P",
            "See [Hybrid search](page:0192abcdef) now ![pic](attachment:99zz)",
            &["Architecture"],
            false,
        );
        assert_eq!(ids(c, &q("architecture")), vec![p.clone()]);
        assert_eq!(ids(c, &q("wiki")), vec![p.clone()]);
        assert_eq!(ids(c, &q("hybrid search")), vec![p.clone()]);
        assert!(ids(c, &q("0192abcdef")).is_empty());
        assert!(ids(c, &q("pic")).is_empty());
        assert!(ids(c, &q("99zz")).is_empty());
    }

    #[test]
    fn task_index_follows_the_tasks_module() {
        let mut e = env();
        let a = e.a.clone();
        let t = tasks::create(
            &mut e.db,
            NewTask {
                category_id: a,
                title: "Fix loader".into(),
                description: Some("The 加载器 crashes on startup, see [Spec](page:abc123)".into()),
                labels: Some(vec!["backend".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let first = |e: &Env, s: &str| ids(e.db.conn(), &q(s));
        assert_eq!(first(&e, "loader"), vec![t.id.clone()]);
        assert_eq!(first(&e, "加载"), vec![t.id.clone()]);
        assert_eq!(first(&e, "backend"), vec![t.id.clone()]);
        assert_eq!(first(&e, "AAA-1"), vec![t.id.clone()]);
        assert_eq!(first(&e, "#AAA-1"), vec![t.id.clone()]);
        assert!(first(&e, "abc123").is_empty());
        // comments
        let c1 = tasks::add_comment(&mut e.db, &t.id, "Reproduced with the nightly build").unwrap();
        let c2 = tasks::add_comment(&mut e.db, &t.id, "根因是内存泄漏").unwrap();
        assert_eq!(first(&e, "nightly"), vec![t.id.clone()]);
        assert_eq!(first(&e, "内存泄漏"), vec![t.id.clone()]);
        tasks::update_comment(&mut e.db, &c1.id, "Reproduced on weekly").unwrap();
        assert!(first(&e, "nightly").is_empty());
        assert_eq!(first(&e, "weekly"), vec![t.id.clone()]);
        tasks::delete_comment(&mut e.db, &c2.id).unwrap();
        assert!(first(&e, "内存泄漏").is_empty());
        assert_eq!(first(&e, "weekly"), vec![t.id.clone()]);
        // update title and labels
        tasks::update(
            &mut e.db,
            &t.id,
            TaskPatch {
                title: Some("Repair loader".into()),
                labels: Some(vec!["frontend".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(first(&e, "repair"), vec![t.id.clone()]);
        assert!(first(&e, "fix").is_empty());
        assert!(first(&e, "backend").is_empty());
        assert_eq!(first(&e, "frontend"), vec![t.id.clone()]);
        // one row only
        let n: i64 =
            e.db.conn()
                .query_row("SELECT count(*) FROM search_index", [], |r| r.get(0))
                .unwrap();
        assert_eq!(n, 1);
        // delete
        tasks::delete(&mut e.db, &t.id).unwrap();
        assert!(first(&e, "loader").is_empty());
        let n: i64 =
            e.db.conn()
                .query_row("SELECT count(*) FROM search_index", [], |r| r.get(0))
                .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn rebuild_restores_everything() {
        let mut e = env();
        let a = e.a.clone();
        let p = add_page(
            e.db.conn(),
            &a,
            "Rebuild page",
            "中文内容",
            &["tagx"],
            false,
        );
        let t = task(&mut e.db, &a, "Rebuild task", "desc");
        tasks::add_comment(&mut e.db, &t.id, "comment words").unwrap();
        e.db.conn().execute("DELETE FROM search_index", []).unwrap();
        assert!(ids(e.db.conn(), &q("rebuild")).is_empty());
        rebuild(e.db.conn()).unwrap();
        rebuild(e.db.conn()).unwrap();
        let c = e.db.conn();
        assert_eq!(ids(c, &q("rebuild")).len(), 2);
        assert_eq!(ids(c, &q("内容")), vec![p.clone()]);
        assert_eq!(ids(c, &q("tagx")), vec![p]);
        assert_eq!(ids(c, &q("comment words")), vec![t.id]);
    }

    /// Run with `cargo test -p cairn-core --release --lib search::tests::perf -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn perf_10k_pages_20k_tasks() {
        use std::time::Instant;
        let e = env();
        let c = e.db.conn();
        let words = [
            "pipeline",
            "sensor",
            "data",
            "partition",
            "query",
            "index",
            "search",
            "report",
            "deploy",
            "release",
            "cluster",
            "storage",
            "latency",
            "model",
            "vector",
            "schema",
            "migration",
            "review",
            "backlog",
            "sprint",
            "design",
            "cache",
            "network",
            "buffer",
            "thread",
        ];
        let han = "数据管道设计分区查询速度索引搜索报告部署发布集群存储延迟模型向量迁移评审";
        let han: Vec<char> = han.chars().collect();
        let mut seed = 12345u64;
        let mut rnd = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        let text = |n: usize, rnd: &mut dyn FnMut(usize) -> usize| {
            let mut s = String::new();
            for i in 0..n {
                if i % 3 == 2 {
                    for _ in 0..4 {
                        s.push(han[rnd(han.len())]);
                    }
                    s.push(' ');
                } else {
                    s.push_str(words[rnd(words.len())]);
                    s.push(' ');
                    if rnd(8) == 0 {
                        s.push_str(&format!("w{} ", rnd(3000)));
                    }
                }
            }
            s
        };
        let tx = c.unchecked_transaction().unwrap();
        for i in 0..10_000 {
            tx.execute(
                "INSERT INTO pages (id, category_id, title, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 'x', 'x')",
                params![format!("p{i}"), e.a, format!("Page {i} {}", text(4, &mut rnd)), text(150, &mut rnd)],
            )
            .unwrap();
        }
        for i in 0..20_000 {
            tx.execute(
                "INSERT INTO tasks (id, key, number, category_id, title, description, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'x', 'x')",
                params![format!("t{i}"), format!("AAA-{}", i + 1), i + 1, e.a, text(6, &mut rnd), text(40, &mut rnd)],
            )
            .unwrap();
            if i % 2 == 0 {
                tx.execute(
                    "INSERT INTO task_comments (id, task_id, text, created_at) VALUES (?1, ?2, ?3, 'x')",
                    params![format!("c{i}"), format!("t{i}"), text(20, &mut rnd)],
                )
                .unwrap();
            }
        }
        tx.commit().unwrap();
        let t0 = Instant::now();
        rebuild(c).unwrap();
        println!("rebuild 10k pages + 20k tasks: {:?}", t0.elapsed());
        let queries = [
            "pipeline",
            "pipeline data",
            "data",
            "sens",
            "数据",
            "管道",
            "管",
            "pipeline 数据",
            "w17",
            "zzzz",
            "partition query index",
            "AAA-123",
        ];
        let mut times = Vec::new();
        for round in 0..20 {
            for qs in queries {
                let t0 = Instant::now();
                let r = search(c, &q(qs)).unwrap();
                let d = t0.elapsed().as_secs_f64() * 1000.0;
                if round == 0 {
                    println!("{qs:25} {:5} hits {d:7.1} ms", r.len());
                }
                times.push(d);
            }
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms",
            times[times.len() / 2],
            times[times.len() * 95 / 100],
            times[times.len() - 1]
        );
        // single-row update cost
        let t0 = Instant::now();
        for i in 0..100 {
            index_task(c, &format!("t{i}")).unwrap();
        }
        println!(
            "index_task: {:.2} ms each",
            t0.elapsed().as_secs_f64() * 10.0
        );
    }
}
