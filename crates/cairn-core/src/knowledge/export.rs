//! Markdown export of one page (TC-8a).
//!
//! The file starts with YAML front matter (title, category, group, tags, source info, created,
//! updated). In the body, page mentions become the page title, `#KEY` becomes `KEY Title`, and
//! `attachment:<name>` images become files in `<stem>_files/` next to the `.md` (Folder) or inside
//! the zip (Zip). `<stem>` is the file name the user chose, without its extension.

use std::collections::BTreeSet;
use std::io::{BufWriter, Write};
use std::path::Path;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use super::*;
use crate::mentions;

/// A YAML double-quoted scalar (JSON strings are valid YAML).
pub(super) fn yaml(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Percent-encodes what would break a Markdown link target (spaces, parentheses, ...).
pub(super) fn link_encode(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || !c.is_ascii() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

fn io_err(e: zip::result::ZipError) -> CoreError {
    CoreError::Io(std::io::Error::other(e))
}

/// A file name made from the title, for when the chosen path has no usable stem.
pub(super) fn safe_stem(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "page".into() } else { s }
}

pub(super) fn export_markdown(
    db: &Db,
    page_id: &str,
    path: &Path,
    mode: MarkdownExportMode,
) -> Result<PageExport> {
    let conn = db.conn();
    let page = get_page(conn, page_id)?;
    let category = categories::get(conn, &page.category_id)?;
    let group = match &page.group_id {
        Some(g) => Some(get_group(conn, g)?.name),
        None => None,
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| safe_stem(&page.title));
    let files_dir = format!("{stem}_files");
    let att_dir = db.paths().attachments_dir();
    let (md, images) = render(
        db,
        &page,
        &category.name,
        group.as_deref(),
        &files_dir,
        &mut |id| find_page(conn, id).ok().flatten().map(|p| p.title),
    );

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match mode {
        MarkdownExportMode::Folder => {
            std::fs::write(path, md)?;
            if !images.is_empty() {
                let target = path.with_file_name(&files_dir);
                std::fs::create_dir_all(&target)?;
                for name in &images {
                    std::fs::copy(att_dir.join(name), target.join(name))?;
                }
            }
        }
        MarkdownExportMode::Zip => {
            let opts = SimpleFileOptions::default();
            let mut zip = ZipWriter::new(BufWriter::new(std::fs::File::create(path)?));
            zip.start_file(format!("{stem}.md"), opts).map_err(io_err)?;
            zip.write_all(md.as_bytes())?;
            for name in &images {
                zip.start_file(format!("{files_dir}/{name}"), opts)
                    .map_err(io_err)?;
                zip.write_all(&std::fs::read(att_dir.join(name))?)?;
            }
            zip.finish().map_err(io_err)?.flush()?;
        }
    }
    Ok(PageExport {
        path: path.to_string_lossy().into_owned(),
        images: images.len() as u32,
    })
}

/// The page as a Markdown file: front matter (title, category, group, tags, source info, created,
/// updated), then the body with mentions as readable text and `attachment:` images pointing into
/// `files_dir` (relative to the `.md`). `page_link` gives the text for a page mention, e.g. its
/// title, or a link to the other page's file (folder export); `None` keeps the mention's own text.
/// Returns the text and the names of the images it uses (only those whose file exists).
pub(super) fn render(
    db: &Db,
    page: &Page,
    category: &str,
    group: Option<&str>,
    files_dir: &str,
    page_link: &mut dyn FnMut(&str) -> Option<String>,
) -> (String, BTreeSet<String>) {
    let conn = db.conn();
    let link_dir = link_encode(files_dir);
    let readable = mentions::to_readable(&page.body, page_link, &mut |key| {
        tasks::get(conn, key)
            .ok()
            .map(|t| format!("{} {}", t.key, t.title))
    });
    let att_dir = db.paths().attachments_dir();
    let mut images: BTreeSet<String> = BTreeSet::new();
    let body = mentions::map_prose(&readable, |seg| {
        let mut seg = seg.to_string();
        for name in attachments::referenced_names(&seg) {
            if name.contains('/') || name.contains('\\') || !att_dir.join(&name).is_file() {
                continue;
            }
            seg = seg.replace(
                &format!("attachment:{name}"),
                &format!("{link_dir}/{}", link_encode(&name)),
            );
            images.insert(name);
        }
        seg
    });

    let mut md = String::from("---\n");
    md.push_str(&format!("title: {}\n", yaml(&page.title)));
    md.push_str(&format!("category: {}\n", yaml(category)));
    if let Some(g) = group {
        md.push_str(&format!("group: {}\n", yaml(g)));
    }
    if !page.tags.is_empty() {
        let tags: Vec<String> = page.tags.iter().map(|t| yaml(t)).collect();
        md.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    if let Some(s) = &page.source_info {
        md.push_str(&format!("source_info: {}\n", yaml(s)));
    }
    md.push_str(&format!("created: {}\n", yaml(&page.created_at)));
    md.push_str(&format!("updated: {}\n", yaml(&page.updated_at)));
    md.push_str("---\n\n");
    md.push_str(&body);
    if !md.ends_with('\n') {
        md.push('\n');
    }
    (md, images)
}
