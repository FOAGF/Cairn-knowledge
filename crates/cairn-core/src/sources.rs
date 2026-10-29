//! Fitting a file's text into the model (FR-KN-UP-4a, D-41): the whole text when it fits;
//! otherwise excerpts picked by meaning for the instruction, or the text in parts that are
//! drafted one by one and then combined (`draft-part`, `draft-combine`).
//!
//! Token counts are estimates ([`crate::chunking::estimate_tokens`]); the budgets leave room for
//! that.

use crate::chunking::estimate_tokens;
use crate::prompts::EXCERPT_GAP;

/// Tokens kept free for the prompt's own text and the reply.
pub const RESERVED_TOKENS: usize = 6_000;

/// Smallest budget [`budget`] returns, however small the context size is.
const MIN_BUDGET: usize = 1_000;

/// Pieces are cut to about this size (estimated tokens), and never above [`MAX_PIECE_TOKENS`].
const PIECE_TOKENS: usize = 350;
const MAX_PIECE_TOKENS: usize = 500;

/// How the source goes to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourcePlan {
    /// Send it all (it fits, or the size is unknown and the model hasn't refused yet).
    Whole,
    /// Too long, with an instruction: send excerpts picked by meaning ([`pieces`], [`excerpts`]).
    Excerpts,
    /// Too long, no instruction: draft these parts in order, then combine them.
    Parts(Vec<String>),
}

/// Tokens available for the source: the model's context size (the *Context size* setting, else
/// LiteLLM's known size) minus [`RESERVED_TOKENS`]; `None` when the size is unknown.
pub fn budget(context_size: Option<u32>) -> Option<usize> {
    context_size.map(|n| (n as usize).saturating_sub(RESERVED_TOKENS).max(MIN_BUDGET))
}

/// The plan for `text`. With `budget: None` the plan is `Whole`; the caller falls back to
/// `plan(text, Some(smaller), …)` when the model answers `too_long`.
pub fn plan(text: &str, budget: Option<usize>, has_instruction: bool) -> SourcePlan {
    let Some(budget) = budget else {
        return SourcePlan::Whole;
    };
    if estimate_tokens(text) <= budget {
        SourcePlan::Whole
    } else if has_instruction {
        SourcePlan::Excerpts
    } else {
        SourcePlan::Parts(parts(text, budget))
    }
}

/// The text in order as parts of at most `budget` tokens (a piece is never cut again), joined
/// back with blank lines.
fn parts(text: &str, budget: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut used = 0;
    for piece in pieces(text) {
        let n = estimate_tokens(&piece);
        if !current.is_empty() && used + n > budget {
            parts.push(current.join("\n\n"));
            current.clear();
            used = 0;
        }
        used += n;
        current.push(piece);
    }
    if !current.is_empty() {
        parts.push(current.join("\n\n"));
    }
    parts
}

fn is_heading(block: &str) -> bool {
    block.trim_start().starts_with('#')
}

/// Cuts `text` into chunks of at most `max` tokens: at line ends, and inside a longer line at
/// the last character that fits.
fn cut_long(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut used = 0;
    for line in text.split_inclusive('\n') {
        let n = estimate_tokens(line);
        if n > max {
            if !current.trim().is_empty() {
                out.push(current.trim_end().to_string());
            }
            current.clear();
            for c in line.chars() {
                current.push(c);
                if estimate_tokens(&current) >= max {
                    out.push(std::mem::take(&mut current));
                }
            }
            used = estimate_tokens(&current);
            continue;
        }
        if used + n > max {
            if !current.trim().is_empty() {
                out.push(current.trim_end().to_string());
            }
            current.clear();
            used = 0;
        }
        current.push_str(line);
        used += n;
    }
    if !current.trim().is_empty() {
        out.push(current.trim_end().to_string());
    }
    out
}

/// The text cut into pieces for picking excerpts: at headings and paragraph breaks, each well
/// under the budget, in source order. Nothing is dropped or rewritten, so the pieces joined with
/// blank lines are the text (runs of blank lines aside).
pub fn pieces(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut used = 0;
    let text = text.replace("\r\n", "\n");
    for block in text
        .split("\n\n")
        .map(|b| b.trim_matches('\n'))
        .filter(|b| !b.trim().is_empty())
    {
        let n = estimate_tokens(block);
        // A heading starts a new piece (unless the piece is only a heading so far), so a
        // section's text stays with its heading.
        if !current.is_empty()
            && ((is_heading(block) && used > PIECE_TOKENS / 4) || used + n > PIECE_TOKENS)
        {
            out.push(std::mem::take(&mut current));
            used = 0;
        }
        if n > MAX_PIECE_TOKENS {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
                used = 0;
            }
            out.extend(cut_long(block, PIECE_TOKENS));
            continue;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block);
        used += n;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The best pieces by `scores` (same order as `pieces`) that fit in `budget`, put back in source
/// order and joined with a line holding [`crate::prompts::EXCERPT_GAP`] where text was skipped.
pub fn excerpts(pieces: &[String], scores: &[f32], budget: usize) -> String {
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by(|&a, &b| {
        let score = |i: usize| scores.get(i).copied().unwrap_or(0.0);
        score(b).total_cmp(&score(a)).then(a.cmp(&b))
    });
    let mut picked: Vec<usize> = Vec::new();
    let mut used = 0;
    for i in order {
        let n = estimate_tokens(&pieces[i]);
        if used + n <= budget {
            used += n;
            picked.push(i);
        }
    }
    picked.sort_unstable();
    let mut out: Vec<&str> = Vec::new();
    let mut next = 0;
    for &i in &picked {
        if i > next {
            out.push(EXCERPT_GAP);
        }
        out.push(&pieces[i]);
        next = i + 1;
    }
    if !picked.is_empty() && next < pieces.len() {
        out.push(EXCERPT_GAP);
    }
    out.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` sections of about 400 tokens each.
    fn long_text(n: usize) -> String {
        (0..n)
            .map(|i| format!("## Section {i}\n\n{}", format!("word{i} ").repeat(320)))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    #[test]
    fn the_budget_leaves_room_for_the_prompt_and_reply() {
        assert_eq!(budget(None), None);
        assert_eq!(budget(Some(128_000)), Some(128_000 - RESERVED_TOKENS));
        assert_eq!(budget(Some(2_000)), Some(MIN_BUDGET));
    }

    #[test]
    fn a_text_that_fits_or_has_no_known_limit_goes_whole() {
        let text = long_text(3);
        assert_eq!(plan(&text, None, false), SourcePlan::Whole);
        assert_eq!(plan(&text, Some(100_000), true), SourcePlan::Whole);
        assert_eq!(plan("short", Some(MIN_BUDGET), false), SourcePlan::Whole);
    }

    #[test]
    fn too_long_with_an_instruction_means_excerpts() {
        assert_eq!(
            plan(&long_text(10), Some(1_500), true),
            SourcePlan::Excerpts
        );
    }

    #[test]
    fn too_long_without_an_instruction_means_parts_in_order_that_fit() {
        let text = long_text(10);
        let SourcePlan::Parts(parts) = plan(&text, Some(1_500), false) else {
            panic!("expected parts");
        };
        assert!(parts.len() >= 3, "{}", parts.len());
        for p in &parts {
            assert!(estimate_tokens(p) <= 1_500, "{}", estimate_tokens(p));
        }
        // Every section is there once, in order.
        let joined = parts.join("\n\n");
        let mut at = 0;
        for i in 0..10 {
            let found = joined[at..]
                .find(&format!("## Section {i}\n"))
                .expect("section missing");
            at += found + 1;
        }
        for i in 0..10 {
            assert_eq!(joined.matches(&format!("word{i} ")).count(), 320);
        }
    }

    #[test]
    fn pieces_keep_all_text_and_start_at_headings() {
        let text = long_text(4);
        let ps = pieces(&text);
        assert!(ps.len() >= 4);
        assert!(ps.iter().all(|p| estimate_tokens(p) <= MAX_PIECE_TOKENS));
        assert!(ps[0].starts_with("## Section 0"));
        let words = |s: &str| s.split_whitespace().count();
        assert_eq!(ps.iter().map(|p| words(p)).sum::<usize>(), words(&text));
        assert!(pieces("").is_empty());
        assert!(pieces("  \n\n \n").is_empty());
    }

    #[test]
    fn one_huge_paragraph_is_cut_below_the_limit() {
        let one_line = "x".repeat(10_000);
        let ps = pieces(&one_line);
        assert!(ps.len() >= 5);
        assert!(ps.iter().all(|p| estimate_tokens(p) <= MAX_PIECE_TOKENS));
        assert_eq!(ps.concat().len(), 10_000);
        // Chinese: about one token per character.
        let han = "字".repeat(3_000);
        let ps = pieces(&han);
        assert!(ps.iter().all(|p| estimate_tokens(p) <= MAX_PIECE_TOKENS));
        assert_eq!(ps.concat().chars().count(), 3_000);
        // A table without blank lines is cut at line ends.
        let table = (0..400)
            .map(|i| format!("| row {i} | {} |", "cell ".repeat(10)))
            .collect::<Vec<_>>()
            .join("\n");
        let ps = pieces(&table);
        assert!(ps.len() > 1);
        assert!(
            ps.iter()
                .all(|p| p.lines().all(|l| l.starts_with("| row ")))
        );
    }

    #[test]
    fn excerpts_pick_the_best_pieces_in_source_order_with_gaps() {
        let ps: Vec<String> = (0..6)
            .map(|i| format!("piece {i} {}", "w ".repeat(100)))
            .collect();
        let each = estimate_tokens(&ps[0]);
        let scores = [0.1, 0.9, 0.2, 0.8, 0.1, 0.3];
        let out = excerpts(&ps, &scores, each * 2 + 5);
        assert_eq!(
            out,
            format!(
                "{EXCERPT_GAP}\n\n{}\n\n{EXCERPT_GAP}\n\n{}\n\n{EXCERPT_GAP}",
                ps[1], ps[3]
            )
        );
        // Everything fits: no gap lines.
        let out = excerpts(&ps, &scores, each * 100);
        assert_eq!(out, ps.join("\n\n"));
        // Neighbours at the start: only a trailing gap.
        let scores = [0.9, 0.8, 0.0, 0.0, 0.0, 0.0];
        let out = excerpts(&ps, &scores, each * 2 + 5);
        assert_eq!(out, format!("{}\n\n{}\n\n{EXCERPT_GAP}", ps[0], ps[1]));
        // Nothing fits: empty rather than a lone gap.
        assert_eq!(excerpts(&ps, &scores, 1), "");
    }
}
