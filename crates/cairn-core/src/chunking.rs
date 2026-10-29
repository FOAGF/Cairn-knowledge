//! FR-RAG-IX-4: cutting pages and tasks into pieces for the search model.
//!
//! Pages are split at headings into pieces of about [`TARGET_TOKENS`] (never more than
//! [`MAX_TOKENS`]). Long sections are split at paragraph breaks (then at sentence ends, then hard
//! cuts as a last resort); short neighbouring sections are joined. Each piece knows its heading
//! path ("Pre-start › Hoses"). Pieces don't overlap. A task is one piece (key, title, status,
//! description — split like a page section if it is long) plus pieces for its comments, grouped
//! up to the target size.
//!
//! Text is cleaned for the model: page mentions `[Title](page:<id>)` become their title, images
//! and attachment links are dropped, code blocks are kept as plain text.
//!
//! Owned by the M4 search-core agent. Pure functions: no database.

/// Aim for pieces about this long (in estimated tokens).
pub const TARGET_TOKENS: usize = 350;
/// Never make a piece longer than this.
pub const MAX_TOKENS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    /// Heading path, e.g. "Pre-start › Hoses"; empty before the first heading.
    pub heading: String,
    pub text: String,
}

use crate::search::{is_han, strip_markdown};

/// Separator of heading levels in [`Piece::heading`].
const PATH_SEP: &str = " › ";

/// Cost of one character in quarter tokens: Han characters 4, anything else 1.
fn quarter_cost(c: char) -> usize {
    if is_han(c) { 4 } else { 1 }
}

/// Rough token count for EmbeddingGemma's tokenizer, erring on the high side: Latin text at
/// about 4 characters per token (rounded up), each Han character about 1 token.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().map(quarter_cost).sum::<usize>().div_ceil(4)
}

/// Splits a page body (Markdown) into pieces. An empty body gives no pieces (the page is still
/// found by its title: the caller embeds the title alone).
pub fn split_page(body: &str) -> Vec<Piece> {
    let cleaned = strip_markdown(body);
    let mut parts: Vec<Piece> = Vec::new();
    for (heading, text) in sections(&cleaned) {
        if estimate_tokens(&text) <= MAX_TOKENS {
            push_clean(&mut parts, &heading, &text);
        } else {
            for chunk in pack(units(&text), TARGET_TOKENS) {
                push_clean(&mut parts, &heading, &chunk);
            }
        }
    }
    merge(parts)
}

/// Splits a task: the first piece starts with "KEY: title (status)", then the description;
/// comments follow in their own pieces, oldest first.
pub fn split_task(
    key: &str,
    title: &str,
    status: &str,
    description: &str,
    comments: &[String],
) -> Vec<Piece> {
    let mut out = Vec::new();
    let first = format!(
        "{key}: {title} ({status})\n\n{}",
        strip_markdown(description)
    );
    for chunk in pack(units(first.trim()), TARGET_TOKENS) {
        push_clean(&mut out, "", &chunk);
    }
    let mut comment_units = Vec::new();
    for c in comments {
        let c = strip_markdown(c);
        let c = c.trim();
        if c.is_empty() {
            continue;
        }
        // Each comment starts a unit; a long one is cut like a page section.
        let mut us = units(c);
        if let Some(u) = us.first_mut() {
            u.sep = "\n\n";
        }
        comment_units.extend(us);
    }
    for chunk in pack(comment_units, TARGET_TOKENS) {
        push_clean(&mut out, "", &chunk);
    }
    out
}

/// A run of text that is not cut further, with the separator that joins it to the previous
/// unit when they share a piece.
struct Unit {
    text: String,
    sep: &'static str,
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// `(level, text)` if `line` is an ATX heading.
fn heading_of(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let level = t.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &t[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim().to_string()))
}

/// The body cut at headings: `(heading path, text)`. Headings inside code fences don't count.
fn sections(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut path = String::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let flush = |path: &str, cur: &mut Vec<&str>, out: &mut Vec<(String, String)>| {
        let text = cur.join("\n");
        if !text.trim().is_empty() {
            out.push((path.to_string(), text));
        }
        cur.clear();
    };
    for line in body.lines() {
        if is_fence(line) {
            in_fence = !in_fence;
        } else if !in_fence && let Some((level, text)) = heading_of(line) {
            flush(&path, &mut cur, &mut out);
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            stack.push((level, text));
            path = stack
                .iter()
                .map(|(_, t)| t.as_str())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(PATH_SEP);
            continue;
        }
        cur.push(line);
    }
    flush(&path, &mut cur, &mut out);
    out
}

/// Paragraphs: runs of lines split at blank lines, except inside code fences.
fn paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if is_fence(line) {
            in_fence = !in_fence;
        }
        if !in_fence && line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(cur.join("\n"));
                cur.clear();
            }
        } else {
            cur.push(line);
        }
    }
    if !cur.is_empty() {
        out.push(cur.join("\n"));
    }
    out
}

/// Sentence-like segments: split after `. ! ?` followed by white space, after CJK sentence ends
/// and after line breaks. Segments keep their trailing white space, so joining restores the text.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        let end = match c {
            '\n' | '。' | '！' | '？' => true,
            '.' | '!' | '?' => chars.peek().is_some_and(|n| n.is_whitespace()),
            _ => false,
        };
        if end {
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                if let Some(w) = chars.next() {
                    cur.push(w);
                }
            }
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Cuts text into runs of at most [`MAX_TOKENS`] anywhere (last resort).
fn hard_cut(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cost = 0;
    for c in text.chars() {
        let q = quarter_cost(c);
        if cost + q > MAX_TOKENS * 4 {
            out.push(std::mem::take(&mut cur));
            cost = 0;
        }
        cur.push(c);
        cost += q;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Units of at most [`MAX_TOKENS`]: paragraphs, else sentences, else hard cuts.
fn units(text: &str) -> Vec<Unit> {
    let mut out = Vec::new();
    for para in paragraphs(text) {
        if estimate_tokens(&para) <= MAX_TOKENS {
            out.push(Unit {
                text: para,
                sep: "\n\n",
            });
            continue;
        }
        let mut first = true;
        for s in sentences(&para) {
            let cuts = if estimate_tokens(&s) <= MAX_TOKENS {
                vec![s]
            } else {
                hard_cut(&s)
            };
            for c in cuts {
                out.push(Unit {
                    text: c,
                    sep: if first { "\n\n" } else { "" },
                });
                first = false;
            }
        }
    }
    out
}

/// Joins units into pieces of up to `target` tokens (a single unit may be longer, up to MAX).
fn pack(units: Vec<Unit>, target: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for u in units {
        if cur.is_empty() {
            cur = u.text;
            continue;
        }
        let joined = format!("{cur}{}{}", u.sep, u.text);
        if estimate_tokens(&joined) <= target {
            cur = joined;
        } else {
            out.push(std::mem::replace(&mut cur, u.text));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Code fence marker lines are dropped; the code itself stays as plain text.
fn push_clean(out: &mut Vec<Piece>, heading: &str, text: &str) {
    let kept: Vec<&str> = text.lines().filter(|l| !is_fence(l)).collect();
    let text = kept.join("\n");
    let text = text.trim();
    if !text.is_empty() {
        out.push(Piece {
            heading: heading.to_string(),
            text: text.to_string(),
        });
    }
}

/// Joins short neighbours up to [`TARGET_TOKENS`]. A section that joins another piece brings its
/// own (last) heading along as a line of text.
fn merge(parts: Vec<Piece>) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for p in parts {
        if let Some(last) = out.last_mut() {
            let add = if last.heading == p.heading || p.heading.is_empty() {
                format!("\n\n{}", p.text)
            } else {
                let name = p.heading.rsplit(PATH_SEP).next().unwrap_or("");
                format!("\n\n{name}\n{}", p.text)
            };
            if estimate_tokens(&last.text) + estimate_tokens(&add) <= TARGET_TOKENS {
                last.text.push_str(&add);
                continue;
            }
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(n: usize, seed: usize) -> String {
        (0..n)
            .map(|i| format!("word{}", (i * 7 + seed) % 97))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn token_estimate() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        assert_eq!(estimate_tokens("液压软管检查"), 6);
        assert_eq!(estimate_tokens("液压 hose"), 4);
    }

    #[test]
    fn empty_page_has_no_pieces() {
        assert!(split_page("").is_empty());
        assert!(split_page("\n\n  \n").is_empty());
        assert!(split_page("# Only a heading").is_empty());
    }

    #[test]
    fn heading_paths_and_merge() {
        let body = format!(
            "Intro text\n\n# Pre-start\n\n{}\n\n## Hoses\n\n{}\n\n## Pumps\n\n{}\n\n# Shutdown\n\n{}",
            words(200, 1),
            words(200, 2),
            words(200, 3),
            words(10, 4)
        );
        let p = split_page(&body);
        assert!(p.len() >= 3, "{p:?}");
        assert_eq!(p[0].heading, "");
        assert!(p[0].text.starts_with("Intro text"));
        assert!(p.iter().any(|x| x.heading == "Pre-start › Hoses"));
        assert!(p.iter().any(|x| x.heading == "Pre-start › Pumps"));
        assert!(p.iter().all(|x| estimate_tokens(&x.text) <= MAX_TOKENS));
    }

    #[test]
    fn short_sections_are_merged() {
        let p = split_page("# A\n\nalpha\n\n# B\n\nbeta\n\n## C\n\ngamma");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].heading, "A");
        assert!(p[0].text.contains("alpha"));
        assert!(p[0].text.contains("B\nbeta"));
        assert!(p[0].text.contains("C\ngamma"));
    }

    #[test]
    fn heading_level_pops_path() {
        let long = words(200, 0);
        let body = format!("# A\n\n## B\n\n{long}\n\n# C\n\n{long}\n\n### D\n\n{long}");
        let p = split_page(&body);
        let heads: Vec<&str> = p.iter().map(|x| x.heading.as_str()).collect();
        assert_eq!(heads, ["A › B", "C", "C › D"]);
    }

    #[test]
    fn long_section_splits_at_paragraphs() {
        let paras: Vec<String> = (0..12).map(|i| words(60, i)).collect();
        let body = format!("# Big\n\n{}", paras.join("\n\n"));
        let p = split_page(&body);
        assert!(p.len() >= 2);
        for x in &p {
            assert_eq!(x.heading, "Big");
            assert!(estimate_tokens(&x.text) <= MAX_TOKENS);
            // Every piece is made of whole paragraphs.
            for para in x.text.split("\n\n") {
                assert!(paras.iter().any(|q| q == para), "cut inside a paragraph");
            }
        }
        let joined: Vec<&str> = p.iter().flat_map(|x| x.text.split("\n\n")).collect();
        assert_eq!(joined.len(), 12);
    }

    #[test]
    fn long_paragraph_splits_at_sentences() {
        let sentence = format!("{}. ", words(40, 5));
        let body = sentence.repeat(30);
        let p = split_page(&body);
        assert!(p.len() >= 2);
        for x in &p {
            assert!(estimate_tokens(&x.text) <= MAX_TOKENS);
            assert!(x.text.ends_with('.'), "cut mid-sentence: {}", x.text);
        }
    }

    #[test]
    fn unbroken_text_is_hard_cut() {
        let body = "x".repeat(MAX_TOKENS * 4 * 3 + 17);
        let p = split_page(&body);
        assert_eq!(p.len(), 4);
        assert!(p.iter().all(|x| estimate_tokens(&x.text) <= MAX_TOKENS));
        let total: usize = p.iter().map(|x| x.text.len()).sum();
        assert_eq!(total, body.len());
    }

    #[test]
    fn code_fence_stays_whole_and_hides_headings() {
        let code = (0..40)
            .map(|i| format!("line_{i}();"))
            .collect::<Vec<_>>()
            .join("\n\n# not a heading\n");
        let body = format!(
            "# Code\n\n{}\n\n```rust\n{code}\n```\n\n{}",
            words(250, 1),
            words(50, 2)
        );
        assert!(estimate_tokens(&code) < MAX_TOKENS);
        let p = split_page(&body);
        // The fence is in exactly one piece, intact, and marker lines are gone.
        let with_code: Vec<_> = p.iter().filter(|x| x.text.contains("line_0();")).collect();
        assert_eq!(with_code.len(), 1);
        assert!(with_code[0].text.contains("line_39();"));
        assert!(with_code[0].text.contains("# not a heading"));
        assert!(p.iter().all(|x| !x.text.contains("```")));
        assert!(p.iter().all(|x| x.heading == "Code"));
    }

    #[test]
    fn huge_code_fence_is_split_within_max() {
        let code = (0..600)
            .map(|i| format!("let value_{i} = compute({i});"))
            .collect::<Vec<_>>()
            .join("\n");
        let p = split_page(&format!("```\n{code}\n```"));
        assert!(p.len() >= 2);
        assert!(p.iter().all(|x| estimate_tokens(&x.text) <= MAX_TOKENS));
    }

    #[test]
    fn chinese_text() {
        let sentence = "液压软管必须在每次启动前检查是否有磨损和泄漏。";
        let p = split_page(&format!("# 检查\n\n{}", sentence.repeat(60)));
        assert!(p.len() >= 3);
        assert!(p.iter().all(|x| x.heading == "检查"));
        assert!(p.iter().all(|x| estimate_tokens(&x.text) <= MAX_TOKENS));
        assert!(p.iter().all(|x| x.text.ends_with('。')));
    }

    #[test]
    fn mentions_images_and_attachments_are_cleaned() {
        let p = split_page(
            "See [Hose guide](page:abc-123) and ![photo](attachment:x1) or [the PDF](attachment:y2) and [web](https://e.com).",
        );
        assert_eq!(p.len(), 1);
        assert_eq!(
            p[0].text,
            "See Hose guide and  or the PDF and [web](https://e.com)."
        );
    }

    #[test]
    fn task_pieces() {
        let p = split_task(
            "PRX-1",
            "Fix hose",
            "todo",
            "Replace the [pump](page:p1).",
            &[],
        );
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text, "PRX-1: Fix hose (todo)\n\nReplace the pump.");

        let p = split_task("PRX-2", "Empty", "done", "", &[]);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text, "PRX-2: Empty (done)");

        let comments = vec![
            "first comment".to_string(),
            "second comment".to_string(),
            "   ".to_string(),
        ];
        let p = split_task("PRX-3", "T", "todo", "d", &comments);
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].text, "first comment\n\nsecond comment");

        let long: Vec<String> = (0..6).map(|i| words(150, i)).collect();
        let p = split_task("PRX-4", "T", "todo", &words(900, 9), &long);
        assert!(p.len() >= 5);
        assert!(p.iter().all(|x| estimate_tokens(&x.text) <= MAX_TOKENS));
        assert!(p[0].text.starts_with("PRX-4: T (todo)"));
        // Comments are oldest first and never share a piece with the description.
        let last = &p[p.len() - 1];
        assert!(last.text.contains(&long[5]));
        assert!(!p[0].text.contains(&long[0]));
    }

    #[test]
    fn max_is_never_exceeded_on_random_input() {
        let mut state: u64 = 0x2545F4914F6CDD1D;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let atoms = [
            "word ",
            "longerwordhere ",
            "。",
            "液压",
            ". ",
            "\n",
            "\n\n",
            "# H\n",
            "## Sub\n",
            "```\n",
            "~~~\n",
            "[a](page:1)",
            "![i](x)",
            "x",
            "? ",
            "!",
            "    ",
            "中文",
        ];
        for round in 0..60 {
            let mut body = String::new();
            let n = (next() % 3000) as usize;
            for _ in 0..n {
                body.push_str(atoms[(next() % atoms.len() as u64) as usize]);
            }
            for x in split_page(&body) {
                assert!(
                    estimate_tokens(&x.text) <= MAX_TOKENS,
                    "round {round}: {} tokens",
                    estimate_tokens(&x.text)
                );
                assert!(!x.text.is_empty());
            }
            let comments: Vec<String> = vec![body.clone(), body.chars().take(500).collect()];
            for x in split_task("K-1", "t", "todo", &body, &comments) {
                assert!(estimate_tokens(&x.text) <= MAX_TOKENS);
            }
        }
    }
}
