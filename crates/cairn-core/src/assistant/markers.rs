//! The `[n]` citation markers of an answer (README "Local checks › ask-answer", FR-LLM-AS-1).

/// The length of a marker's inside when `after` (the text after a `[`) starts one: digits,
/// optionally more numbers after commas, then `]`. `Some(len)` is the offset of the `]`.
pub(crate) fn marker_len(after: &str) -> Option<usize> {
    let bytes = after.as_bytes();
    let mut i = 0;
    let mut digits = false;
    loop {
        match bytes.get(i) {
            Some(b) if b.is_ascii_digit() => {
                digits = true;
                i += 1;
            }
            Some(b']') if digits => return Some(i),
            Some(b',') if digits => {
                digits = false;
                i += 1;
                while bytes.get(i).is_some_and(|b| *b == b' ' || *b == b'\t') {
                    i += 1;
                }
            }
            Some(b' ' | b'\t') if digits => {
                // spaces before a comma: "[1 , 2]"
                let mut j = i;
                while bytes.get(j).is_some_and(|b| *b == b' ' || *b == b'\t') {
                    j += 1;
                }
                if bytes.get(j) != Some(&b',') {
                    return None;
                }
                i = j;
            }
            _ => return None,
        }
    }
}

/// The answer as the user sees it, and the cited numbers in order of first use:
/// - numbers outside `1..=count` are removed (a marker with none left disappears);
/// - `[1, 2]` becomes `[1] [2]`;
/// - where a marker disappeared, the spaces before it go when punctuation, a space or the line end
///   follows. Only there: unlike `formats.py`, the rest of the text is never touched, so Markdown
///   indentation survives.
pub fn read(text: &str, count: usize) -> (String, Vec<u32>) {
    let mut out = String::with_capacity(text.len());
    let mut cited: Vec<u32> = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match marker_len(after) {
            Some(len) => {
                out.push_str(&rest[..open]);
                let keep: Vec<u32> = after[..len]
                    .split(',')
                    .filter_map(|n| n.trim().parse::<u32>().ok())
                    .filter(|&n| n >= 1 && (n as usize) <= count)
                    .collect();
                for n in &keep {
                    if !cited.contains(n) {
                        cited.push(*n);
                    }
                }
                rest = &after[len + 1..];
                if keep.is_empty() {
                    let next = rest.chars().next();
                    if matches!(next, None | Some('.' | ',' | ';' | ':' | ' ' | '\t' | '\n' | '\r')) {
                        let trimmed = out.trim_end_matches([' ', '\t']).len();
                        out.truncate(trimmed);
                    }
                } else {
                    out.push_str(
                        &keep
                            .iter()
                            .map(|n| format!("[{n}]"))
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    (out, cited)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_markers_are_kept_and_listed_in_order() {
        let (text, cited) = read("The K-factor is 0.982 [2]. Weigh it [1, 2] [3].", 3);
        assert_eq!(text, "The K-factor is 0.982 [2]. Weigh it [1] [2] [3].");
        assert_eq!(cited, vec![2, 1, 3]);
    }

    #[test]
    fn numbers_that_are_not_sources_are_removed() {
        let (text, cited) = read("Torque to 210 Nm [7]. Grease weekly [1, 9].", 2);
        assert_eq!(text, "Torque to 210 Nm. Grease weekly [1].");
        assert_eq!(cited, vec![1]);
        let (text, cited) = read("Nothing here [4] at all.", 0);
        assert_eq!(text, "Nothing here at all.");
        assert!(cited.is_empty());
        // Markdown indentation elsewhere is kept.
        let (text, _) = read("- a [3]\n  - nested [1]\n    code  here", 1);
        assert_eq!(text, "- a\n  - nested [1]\n    code  here");
    }

    #[test]
    fn other_brackets_are_left_alone() {
        let s = "See [the page](page:x), [OPS-12], [ ] and [1a].";
        assert_eq!(read(s, 5), (s.to_string(), vec![]));
        assert_eq!(read("[1 , 2]", 2).1, vec![1, 2]);
    }
}
