//! Small helpers shared by all modules.

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Params};
use serde::{Serialize, de::DeserializeOwned};

use crate::Result;

/// Current time as an ISO 8601 UTC timestamp with milliseconds, e.g. `2026-10-14T09:30:00.123Z`.
/// Timestamps are stored and sent to the UI as strings in this format.
pub fn now_iso() -> String {
    iso(Utc::now())
}

/// A time in the format of [`now_iso`].
pub fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// New random id. UUID v7 sorts by creation time, which keeps indexes tidy.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// The string an enum value is stored and sent as: its serde name (`in_progress`, `wont_do`).
pub(crate) fn enum_str<T: Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

/// The enum value of a stored string, as a row-mapping error (column `idx`) when it isn't one.
pub(crate) fn parse_enum<T: DeserializeOwned>(idx: usize, s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Creates and removes a small file in `dir`, to find out whether Cairn can write there.
pub(crate) fn try_write(dir: &std::path::Path) -> std::io::Result<()> {
    let probe = dir.join(".cairn-write-test");
    std::fs::write(&probe, b"")?;
    let _ = std::fs::remove_file(probe);
    Ok(())
}

/// Whether `sql` (a `SELECT 1 …`) finds a row.
pub(crate) fn exists(conn: &Connection, sql: &str, args: impl Params) -> Result<bool> {
    Ok(conn.query_row(sql, args, |_| Ok(())).optional()?.is_some())
}

/// Whether `table` has a row with this id. `table` is always a name from the code, never user text.
pub(crate) fn has_id(conn: &Connection, table: &str, id: &str) -> Result<bool> {
    exists(conn, &format!("SELECT 1 FROM {table} WHERE id = ?1"), [id])
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
