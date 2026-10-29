//! Pasted images (FR-KN-ED-3, -11), content-addressed files in `<data>/attachments/`.
//!
//! Decisions: only raster images are accepted (png, jpeg, gif, webp); SVG is refused because it can
//! carry scripts. Files are named `<first 16 hex of sha256>.<ext>`, so the same image pasted twice is
//! stored once. A file is deleted only when no page body, page version, task description, task
//! version, comment or draft mentions `attachment:<name>`.

use std::collections::BTreeSet;
use std::io::Write;

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::knowledge::Attachment;
use crate::util::now_iso;
use crate::{CoreError, Db, Result};

/// Every place that can hold Markdown with `attachment:<name>` references.
const TEXT_SOURCES: &[(&str, &str)] = &[
    ("pages", "body"),
    ("page_versions", "snapshot"),
    ("tasks", "description"),
    ("task_versions", "snapshot"),
    ("task_comments", "text"),
    ("drafts", "content"),
];

/// File extension for an accepted image MIME type.
fn extension(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn mime_for(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        _ => "image/webp",
    }
}

/// The file extension and the MIME type Cairn stores for an image of type `mime`; other types
/// are refused (PNG, JPEG, GIF and WebP only).
pub(crate) fn image_type(mime: &str) -> Result<(&'static str, &'static str)> {
    let ext = extension(mime).ok_or_else(|| {
        CoreError::Invalid(format!(
            "\"{mime}\" is not a supported image type. Use PNG, JPEG, GIF or WebP."
        ))
    })?;
    Ok((ext, mime_for(ext)))
}

/// Stores image bytes (deduplicated by content hash) and records them. Only image MIME types.
pub fn save(db: &mut Db, bytes: &[u8], mime: &str) -> Result<Attachment> {
    let att = store_file(&db.paths().attachments_dir(), bytes, mime)?;
    db.tx(|tx| record(tx, &att))?;
    Ok(att)
}

/// The file part of [`save`]: writes the image into `dir` under its content name (once).
pub(crate) fn store_file(dir: &std::path::Path, bytes: &[u8], mime: &str) -> Result<Attachment> {
    let (ext, _) = image_type(mime)?;
    if bytes.is_empty() {
        return Err(CoreError::Invalid("The image is empty.".into()));
    }
    let size = u32::try_from(bytes.len())
        .map_err(|_| CoreError::Invalid("The image is too large.".into()))?;
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let name = format!("{hex}.{ext}");
    std::fs::create_dir_all(dir)?;
    let file = dir.join(&name);
    if !file.exists() {
        let tmp = dir.join(format!("{name}.tmp"));
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &file)?;
    }
    Ok(Attachment {
        name,
        mime: mime_for(ext).to_string(),
        size,
    })
}

/// The row of an image file already in `dir` (one a switch backup brought, which carries no
/// rows), unless it has one.
pub(crate) fn record_file(conn: &Connection, dir: &std::path::Path, name: &str) -> Result<()> {
    let Some((_, ext)) = name.rsplit_once('.') else { return Ok(()) };
    let Ok(meta) = std::fs::metadata(dir.join(name)) else { return Ok(()) };
    let att = Attachment {
        name: name.to_string(),
        mime: mime_for(&ext.to_ascii_lowercase()).to_string(),
        size: u32::try_from(meta.len()).unwrap_or(u32::MAX),
    };
    record(conn, &att)
}

/// The row part of [`save`].
pub(crate) fn record(conn: &Connection, att: &Attachment) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO attachments (name, mime, size, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![att.name, att.mime, att.size, now_iso()],
    )?;
    Ok(())
}

/// Attachment names referenced as `attachment:<name>` in `text`.
pub(crate) fn referenced_names(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find("attachment:") {
        rest = &rest[i + "attachment:".len()..];
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
            .unwrap_or(rest.len());
        let name = rest[..len].trim_end_matches('.');
        if !name.is_empty() {
            out.insert(name.to_string());
        }
    }
    out
}

fn is_referenced(conn: &Connection, name: &str) -> Result<bool> {
    let needle = format!("attachment:{name}");
    for (table, col) in TEXT_SOURCES {
        let hit = conn.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE instr({col}, ?1) > 0)"),
            [&needle],
            |r| r.get::<_, bool>(0),
        )?;
        if hit {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Deletes the unreferenced ones among `names` (rows and files). Returns how many were removed.
pub(crate) fn collect_among(db: &mut Db, names: &BTreeSet<String>) -> Result<u32> {
    let dir = db.paths().attachments_dir();
    let doomed = db.tx(|tx| {
        let mut doomed = Vec::new();
        for name in names {
            if !is_referenced(tx, name)? {
                tx.execute("DELETE FROM attachments WHERE name = ?1", [name])?;
                doomed.push(name.clone());
            }
        }
        Ok(doomed)
    })?;
    let mut n = 0;
    for name in &doomed {
        // Names come from the table or from the directory listing, never from a path.
        if name.contains('/') || name.contains('\\') || name.starts_with('.') {
            continue;
        }
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => n += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(error = %e, name, "could not delete an attachment file"),
        }
    }
    Ok(n)
}

/// Deletes attachment files and rows that no page body, page version, task description,
/// comment or draft refers to any more. Run after a page delete and at startup.
pub fn collect_garbage(db: &mut Db) -> Result<u32> {
    let mut names: BTreeSet<String> = {
        let mut stmt = db.conn().prepare("SELECT name FROM attachments")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    // Files without a row (e.g. a crash between writing the file and recording it).
    if let Ok(entries) = std::fs::read_dir(db.paths().attachments_dir()) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let ours = name.split_once('.').is_some_and(|(h, ext)| {
                h.len() == 16
                    && h.bytes().all(|b| b.is_ascii_hexdigit())
                    && matches!(ext, "png" | "jpg" | "gif" | "webp")
            });
            if ours {
                names.insert(name);
            }
        }
    }
    collect_among(db, &names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_names() {
        let t = "![a](attachment:0123456789abcdef.png) and attachment:fedcba9876543210.jpg. \\nattachment:x.gif\\n";
        let n = referenced_names(t);
        assert!(n.contains("0123456789abcdef.png"));
        assert!(n.contains("fedcba9876543210.jpg"));
        assert!(n.contains("x.gif"));
    }
}
