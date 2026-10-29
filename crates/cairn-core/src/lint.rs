//! Spelling and grammar checks with native Harper (FR-KN-ED-6..8). Owned by the M3 search agent.
//!
//! Harper is loaded on first use and stays loaded (about 130 MB, measured in M0). Before linting,
//! mentions (`#KEY`, `[..](page:..)`), image links, URLs, code and LaTeX formulas are masked so they aren't flagged;
//! Chinese text is skipped, not flagged. Words in the personal dictionary (settings) are accepted.
//! Offsets are **UTF-16 code units** so the editor (JavaScript strings) can use them directly.
//!
//! Masking replaces characters with U+FFFC (object replacement character), one for one, so Harper's
//! character offsets still line up with the original text; Harper treats it as unlintable. Code
//! spans and fenced blocks are skipped by Harper's own Markdown parser. As a second safety net,
//! any lint whose span touches a masked character is dropped.
//!
//! Locking: the linter needs `&mut`, so calls are serialised on a global mutex (the document is
//! parsed outside it). `lint` needs no database, so callers should read the dictionary under the DB
//! lock and call `lint` outside it, on a blocking thread (see `commands/search.rs`).

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use harper_core::linting::{LintGroup, LintKind, Linter, Suggestion};
use harper_core::spell::FstDictionary;
use harper_core::{Dialect, Document};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::Result;

/// The English variant Harper checks against. A later setting may change it.
const DIALECT: Dialect = Dialect::American;
const MAX_SUGGESTIONS: usize = 5;
/// Placeholder for masked characters; Harper ignores it.
const MASK: char = '\u{FFFC}';

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LintCategory {
    Spelling,
    Grammar,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LintOptions {
    pub spelling: bool,
    pub grammar: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LintHit {
    /// UTF-16 offset of the first character.
    pub start: u32,
    /// UTF-16 offset after the last character.
    pub end: u32,
    pub category: LintCategory,
    pub message: String,
    /// Replacement texts, best first (may be empty).
    pub suggestions: Vec<String>,
}

struct Engine {
    dict: Arc<FstDictionary>,
    linter: Mutex<LintGroup>,
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let dict = FstDictionary::curated();
        let linter = LintGroup::new_curated(dict.clone(), DIALECT);
        Engine {
            dict,
            linter: Mutex::new(linter),
        }
    })
}

fn category(kind: LintKind) -> Option<LintCategory> {
    match kind {
        LintKind::Spelling | LintKind::Typo => Some(LintCategory::Spelling),
        // Style advice is too noisy for notes.
        LintKind::Readability | LintKind::Style | LintKind::Enhancement | LintKind::Regionalism => {
            None
        }
        _ => Some(LintCategory::Grammar),
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF | 0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x323AF)
}

/// Position of `seq` at or after `from`.
fn find_seq(chars: &[char], from: usize, seq: &[char]) -> Option<usize> {
    (from..chars.len()).find(|&k| chars[k..].starts_with(seq))
}

/// End (exclusive) of an inline `$…$` formula opening at `i` (D-63): the opening `$` is followed by a
/// non-space; the next single, unescaped `$` on the same line closes it if it follows a non-space and
/// is not followed by a digit. Otherwise this `$` opens nothing.
fn inline_formula_end(chars: &[char], i: usize) -> Option<usize> {
    let next = *chars.get(i + 1)?;
    if next.is_whitespace() || next == '$' {
        return None;
    }
    let mut k = i + 1;
    while k < chars.len() && chars[k] != '\n' {
        if chars[k] == '$' {
            if chars.get(k + 1) == Some(&'$') {
                // A `$$` run is not a closer.
                while chars.get(k + 1) == Some(&'$') {
                    k += 1;
                }
            } else if chars[k - 1] != '\\' {
                let ok = !chars[k - 1].is_whitespace() && !chars.get(k + 1).is_some_and(|c| c.is_ascii_digit());
                return ok.then_some(k + 1);
            }
        }
        k += 1;
    }
    None
}

/// Masks what must not be checked. Returns the masked characters and a flag per character.
fn mask(text: &str) -> (Vec<char>, Vec<bool>) {
    let mut chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut masked = vec![false; n];
    let starts_with = |chars: &[char], i: usize, s: &str| {
        s.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if is_cjk(c) {
            ranges.push((i, i + 1));
            i += 1;
        } else if c == '#' && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
            // #KEY-12
            let mut j = i + 1;
            while j < n && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j > i + 1 && j < n && chars[j] == '-' {
                let mut k = j + 1;
                while k < n && chars[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 {
                    ranges.push((i, k));
                    i = k;
                    continue;
                }
            }
            i += 1;
        } else if c == '!' && chars.get(i + 1) == Some(&'[') {
            // ![alt](target): the whole image link.
            match find_link_end(&chars, i + 1) {
                Some(end) => {
                    ranges.push((i, end));
                    i = end;
                }
                None => i += 1,
            }
        } else if c == ']' && chars.get(i + 1) == Some(&'(') {
            // [text](target): keep the text checkable, mask the target.
            match chars[i + 2..].iter().position(|&c| c == ')' || c == '\n') {
                Some(p) if chars[i + 2 + p] == ')' => {
                    ranges.push((i + 2, i + 2 + p));
                    i += 2 + p;
                }
                _ => i += 1,
            }
        } else if c == '$' && chars.get(i + 1) == Some(&'$') {
            // $$…$$: a formula (D-63), on one line or over several.
            match find_seq(&chars, i + 2, &['$', '$']) {
                Some(j) => {
                    ranges.push((i, j + 2));
                    i = j + 2;
                }
                None => i += 2,
            }
        } else if c == '\\' && matches!(chars.get(i + 1), Some('(') | Some('[')) {
            // \(…\) and \[…\]: LaTeX's own formula delimiters.
            let close = if chars[i + 1] == '(' { ['\\', ')'] } else { ['\\', ']'] };
            match find_seq(&chars, i + 2, &close) {
                Some(j) => {
                    ranges.push((i, j + 2));
                    i = j + 2;
                }
                None => i += 2,
            }
        } else if c == '$' && (i == 0 || chars[i - 1] != '\\') {
            // $…$ by Pandoc's rule, as app/src/lib/math.ts renders it (amounts like "$5 and $10" stay text).
            match inline_formula_end(&chars, i) {
                Some(end) => {
                    ranges.push((i, end));
                    i = end;
                }
                None => i += 1,
            }
        } else if starts_with(&chars, i, "http://")
            || starts_with(&chars, i, "https://")
            || starts_with(&chars, i, "www.")
        {
            let mut j = i;
            while j < n && !chars[j].is_whitespace() && !matches!(chars[j], ')' | '>' | '<') {
                j += 1;
            }
            ranges.push((i, j));
            i = j;
        } else {
            i += 1;
        }
    }
    for (from, to) in ranges {
        for k in from..to.min(n) {
            chars[k] = MASK;
            masked[k] = true;
        }
    }
    (chars, masked)
}

/// Given `start` at the `[` of `[alt](target)`, the index after the closing `)`.
fn find_link_end(chars: &[char], start: usize) -> Option<usize> {
    let close = (start..chars.len()).find(|&k| chars[k] == ']' || chars[k] == '\n')?;
    if chars[close] != ']' || chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let end = (close + 2..chars.len()).find(|&k| chars[k] == ')' || chars[k] == '\n')?;
    (chars[end] == ')').then_some(end + 1)
}

/// Checks Markdown `text`. `dictionary` is the personal dictionary.
pub fn lint(text: &str, options: LintOptions, dictionary: &[String]) -> Result<Vec<LintHit>> {
    if !(options.spelling || options.grammar) || text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let (chars, masked) = mask(text);
    let masked_text: String = chars.iter().collect();
    let eng = engine();
    let doc = Document::new_markdown_default(&masked_text, &*eng.dict);
    let lints = eng
        .linter
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .lint(&doc);

    let known: HashSet<String> = dictionary
        .iter()
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    // UTF-16 offset of every character boundary.
    let original: Vec<char> = text.chars().collect();
    let mut utf16 = Vec::with_capacity(original.len() + 1);
    let mut acc = 0u32;
    for c in &original {
        utf16.push(acc);
        acc += c.len_utf16() as u32;
    }
    utf16.push(acc);

    let mut seen: HashSet<(usize, usize, bool)> = HashSet::new();
    let mut hits = Vec::new();
    for l in lints {
        let Some(cat) = category(l.lint_kind) else {
            continue;
        };
        let wanted = match cat {
            LintCategory::Spelling => options.spelling,
            LintCategory::Grammar => options.grammar,
        };
        let (s, e) = (l.span.start, l.span.end.min(original.len()));
        if !wanted || s >= e || masked[s..e].iter().any(|&m| m) {
            continue;
        }
        if cat == LintCategory::Spelling {
            let word: String = original[s..e].iter().collect();
            if known.contains(&word.to_lowercase()) {
                continue;
            }
        }
        if !seen.insert((s, e, cat == LintCategory::Spelling)) {
            continue;
        }
        let mut suggestions: Vec<String> = Vec::new();
        for sug in &l.suggestions {
            let s_text = match sug {
                Suggestion::ReplaceWith(cs) => cs.iter().collect::<String>(),
                Suggestion::InsertAfter(cs) => {
                    original[s..e].iter().chain(cs.iter()).collect::<String>()
                }
                Suggestion::Remove => String::new(),
            };
            if !suggestions.contains(&s_text) {
                suggestions.push(s_text);
            }
            if suggestions.len() == MAX_SUGGESTIONS {
                break;
            }
        }
        hits.push(LintHit {
            start: utf16[s],
            end: utf16[e],
            category: cat,
            message: l.message,
            suggestions,
        });
    }
    hits.sort_by_key(|h| (h.start, h.end));
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: LintOptions = LintOptions {
        spelling: true,
        grammar: true,
    };

    fn lint_ok(text: &str) -> Vec<LintHit> {
        lint(text, BOTH, &[]).unwrap()
    }

    fn slice(text: &str, h: &LintHit) -> String {
        let u: Vec<u16> = text.encode_utf16().collect();
        String::from_utf16(&u[h.start as usize..h.end as usize]).unwrap()
    }

    #[test]
    fn misspelling_with_suggestions() {
        let t = "This is a speling mistake.";
        let hits = lint_ok(t);
        let h = hits.iter().find(|h| slice(t, h) == "speling").unwrap();
        assert_eq!(h.category, LintCategory::Spelling);
        assert!(h.suggestions.contains(&"spelling".to_string()));
        assert!(h.suggestions.len() <= 5);
    }

    #[test]
    fn repeated_word_and_article() {
        let t = "The the cat sat down.";
        let hits = lint_ok(t);
        assert!(hits.iter().any(|h| slice(t, h).to_lowercase() == "the the"));
        let t2 = "This is an test.";
        let hits = lint_ok(t2);
        let h = hits.iter().find(|h| slice(t2, h) == "an").unwrap();
        assert_eq!(h.category, LintCategory::Grammar);
        assert!(h.suggestions.contains(&"a".to_string()));
    }

    #[test]
    fn masking_keeps_mentions_urls_code_and_chinese_quiet() {
        let t = "See #DPL-3 and [Hybrid search](page:0192abcd) at https://example.com/aa_bb \
                 ![diagram](attachment:0192xyz) `speling` 中文内容不应该被标记。\n\n\
                 ```\nlet x = speling;\n```\n";
        assert_eq!(lint_ok(t), vec![]);
        // link text is still checked, its target is not
        let t = "See [Hybrid serch](page:0192abcd).";
        let hits = lint_ok(t);
        assert_eq!(hits.len(), 1);
        assert_eq!(slice(t, &hits[0]), "serch");
        // Chinese next to English words
        assert_eq!(lint_ok("中文Hello English words 混合 text。"), vec![]);
    }

    #[test]
    fn formulas_are_not_checked() {
        assert_eq!(lint_ok("The ratio $\\frac{speling}{x}$ is fine."), vec![]);
        assert_eq!(lint_ok("Before:\n\n$$\n\\alpha + speling\n$$\n\nAfter."), vec![]);
        assert_eq!(lint_ok("Use \\(speling\\) and \\[ speling \\] here."), vec![]);
        // Amounts are text, so what follows them is still checked.
        let t = "It costs $5 and $10, speling included.";
        let hits = lint_ok(t);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(slice(t, &hits[0]), "speling");
    }

    #[test]
    fn personal_dictionary() {
        let t = "We use Cairnify daily.";
        assert!(lint_ok(t).iter().any(|h| slice(t, h) == "Cairnify"));
        let hits = lint(t, BOTH, &["cairnify".to_string()]).unwrap();
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn options_off() {
        let t = "This is an test with speling.";
        assert!(lint(t, LintOptions::default(), &[]).unwrap().is_empty());
        let sp = lint(
            t,
            LintOptions {
                spelling: true,
                grammar: false,
            },
            &[],
        )
        .unwrap();
        assert!(sp.iter().all(|h| h.category == LintCategory::Spelling) && !sp.is_empty());
        let gr = lint(
            t,
            LintOptions {
                spelling: false,
                grammar: true,
            },
            &[],
        )
        .unwrap();
        assert!(gr.iter().all(|h| h.category == LintCategory::Grammar) && !gr.is_empty());
    }

    #[test]
    fn utf16_offsets() {
        let t = "😀 中文 😀 a speling here";
        let hits = lint_ok(t);
        let h = hits.iter().find(|h| slice(t, h) == "speling").unwrap();
        // two emoji (2 units each), two Han (1 each), spaces and "a "
        assert_eq!(h.start, 2 + 1 + 2 + 1 + 2 + 1 + 1 + 1);
    }

    #[test]
    fn concurrent_calls() {
        let hs: Vec<_> = (0..4)
            .map(|_| std::thread::spawn(|| lint_ok("A speling error.").len()))
            .collect();
        for h in hs {
            assert_eq!(h.join().unwrap(), 1);
        }
    }
}
