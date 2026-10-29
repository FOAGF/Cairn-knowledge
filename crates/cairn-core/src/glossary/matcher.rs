//! The matcher behind the glossary (FR-KN-GL-4): finds the names of a set of entries in a text by
//! the rules in the parent module's documentation.
//!
//! Approach: two Aho-Corasick automata find every raw occurrence of every name in one pass over
//! the text (one case-sensitive automaton for names that are all capitals, one ASCII
//! case-insensitive for the rest). Each raw occurrence is then checked against the word-boundary
//! and suffix rules, and finally the leftmost (then longest) valid occurrences win.
//! White space is first collapsed (one space per run), so a space inside a name matches any run.

use std::collections::HashSet;

use aho_corasick::{AhoCorasick, MatchKind};

use crate::search::is_han;

/// One match: which name-set entry it is (the index given to [`Matcher::new`]) and the byte range
/// in the original text, including a plural/code suffix ("MMUs", "MMU-07").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Found {
    pub entry: usize,
    pub start: usize,
    pub end: usize,
}

/// Facts about one name that the checks need.
struct NameInfo {
    entry: usize,
    /// Contains Han characters: matches anywhere, no boundary or suffix rules.
    han: bool,
    /// Starts / ends with a word character, so a boundary applies there.
    word_start: bool,
    word_end: bool,
}

struct Set {
    automaton: AhoCorasick,
    names: Vec<NameInfo>,
}

pub struct Matcher {
    /// Names of two or more capitals and no lowercase: matched case-sensitively.
    exact: Option<Set>,
    /// All other names: ASCII case-insensitive.
    folded: Option<Set>,
}

/// A character that makes a match "inside a word": Latin letters, digits and `_`.
/// (Chinese characters are not word characters, so "用RAG检索" matches.)
fn is_word(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric() || is_latin_letter(c)
}

fn is_latin_letter(c: char) -> bool {
    matches!(c as u32, 0xC0..=0x24F) && c != '\u{D7}' && c != '\u{F7}'
}

/// Letters and digits only (what may follow a "-" in a code such as "MMU-07").
fn is_code_char(c: char) -> bool {
    is_word(c) && c != '_'
}

/// Two or more capitals and no lowercase letter ("AN", "MMU", "NSW RFS").
fn is_all_caps(name: &str) -> bool {
    name.chars().filter(|c| c.is_uppercase()).count() >= 2
        && !name.chars().any(|c| c.is_lowercase())
}

/// Every run of white space becomes one space; the ends are trimmed.
fn collapse(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text with white space runs collapsed, and for each byte of it where it came from.
struct Normal {
    text: String,
    /// `starts[i]`: byte offset in the original where the character producing byte `i` starts.
    starts: Vec<usize>,
    /// `ends[i]`: where the character (or white space run) producing byte `i` ends.
    ends: Vec<usize>,
}

fn needs_collapse(text: &str) -> bool {
    let mut prev_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if c != ' ' || prev_space {
                return true;
            }
            prev_space = true;
        } else {
            prev_space = false;
        }
    }
    false
}

fn normalise(text: &str) -> Normal {
    let mut out = Normal {
        text: String::with_capacity(text.len()),
        starts: Vec::with_capacity(text.len()),
        ends: Vec::with_capacity(text.len()),
    };
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = chars.peek() {
                if !d.is_whitespace() {
                    break;
                }
                end = j + d.len_utf8();
                chars.next();
            }
            out.text.push(' ');
            out.starts.push(i);
            out.ends.push(end);
        } else {
            let end = i + c.len_utf8();
            out.text.push(c);
            for _ in 0..c.len_utf8() {
                out.starts.push(i);
                out.ends.push(end);
            }
        }
    }
    out
}

fn build(patterns: Vec<(String, NameInfo)>, case_insensitive: bool) -> Option<Set> {
    if patterns.is_empty() {
        return None;
    }
    let (texts, names): (Vec<String>, Vec<NameInfo>) = patterns.into_iter().unzip();
    let automaton = AhoCorasick::builder()
        .ascii_case_insensitive(case_insensitive)
        // Standard semantics are needed for overlapping search: the leftmost-longest choice is
        // made below, after the boundary checks (a longer name may fail them while a shorter passes).
        .match_kind(MatchKind::Standard)
        .build(texts)
        .ok()?;
    Some(Set { automaton, names })
}

impl Matcher {
    /// `names` are `(entry index, name)` pairs. When several entries have the same name, the first
    /// one listed owns it.
    pub fn new(names: &[(usize, String)]) -> Matcher {
        let mut seen: HashSet<(String, bool)> = HashSet::new();
        let mut exact = Vec::new();
        let mut folded = Vec::new();
        for (entry, name) in names {
            let name = collapse(name);
            let (Some(first), Some(last)) = (name.chars().next(), name.chars().next_back()) else {
                continue;
            };
            let han = name.chars().any(is_han);
            let caps = is_all_caps(&name) && !han;
            let key = if caps {
                name.clone()
            } else {
                name.to_ascii_lowercase()
            };
            if !seen.insert((key, caps)) {
                continue;
            }
            let info = NameInfo {
                entry: *entry,
                han,
                word_start: is_word(first),
                word_end: is_word(last),
            };
            if caps {
                exact.push((name, info));
            } else {
                folded.push((name, info));
            }
        }
        Matcher {
            exact: build(exact, false),
            folded: build(folded, true),
        }
    }

    /// Every match in `text`, in text order, no two overlapping (leftmost wins, then longest).
    pub fn find(&self, text: &str) -> Vec<Found> {
        let normal = needs_collapse(text).then(|| normalise(text));
        let haystack = normal.as_ref().map_or(text, |n| n.text.as_str());

        let mut candidates: Vec<Found> = Vec::new();
        for set in [&self.exact, &self.folded].into_iter().flatten() {
            for m in set.automaton.find_overlapping_iter(haystack) {
                let info = &set.names[m.pattern().as_usize()];
                if let Some(end) = check(haystack, m.start(), m.end(), info) {
                    candidates.push(Found {
                        entry: info.entry,
                        start: m.start(),
                        end,
                    });
                }
            }
        }
        // Leftmost first, the longest of those first; then take what doesn't overlap.
        candidates.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
        let mut out: Vec<Found> = Vec::new();
        let mut taken_to = 0;
        for c in candidates {
            if out.is_empty() || c.start >= taken_to {
                taken_to = c.end;
                out.push(c);
            }
        }
        if let Some(n) = &normal {
            for f in &mut out {
                f.start = n.starts[f.start];
                f.end = n.ends[f.end - 1];
            }
        }
        out
    }
}

/// Applies the boundary and suffix rules to a raw occurrence; returns where the match really ends.
fn check(text: &str, start: usize, end: usize, info: &NameInfo) -> Option<usize> {
    if info.han {
        return Some(end);
    }
    if info.word_start && text[..start].chars().next_back().is_some_and(is_word) {
        return None;
    }
    if !info.word_end {
        return Some(end);
    }
    // After the name: an optional plural ("es" or "s"), then any "-code" runs, then a boundary.
    for plural in ["es", "s", ""] {
        let Some(head) = text[end..].get(..plural.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(plural) {
            continue;
        }
        let mut p = end + plural.len();
        loop {
            let tail = &text[p..];
            let mut it = tail.chars();
            if it.next() == Some('-') && it.next().is_some_and(is_code_char) {
                p += 1;
                p += tail[1..]
                    .chars()
                    .take_while(|&c| is_code_char(c))
                    .map(char::len_utf8)
                    .sum::<usize>();
            } else {
                break;
            }
        }
        if !text[p..].chars().next().is_some_and(is_word) {
            return Some(p);
        }
    }
    None
}
