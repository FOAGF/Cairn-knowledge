//! Small helpers shared by all modules.

use chrono::{SecondsFormat, Utc};

/// Current time as an ISO 8601 UTC timestamp with milliseconds, e.g. `2026-10-14T09:30:00.123Z`.
/// Timestamps are stored and sent to the UI as strings in this format.
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// New random id. UUID v7 sorts by creation time, which keeps indexes tidy.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// A file name that is valid on Windows and Linux, for the name Cairn *suggests* in a *Save as*
/// dialog (D-58). It replaces `<>:"/\|?*` and control characters with `-`, trims dots and spaces
/// from the end (and spaces from the start), avoids Windows' reserved names (`CON`, `PRN`, `AUX`,
/// `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`, with or without an extension) by adding `_`, and keeps
/// other scripts such as Chinese as they are. An empty result becomes `fallback`. Applied on every
/// platform, so a file moved to Windows later is fine.
pub fn safe_file_name(name: &str, fallback: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut out = replaced.trim_matches(|c| c == ' ').to_string();
    out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        out = fallback.to_string();
    }
    // The reserved names count with any extension (`con.txt`) and with trailing spaces before it.
    let stem = out
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        out.insert(stem.len().min(out.find('.').unwrap_or(out.len())), '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::safe_file_name as f;

    #[test]
    fn bad_characters_become_hyphens() {
        assert_eq!(f("a<b>c:d\"e/f\\g|h?i*j", "page"), "a-b-c-d-e-f-g-h-i-j");
        assert_eq!(f("tab\there\u{7}", "page"), "tab-here-");
    }

    #[test]
    fn dots_and_spaces_at_the_end_go() {
        assert_eq!(f("  notes. . ", "page"), "notes");
        assert_eq!(f("...", "page"), "page");
        assert_eq!(f("", "page"), "page");
    }

    #[test]
    fn reserved_names_are_avoided_with_or_without_an_extension() {
        assert_eq!(f("CON", "page"), "CON_");
        assert_eq!(f("nul", "page"), "nul_");
        assert_eq!(f("Com1", "page"), "Com1_");
        assert_eq!(f("lpt9.txt", "page"), "lpt9_.txt");
        assert_eq!(f("aux.tar.gz", "page"), "aux_.tar.gz");
        // Not reserved.
        assert_eq!(f("COM0", "page"), "COM0");
        assert_eq!(f("COM10", "page"), "COM10");
        assert_eq!(f("console", "page"), "console");
    }

    #[test]
    fn chinese_and_other_text_stay() {
        assert_eq!(f("知识 页面：第一章", "page"), "知识 页面：第一章");
        assert_eq!(f("Ünï café 😀", "page"), "Ünï café 😀");
    }
}
