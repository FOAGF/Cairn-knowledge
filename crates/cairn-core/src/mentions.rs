//! Mention index for "Mentioned in" lists (DM-MN-4).
//!
//! Syntax: tasks `#KEY` (current key or alias, resolved to the task id when indexed),
//! pages `[Title](page:<id>)`. Mentions inside code spans/blocks don't count.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::knowledge::{Backlink, MentionSource, MentionTarget};
use crate::tasks::enum_str;
use crate::{CoreError, Result, tasks};

// ---------------------------------------------------------------- parsing

/// Applies `f` to every stretch of `text` that is not code (fenced blocks and inline code spans
/// are copied unchanged). Inline spans are matched within a line; an unclosed fence runs to the end.
pub(crate) fn map_prose(text: &str, mut f: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<(char, usize)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = fence_marker(trimmed);
        if let Some((c, n)) = fence {
            out.push_str(line);
            if indent <= 3 {
                if let Some((c2, n2)) = marker {
                    if c2 == c && n2 >= n && trimmed.trim_start_matches(c).trim().is_empty() {
                        fence = None;
                    }
                }
            }
            continue;
        }
        if indent <= 3 {
            if let Some(m) = marker {
                fence = Some(m);
                out.push_str(line);
                continue;
            }
        }
        map_inline(line, &mut f, &mut out);
    }
    out
}

/// `(char, run length)` when the line starts a code fence of at least three ` or ~.
fn fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let c = trimmed.chars().next()?;
    if c != '`' && c != '~' {
        return None;
    }
    let n = trimmed.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

fn map_inline(line: &str, f: &mut impl FnMut(&str) -> String, out: &mut String) {
    let bytes = line.as_bytes();
    let mut prose_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
        // Find the closing run of exactly the same length.
        let mut j = i + run;
        let mut close = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let r = bytes[j..].iter().take_while(|&&b| b == b'`').count();
                if r == run {
                    close = Some(j + r);
                    break;
                }
                j += r;
            } else {
                j += 1;
            }
        }
        match close {
            Some(end) => {
                out.push_str(&f(&line[prose_start..i]));
                out.push_str(&line[i..end]);
                prose_start = end;
                i = end;
            }
            None => i += run,
        }
    }
    out.push_str(&f(&line[prose_start..]));
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A task mention at the start of `s` (after the `#`): `[A-Z][A-Z0-9]{1,9}-\d+` ending at a word boundary.
/// Returns the key and the byte length consumed.
fn task_key_at(s: &str) -> Option<(&str, usize)> {
    let b = s.as_bytes();
    if b.is_empty() || !b[0].is_ascii_uppercase() {
        return None;
    }
    let mut i = 1;
    while i < b.len() && (b[i].is_ascii_uppercase() || b[i].is_ascii_digit()) {
        i += 1;
    }
    if !(2..=10).contains(&i) || b.get(i) != Some(&b'-') {
        return None;
    }
    let digits_start = i + 1;
    let mut j = digits_start;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j == digits_start {
        return None;
    }
    if s[j..].chars().next().is_some_and(is_word) {
        return None;
    }
    Some((&s[..j], j))
}

/// Calls `f(key, start, end)` for each `#KEY` in `s` (byte range of the whole `#KEY`).
fn for_each_task_mention(s: &str, mut f: impl FnMut(&str, usize, usize)) {
    let mut prev: Option<char> = None;
    let mut skip_to = 0;
    for (i, c) in s.char_indices() {
        if i < skip_to {
            continue;
        }
        if c == '#' && prev.is_none_or(|p| !is_word(p) && p != '&') {
            if let Some((key, len)) = task_key_at(&s[i + 1..]) {
                f(key, i, i + 1 + len);
                skip_to = i + 1 + len;
                prev = key.chars().last();
                continue;
            }
        }
        prev = Some(c);
    }
}

/// Calls `f(title, id, start, end)` for each `[Title](page:<id>)` in `s`.
fn for_each_page_mention(s: &str, mut f: impl FnMut(&str, &str, usize, usize)) {
    let mut from = 0;
    while let Some(rel) = s[from..].find("](page:") {
        let close_title = from + rel;
        let id_start = close_title + "](page:".len();
        let id_len = s[id_start..]
            .find(|c: char| c == ')' || c.is_whitespace())
            .unwrap_or(s.len() - id_start);
        let id_end = id_start + id_len;
        if s[id_end..].starts_with(')') && id_len > 0 {
            if let Some(open) = s[..close_title].rfind('[') {
                if !s[open + 1..close_title].contains('\n') {
                    f(
                        &s[open + 1..close_title],
                        &s[id_start..id_end],
                        open,
                        id_end + 1,
                    );
                }
            }
        }
        from = id_end.max(id_start);
    }
}

/// Page ids and task keys mentioned in `text` outside code, in order of first appearance.
pub(crate) fn parse(text: &str) -> (Vec<String>, Vec<String>) {
    let mut keys: Vec<String> = Vec::new();
    let mut pages: Vec<String> = Vec::new();
    map_prose(text, |s| {
        for_each_task_mention(s, |k, _, _| {
            if !keys.iter().any(|x| x == k) {
                keys.push(k.to_string());
            }
        });
        for_each_page_mention(s, |_, id, _, _| {
            if !pages.iter().any(|x| x == id) {
                pages.push(id.to_string());
            }
        });
        String::new()
    });
    (pages, keys)
}

/// For Markdown export: page mentions become the page's title, `#KEY` becomes `KEY Title`.
/// The callbacks return `None` for unknown items (then the original text is kept).
pub(crate) fn to_readable(
    text: &str,
    page_title: &mut dyn FnMut(&str) -> Option<String>,
    task_label: &mut dyn FnMut(&str) -> Option<String>,
) -> String {
    map_prose(text, |s| {
        let mut reps: Vec<(usize, usize, String)> = Vec::new();
        for_each_task_mention(s, |k, a, b| {
            if let Some(label) = task_label(k) {
                reps.push((a, b, label));
            }
        });
        for_each_page_mention(s, |title, id, a, b| {
            let t = page_title(id).unwrap_or_else(|| title.to_string());
            reps.push((a, b, t));
        });
        reps.sort_by_key(|r| r.0);
        let mut out = String::new();
        let mut pos = 0;
        for (a, b, r) in reps {
            if a < pos {
                continue;
            }
            out.push_str(&s[pos..a]);
            out.push_str(&r);
            pos = b;
        }
        out.push_str(&s[pos..]);
        out
    })
}

// ---------------------------------------------------------------- index

/// Replaces the index rows of one source with the mentions found in `text`.
pub(crate) fn index_source(
    conn: &Connection,
    source: MentionSource,
    source_id: &str,
    text: &str,
) -> Result<()> {
    remove_source(conn, source, source_id)?;
    let (pages, keys) = parse(text);
    let mut targets: BTreeSet<(&'static str, String)> = BTreeSet::new();
    for id in pages {
        if source == MentionSource::Page && id == source_id {
            continue;
        }
        let exists = conn
            .query_row("SELECT 1 FROM pages WHERE id = ?1", [&id], |_| Ok(()))
            .optional()?
            .is_some();
        if exists {
            targets.insert(("page", id));
        }
    }
    for key in keys {
        match tasks::get(conn, &key) {
            Ok(t) => {
                if source == MentionSource::Task && t.id == source_id {
                    continue;
                }
                targets.insert(("task", t.id));
            }
            Err(CoreError::NotFound { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    for (kind, id) in targets {
        conn.execute(
            "INSERT OR IGNORE INTO mentions (source_kind, source_id, target_kind, target_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![enum_str(&source), source_id, kind, id],
        )?;
    }
    Ok(())
}

/// Removes every row where the source or the target is this item (and, for a task, its comments).
pub(crate) fn remove_item(conn: &Connection, target: MentionTarget, id: &str) -> Result<()> {
    let kind = enum_str(&target);
    conn.execute(
        "DELETE FROM mentions WHERE target_kind = ?1 AND target_id = ?2",
        params![kind, id],
    )?;
    conn.execute(
        "DELETE FROM mentions WHERE source_kind = ?1 AND source_id = ?2",
        params![kind, id],
    )?;
    if target == MentionTarget::Task {
        conn.execute(
            "DELETE FROM mentions WHERE source_kind = 'comment'
               AND source_id IN (SELECT id FROM task_comments WHERE task_id = ?1)",
            [id],
        )?;
    }
    Ok(())
}

/// Removes the rows of one source (e.g. a deleted comment).
pub(crate) fn remove_source(
    conn: &Connection,
    source: MentionSource,
    source_id: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM mentions WHERE source_kind = ?1 AND source_id = ?2",
        params![enum_str(&source), source_id],
    )?;
    Ok(())
}

/// "Mentioned in" for a task or page, newest source first.
pub fn backlinks(conn: &Connection, target: MentionTarget, id: &str) -> Result<Vec<Backlink>> {
    let kind = enum_str(&target);
    let mut out = Vec::new();

    let mut stmt = conn.prepare(
        "SELECT p.id, p.title, p.updated_at FROM mentions m JOIN pages p ON p.id = m.source_id
         WHERE m.source_kind = 'page' AND m.target_kind = ?1 AND m.target_id = ?2",
    )?;
    let rows = stmt
        .query_map(params![kind, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (pid, title, at) in rows {
        out.push(Backlink {
            source: MentionSource::Page,
            source_id: pid.clone(),
            page_id: Some(pid),
            task_id: None,
            label: title,
            at,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT t.id, t.key, t.title, t.updated_at FROM mentions m JOIN tasks t ON t.id = m.source_id
         WHERE m.source_kind = 'task' AND m.target_kind = ?1 AND m.target_id = ?2",
    )?;
    let rows = stmt
        .query_map(params![kind, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (tid, key, title, at) in rows {
        out.push(Backlink {
            source: MentionSource::Task,
            source_id: tid.clone(),
            page_id: None,
            task_id: Some(tid),
            label: format!("{key} {title}"),
            at,
        });
    }

    let mut stmt = conn.prepare(
        "SELECT c.id, t.id, t.key, t.title, COALESCE(c.updated_at, c.created_at)
         FROM mentions m JOIN task_comments c ON c.id = m.source_id JOIN tasks t ON t.id = c.task_id
         WHERE m.source_kind = 'comment' AND m.target_kind = ?1 AND m.target_id = ?2",
    )?;
    let rows = stmt
        .query_map(params![kind, id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (cid, tid, key, title, at) in rows {
        out.push(Backlink {
            source: MentionSource::Comment,
            source_id: cid,
            page_id: None,
            task_id: Some(tid),
            label: format!("{key} {title}"),
            at,
        });
    }

    out.sort_by(|a, b| b.at.cmp(&a.at).then(a.source_id.cmp(&b.source_id)));
    Ok(out)
}

/// Rebuilds the whole index from all pages, task descriptions and comments (after import, dev seed).
pub fn rebuild(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM mentions", [])?;
    let rows = |sql: &str| -> Result<Vec<(String, String)>> {
        let mut stmt = conn.prepare(sql)?;
        let v = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    };
    for (id, text) in rows("SELECT id, body FROM pages")? {
        index_source(conn, MentionSource::Page, &id, &text)?;
    }
    for (id, text) in rows("SELECT id, description FROM tasks")? {
        index_source(conn, MentionSource::Task, &id, &text)?;
    }
    for (id, text) in rows("SELECT id, text FROM task_comments")? {
        index_source(conn, MentionSource::Comment, &id, &text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ignores_code_and_unknown_shapes() {
        let text = "See #PRX-12 and [Intro](page:abc-1).\n\
                    `#PRX-13` and ``[x](page:in-code)`` ignored.\n\
                    ```\n#PRX-14\n[y](page:fenced)\n```\n\
                    ~~~rust\n#PRX-15\n~~~\n\
                    a#PRX-16 &#PRX-17 #prx-18 #P-1 #PRX-19x (#PRX-20) 中#PRX-21\n\
                    #PRX-12 again";
        let (pages, keys) = parse(text);
        assert_eq!(pages, vec!["abc-1"]);
        assert_eq!(keys, vec!["PRX-12", "PRX-20", "PRX-21"]);
    }

    #[test]
    fn unclosed_fence_runs_to_the_end() {
        let (pages, keys) = parse("#AA-1\n```\n#AA-2\n[x](page:p)");
        assert_eq!(keys, vec!["AA-1"]);
        assert!(pages.is_empty());
    }

    #[test]
    fn readable_text_keeps_code() {
        let out = to_readable(
            "Do #AB-1 per [Old name](page:p1) `#AB-1` [Gone](page:zz)",
            &mut |id| (id == "p1").then(|| "New name".to_string()),
            &mut |k| (k == "AB-1").then(|| "AB-1 Fix it".to_string()),
        );
        assert_eq!(out, "Do AB-1 Fix it per New name `#AB-1` Gone");
    }
}
