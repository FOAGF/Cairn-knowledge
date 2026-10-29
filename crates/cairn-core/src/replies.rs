//! Reading LLM replies: the shared parts of `prompts/README.md` "Local checks" (M5). Each
//! feature's own checks (the check result, the term filter, group suggestions) live with the
//! feature; these are the pieces every flow needs.

use serde_json::{Map, Value};

/// Strips a code fence that wraps the whole reply (```` ``` ```` or ```` ```lang ````), and blank
/// lines around it. Fences inside the text are kept.
pub fn strip_fence(text: &str) -> &str {
    let t = text.trim_matches(|c: char| c == '\n' || c == '\r' || c == ' ');
    if let Some(rest) = t.strip_prefix("```")
        && let Some(inner) = rest.strip_suffix("```")
    {
        // Drop the info string on the opening line ("json", "markdown", ...).
        let inner = match inner.find('\n') {
            Some(i) if !inner[..i].trim().contains(' ') => &inner[i + 1..],
            _ => inner,
        };
        // Only a fence that wraps everything: no other fence line may close it early.
        if !inner.lines().any(|l| l.trim_start().starts_with("```")) {
            return inner.trim_matches(|c: char| c == '\n' || c == '\r');
        }
    }
    t
}

/// README "JSON shapes": the text between the first `{` and the last `}` (after stripping a
/// wrapping fence), parsed. `None` when that isn't a JSON object, and the caller sends
/// `check-retry` once.
pub fn json_object(text: &str) -> Option<Map<String, Value>> {
    let t = strip_fence(text);
    let (start, end) = (t.find('{')?, t.rfind('}')?);
    if end <= start {
        return None;
    }
    match serde_json::from_str::<Value>(&t[start..=end]).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// A page the model wrote, read by README "Pages": `body` is the whole Markdown and always starts
/// with the `# title` line (the convention of every page body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageReply {
    pub title: String,
    pub body: String,
}

fn heading_title(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
        let title = t[hashes..].trim().trim_end_matches('#').trim();
        (!title.is_empty()).then_some(title)
    } else {
        None
    }
}

/// The page in a reply: line 1 `# title` is the title; otherwise the first heading anywhere,
/// otherwise `fallback_title` (the file name, task title, chosen topic or existing page), and the
/// `# title` line is added in front.
pub fn page(text: &str, fallback_title: &str) -> PageReply {
    let body = strip_fence(text).trim().to_string();
    let first = body.lines().next().unwrap_or("");
    if let Some(title) = first.strip_prefix("# ").map(str::trim).filter(|t| !t.is_empty()) {
        return PageReply {
            title: title.to_string(),
            body,
        };
    }
    let title = body
        .lines()
        .find_map(heading_title)
        .unwrap_or(fallback_title)
        .trim()
        .to_string();
    let body = if body.is_empty() {
        format!("# {title}")
    } else {
        format!("# {title}\n\n{body}")
    };
    PageReply { title, body }
}

/// `revise` (output `page-or-text`): a reply whose first line starts with `# ` is a revised page
/// (a new draft card); anything else is a chat answer and the draft stays as it was.
pub fn is_page(text: &str) -> bool {
    strip_fence(text).trim_start().starts_with("# ")
}

/// Replaces the `# title` line of a page body (e.g. *Suggest merge* keeps the existing page's
/// title, README "Local checks").
pub fn with_title(body: &str, title: &str) -> String {
    match body.split_once('\n') {
        Some((first, rest)) if first.starts_with("# ") => format!("# {title}\n{rest}"),
        None if body.starts_with("# ") => format!("# {title}"),
        _ => format!("# {title}\n\n{body}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fences_are_stripped_only_when_they_wrap_everything() {
        assert_eq!(strip_fence("```json\n{\"a\": 1}\n```\n"), "{\"a\": 1}");
        assert_eq!(strip_fence("```\n# T\n\nbody\n```"), "# T\n\nbody");
        let inner = "# T\n\n```sh\nls\n```\n\nmore";
        assert_eq!(strip_fence(inner), inner);
    }

    #[test]
    fn json_objects_are_found_inside_chatter() {
        let v = json_object("Sure! Here it is:\n```json\n{\"topics\": [\"a\"]}\n```").unwrap();
        assert_eq!(v["topics"][0], "a");
        assert!(json_object("{topics: oops").is_none());
        assert!(json_object("[1, 2]").is_none());
    }

    #[test]
    fn pages_always_start_with_their_title() {
        assert_eq!(page("# Pump calibration\n\nText", "f.pdf").title, "Pump calibration");
        let p = page("Intro\n\n## Calibration\nText", "manual.pdf");
        assert_eq!((p.title.as_str(), p.body.starts_with("# Calibration\n\nIntro")), ("Calibration", true));
        let q = page("Just text", "manual");
        assert_eq!(q.body, "# manual\n\nJust text");
        assert!(is_page("```markdown\n# New\n\nx\n```"));
        assert!(!is_page("The K-factor is 0.982."));
    }

    #[test]
    fn titles_are_replaced() {
        assert_eq!(with_title("# New title\n\nText", "Old"), "# Old\n\nText");
        assert_eq!(with_title("Text", "Old"), "# Old\n\nText");
    }
}
