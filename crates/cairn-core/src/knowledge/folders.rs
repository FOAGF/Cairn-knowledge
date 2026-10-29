//! Markdown folders (FR-KN-MD-1..4, D-68): import every `.md` file of a folder (subfolders too)
//! into one category and group, and export chosen categories and groups to a folder.
//!
//! Both run in two steps. The *plan* reads everything and says what would happen: which pages are
//! created, replaced or skipped, which files are written or replaced. The UI lists it and asks for
//! confirmation. The *run* makes the plan again and refuses if the replacements are not exactly
//! the ones the user confirmed, so nothing is replaced that wasn't listed.
//!
//! Import: the title is the file name; a page with that title in the category gets the file's text
//! (title and group stay, tags are added, it becomes not checked), after a backup of all data. A
//! second file with the same name is skipped. Images linked by a relative path are stored as page
//! images; links to other imported files become page links.
//!
//! Export: pages are chosen one by one (or whole groups and categories in the UI); the layout
//! follows their groups and categories (one group: flat; several groups: a folder per group;
//! several categories: category folders with group folders inside; pages in no group go in their
//! category's folder). Pages are written as by the one-page export (TC-8a); links between exported
//! pages become relative links. Files with the same name are replaced; nothing else is touched.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;

use super::export::{link_encode, render};
use super::*;
use crate::mentions;
use crate::util::safe_file_name;

// ---------------------------------------------------------------- types

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderImportInput {
    pub folder: String,
    pub category_id: String,
    /// An existing group of the category; none or empty = no group.
    #[specta(optional)]
    pub group_id: Option<String>,
    /// A group made for this import, instead of `group_id`.
    #[specta(optional)]
    pub new_group: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportAction {
    Create,
    Replace,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportFile {
    /// Relative to the folder, with `/`.
    pub path: String,
    pub title: String,
    pub action: ImportAction,
    /// `replace`: the page whose text is replaced.
    pub page_id: Option<String>,
    /// `replace`: that page is archived (it stays archived).
    pub page_archived: bool,
    /// `skip`: why.
    pub reason: Option<String>,
    /// Images linked by a relative path that will be stored with the page.
    pub images: u32,
    /// Image links that can't be used (no such file, or not PNG, JPEG, GIF or WebP); they stay as written.
    pub missing_images: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub files: Vec<ImportFile>,
    pub create: u32,
    pub replace: u32,
    pub skip: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderImportResult {
    pub created: u32,
    pub replaced: u32,
    pub skipped: u32,
    pub images: u32,
    /// The group the pages went into (a new one when it was made by the import).
    pub group_id: Option<String>,
    /// The backup of all data made before pages were replaced.
    pub backup_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderExportInput {
    pub folder: String,
    /// The pages to export (archived ones too, when chosen).
    pub page_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    /// Relative to the folder, with `/`.
    pub path: String,
    pub page_id: String,
    pub title: String,
    /// A file with this name is already there and will be replaced.
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    pub files: Vec<ExportFile>,
    pub replace: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderExportResult {
    pub written: u32,
    pub replaced: u32,
    pub images: u32,
}

/// FR-KN-EXT-13: the Markdown folder import and export are for internal mode.
fn internal_only(conn: &Connection) -> Result<()> {
    if super::external::is_external(conn)? {
        return Err(CoreError::Invalid(
            "Markdown folder import and export are not available while knowledge is kept in a folder.".into(),
        ));
    }
    Ok(())
}

fn changed_since_check() -> CoreError {
    CoreError::Conflict(
        "The folder or the pages changed since the check. Check again before going on.".into(),
    )
}

// ---------------------------------------------------------------- import: reading the folder

const IMAGE_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

/// A file of the import with what was read from it.
struct Source {
    file: ImportFile,
    abs: PathBuf,
    body: String,
    tags: Vec<String>,
    source_info: Option<String>,
}

/// Every `.md` / `.markdown` file under `root`, with its path relative to `root`, sorted. Hidden
/// files and folders (`.git`, `.obsidian`) are left out, and linked folders are not followed.
fn markdown_files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let rel_name = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&path, &rel_name, out)?;
            } else if path.is_file() {
                let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase());
                if matches!(ext.as_deref(), Some("md" | "markdown")) {
                    out.push((rel_name, path));
                }
            }
        }
        Ok(())
    }
    if !root.is_dir() {
        return Err(CoreError::Invalid(format!(
            "The folder \"{}\" can't be read. Choose another folder.",
            root.display()
        )));
    }
    let mut out = Vec::new();
    walk(root, "", &mut out)?;
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()).then(a.0.cmp(&b.0)));
    Ok(out)
}

/// `(front matter, body)` when the text starts with a `---` block closed by `---` or `...`.
fn split_front_matter(text: &str) -> (Option<&str>, &str) {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return (None, text);
    };
    let mut pos = 0;
    for line in rest.split_inclusive('\n') {
        let t = line.trim_end_matches(['\r', '\n']);
        if t == "---" || t == "..." {
            return (Some(&rest[..pos]), &rest[pos + line.len()..]);
        }
        pos += line.len();
    }
    (None, text)
}

/// A YAML scalar as plain text: `"json style"`, `'single'` or bare.
pub(super) fn yaml_scalar(v: &str) -> String {
    let v = v.trim();
    if v.starts_with('"') {
        if let Ok(s) = serde_json::from_str::<String>(v) {
            return s;
        }
        return v.trim_matches('"').to_string();
    }
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        return v[1..v.len() - 1].replace("''", "'");
    }
    v.to_string()
}

/// Splits `a, "b, c", d` at the commas outside quotes.
pub(super) fn split_list(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quote) = (String::new(), None::<char>);
    for c in v.chars() {
        match (c, quote) {
            ('"' | '\'', None) => {
                quote = Some(c);
                cur.push(c);
            }
            (q, Some(open)) if q == open => {
                quote = None;
                cur.push(c);
            }
            (',', None) => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| yaml_scalar(&s)).filter(|s| !s.is_empty()).collect()
}

/// `tags` (inline list, block list or comma-separated) and `source` / `source_info` from the
/// front matter. Other keys are ignored.
fn front_matter_fields(fm: &str) -> (Vec<String>, Option<String>) {
    let mut tags = Vec::new();
    let mut source = None;
    let mut in_tags = false;
    for line in fm.lines() {
        let trimmed = line.trim();
        if in_tags {
            if let Some(item) = trimmed.strip_prefix("- ").or_else(|| (trimmed == "-").then_some("")) {
                let t = yaml_scalar(item);
                if !t.is_empty() {
                    tags.push(t);
                }
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            in_tags = false;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let value = value.trim();
        match key.trim() {
            "tags" | "tag" => {
                if value.is_empty() {
                    in_tags = true;
                } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                    tags.extend(split_list(inner));
                } else {
                    tags.extend(split_list(value));
                }
            }
            "source" | "source_info" if !value.is_empty() => source = Some(yaml_scalar(value)),
            _ => {}
        }
    }
    (tags, source)
}

/// Calls `f(is_image, text, target)` for every inline link `[text](target)` and image
/// `![text](target)` in a prose stretch; a returned string replaces the whole link.
fn map_links(s: &str, f: &mut dyn FnMut(bool, &str, &str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    let mut search = 0;
    while let Some(rel) = s[search..].find('[') {
        let open = search + rel;
        search = open + 1;
        if open > 0 && s.as_bytes()[open - 1] == b'\\' {
            continue;
        }
        // The text, to the matching `]` on the same line (escaped brackets allowed).
        let mut close = None;
        let mut escaped = false;
        for (i, c) in s[open + 1..].char_indices() {
            match c {
                '\n' => break,
                '\\' if !escaped => {
                    escaped = true;
                    continue;
                }
                ']' if !escaped => {
                    close = Some(open + 1 + i);
                    break;
                }
                _ => {}
            }
            escaped = false;
        }
        let Some(close) = close else { continue };
        if !s[close + 1..].starts_with('(') {
            continue;
        }
        let inner_start = close + 2;
        // The target: `<...>`, or up to a space or the closing parenthesis.
        let (target, end) = if s[inner_start..].starts_with('<') {
            let Some(gt) = s[inner_start..].find('>') else { continue };
            let target = &s[inner_start + 1..inner_start + gt];
            let Some(paren) = s[inner_start + gt..].find(')') else { continue };
            (target, inner_start + gt + paren + 1)
        } else {
            let Some(paren) = s[inner_start..].find(')') else { continue };
            let inner = &s[inner_start..inner_start + paren];
            if inner.contains('\n') {
                continue;
            }
            (inner.split_whitespace().next().unwrap_or(""), inner_start + paren + 1)
        };
        let image = open > 0 && s.as_bytes()[open - 1] == b'!';
        let start = if image { open - 1 } else { open };
        if start < pos {
            continue;
        }
        if let Some(rep) = f(image, &s[open + 1..close], target) {
            out.push_str(&s[pos..start]);
            out.push_str(&rep);
            pos = end;
            search = end;
        }
    }
    out.push_str(&s[pos..]);
    out
}

/// `%20` and friends decoded; the text as is when it isn't valid.
pub(super) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16);
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// The file a relative link target points to, from the folder of `md_file`; `None` for web
/// addresses, other schemes, anchors and absolute paths.
fn local_target(md_file: &Path, target: &str) -> Option<PathBuf> {
    let target = target.split('#').next().unwrap_or("");
    if target.is_empty()
        || target.contains("://")
        || target.starts_with('/')
        || target.starts_with('\\')
        || target.split_once(':').is_some_and(|(scheme, _)| !scheme.contains(['/', '\\', '.']))
    {
        return None;
    }
    let mut path = md_file.parent()?.to_path_buf();
    for part in Path::new(&percent_decode(target)).components() {
        match part {
            Component::Normal(p) => path.push(p),
            Component::ParentDir => {
                path.pop();
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(path)
}

fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    IMAGE_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// The local images a body links to: the usable ones (found, a supported type) and how many
/// links can't be used.
fn body_images(abs: &Path, body: &str) -> (Vec<PathBuf>, u32) {
    let mut found = Vec::new();
    let mut missing = 0;
    mentions::map_prose(body, |seg| {
        map_links(seg, &mut |image, _, target| {
            if image {
                match local_target(abs, target) {
                    Some(p) if p.is_file() && image_mime(&p).is_some() => found.push(p),
                    Some(_) => missing += 1,
                    None => {}
                }
            }
            None
        });
        String::new()
    });
    (found, missing)
}

/// Reads the folder and decides, file by file, what the import will do.
fn read_import(conn: &Connection, input: &FolderImportInput) -> Result<Vec<Source>> {
    internal_only(conn)?;
    active_category(conn, &input.category_id)?;
    match (input.new_group.as_deref().map(str::trim), input.group_id.as_deref().map(str::trim)) {
        (Some(name), _) if !name.is_empty() => {
            let name = clean_group_name(name)?;
            check_group_name_free(conn, &input.category_id, &name, None)?;
        }
        (_, Some(g)) if !g.is_empty() => check_group(conn, &input.category_id, g)?,
        _ => {}
    }
    let root = PathBuf::from(&input.folder);
    let files = markdown_files(&root)?;

    // The category's pages by title (case-insensitive, FR-KN-ED-12).
    let mut stmt = conn.prepare("SELECT id, title, archived FROM pages WHERE category_id = ?1")?;
    let existing: HashMap<String, (String, bool)> = stmt
        .query_map([&input.category_id], |r| {
            Ok((r.get::<_, String>(1)?.to_lowercase(), (r.get(0)?, r.get(2)?)))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut seen: HashMap<String, String> = HashMap::new();
    let mut out = Vec::new();
    for (rel, abs) in files {
        let stem = abs
            .file_stem()
            .map(|s| s.to_string_lossy().trim().to_string())
            .unwrap_or_default();
        let mut file = ImportFile {
            path: rel.clone(),
            title: stem.clone(),
            action: ImportAction::Skip,
            page_id: None,
            page_archived: false,
            reason: None,
            images: 0,
            missing_images: 0,
        };
        let mut source = Source {
            file: file.clone(),
            abs: abs.clone(),
            body: String::new(),
            tags: Vec::new(),
            source_info: None,
        };
        let skip = |mut source: Source, why: String| {
            source.file.reason = Some(why);
            source
        };
        if stem.is_empty() {
            out.push(skip(source, "The file has no name to use as a title.".into()));
            continue;
        }
        if let Some(first) = seen.get(&stem.to_lowercase()) {
            out.push(skip(source, format!("Another file in this import has the same name: {first}.")));
            continue;
        }
        seen.insert(stem.to_lowercase(), rel.clone());
        let text = match std::fs::read(&abs) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(t) => t,
                Err(_) => {
                    out.push(skip(source, "The file is not UTF-8 text.".into()));
                    continue;
                }
            },
            Err(e) => {
                out.push(skip(source, format!("The file can't be read: {e}")));
                continue;
            }
        };
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let (fm, body) = split_front_matter(text);
        let (tags, source_info) = fm.map(front_matter_fields).unwrap_or_default();
        let body = body.trim_start_matches(['\r', '\n']).to_string();
        let (images, missing) = body_images(&abs, &body);
        file.images = images.len() as u32;
        file.missing_images = missing;
        match existing.get(&stem.to_lowercase()) {
            Some((id, archived)) => {
                file.action = ImportAction::Replace;
                file.page_id = Some(id.clone());
                file.page_archived = *archived;
            }
            None => file.action = ImportAction::Create,
        }
        source.file = file;
        source.body = body;
        source.tags = tags;
        source.source_info = source_info;
        out.push(source);
    }
    Ok(out)
}

fn plan_of(sources: &[Source]) -> ImportPlan {
    let count = |a: ImportAction| sources.iter().filter(|s| s.file.action == a).count() as u32;
    ImportPlan {
        files: sources.iter().map(|s| s.file.clone()).collect(),
        create: count(ImportAction::Create),
        replace: count(ImportAction::Replace),
        skip: count(ImportAction::Skip),
    }
}

// ---------------------------------------------------------------- import: plan and run

/// FR-KN-MD-1/2: what importing the folder would do. Nothing is written.
pub fn plan_folder_import(db: &Db, input: &FolderImportInput) -> Result<ImportPlan> {
    Ok(plan_of(&read_import(db.conn(), input)?))
}

/// FR-KN-MD-1/2: imports the folder. `confirmed` are the ids of the pages the user agreed to
/// replace; if the folder now replaces other pages, nothing happens and the user checks again.
pub fn import_folder(
    db: &mut Db,
    input: FolderImportInput,
    confirmed: Vec<String>,
) -> Result<FolderImportResult> {
    let sources = read_import(db.conn(), &input)?;
    let replacing: BTreeSet<&str> = sources
        .iter()
        .filter_map(|s| s.file.page_id.as_deref())
        .collect();
    if replacing != confirmed.iter().map(String::as_str).collect() {
        return Err(changed_since_check());
    }

    // FR-KN-MD-2, FR-DATA-6: a backup of all data before any page is replaced.
    let backup_path = if replacing.is_empty() {
        None
    } else {
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
        let path = db.paths().backups_dir().join(format!("pre-import-{stamp}.zip"));
        crate::export::export_to(db, &path)?;
        Some(path.to_string_lossy().into_owned())
    };

    // Images first (they are files; one nothing ends up using is removed at the next start).
    let mut stored: HashMap<PathBuf, String> = HashMap::new();
    for s in sources.iter().filter(|s| s.file.action != ImportAction::Skip) {
        for path in body_images(&s.abs, &s.body).0 {
            if stored.contains_key(&path) {
                continue;
            }
            let (Ok(bytes), Some(mime)) = (std::fs::read(&path), image_mime(&path)) else {
                continue;
            };
            if let Ok(att) = attachments::save(db, &bytes, mime) {
                stored.insert(path, att.name);
            }
        }
    }

    // Each page's id, known before any is written, so the files can link to each other.
    let ids: HashMap<PathBuf, (String, String)> = sources
        .iter()
        .filter(|s| s.file.action != ImportAction::Skip)
        .map(|s| {
            let id = s.file.page_id.clone().unwrap_or_else(new_id);
            (s.abs.clone(), (id, s.file.title.clone()))
        })
        .collect();
    let rewrite = |s: &Source| {
        mentions::map_prose(&s.body, |seg| {
            map_links(seg, &mut |image, text, target| {
                let path = local_target(&s.abs, target)?;
                if image {
                    let name = stored.get(&path)?;
                    Some(format!("![{text}](attachment:{name})"))
                } else {
                    let (id, title) = ids.get(&path)?;
                    let title = title.replace(['[', ']', '\\'], "").split_whitespace().collect::<Vec<_>>().join(" ");
                    Some(format!("[{title}](page:{id})"))
                }
            })
        })
    };

    let category_id = input.category_id.clone();
    let new_group = input.new_group.clone().filter(|g| !g.trim().is_empty());
    let chosen_group = input.group_id.clone().filter(|g| !g.trim().is_empty());
    let (mut created, mut replaced) = (0, 0);
    let group_id = db.tx(|tx| {
        let group_id = match new_group {
            Some(name) => Some(
                create_group_in(
                    tx,
                    NewGroup {
                        category_id: category_id.clone(),
                        name,
                        description: None,
                        parent_id: None,
                    },
                )?
                .id,
            ),
            None => chosen_group,
        };
        let st = Stamp::now();
        for s in sources.iter().filter(|s| s.file.action != ImportAction::Skip) {
            let body = rewrite(s);
            let (id, _) = &ids[&s.abs];
            if s.file.action == ImportAction::Create {
                create_page_as(
                    tx,
                    id.clone(),
                    NewPage {
                        category_id: category_id.clone(),
                        title: s.file.title.clone(),
                        body,
                        group_id: group_id.clone(),
                        tags: Some(s.tags.clone()),
                        source_info: s.source_info.clone(),
                        origin: Some(PageOrigin::File),
                        ..Default::default()
                    },
                    &st,
                )?;
                created += 1;
            } else {
                // FR-KN-MD-2: the text is replaced; title and group stay, tags are added.
                let old = get_page(tx, id)?;
                let mut tags = old.tags.clone();
                tags.extend(s.tags.iter().cloned());
                let new = update_page_in(
                    tx,
                    id,
                    PagePatch {
                        body: Some(body),
                        tags: Some(tags),
                        source_info: s.source_info.clone(),
                        ..Default::default()
                    },
                    &st,
                )?;
                if new != old {
                    tx.execute(
                        "UPDATE pages SET check_status = ?2 WHERE id = ?1",
                        params![id, enum_str(&CheckStatus::NotChecked)],
                    )?;
                }
                replaced += 1;
            }
        }
        Ok(group_id)
    })?;
    tracing::info!(created, replaced, "Markdown folder imported");
    Ok(FolderImportResult {
        created,
        replaced,
        skipped: sources.iter().filter(|s| s.file.action == ImportAction::Skip).count() as u32,
        images: stored.len() as u32,
        group_id,
        backup_path,
    })
}

// ---------------------------------------------------------------- export

/// A page placed in the export: its folder (as path parts) and file stem.
struct Placed {
    page: Page,
    category: String,
    group: Option<String>,
    dirs: Vec<String>,
    stem: String,
}

impl Placed {
    fn rel_parts(&self) -> Vec<String> {
        let mut parts = self.dirs.clone();
        parts.push(format!("{}.md", self.stem));
        parts
    }
    fn rel(&self) -> String {
        self.rel_parts().join("/")
    }
}

/// `name`, or `name (2)`, `name (3)`, ... until it isn't taken (case-insensitive) in `taken`.
fn unique(name: String, taken: &mut BTreeSet<String>) -> String {
    let mut candidate = name.clone();
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{name} ({n})");
        n += 1;
    }
    candidate
}

/// The chosen pages, each in its place (FR-KN-MD-3). The layout follows the groups and categories
/// the pages are in: one group (or one category's pages in no group) → straight into the folder;
/// several groups of one category → a folder per group, pages in no group in the folder itself;
/// several categories → a folder per category with its group folders inside.
fn place(conn: &Connection, input: &FolderExportInput) -> Result<Vec<Placed>> {
    internal_only(conn)?;
    if input.page_ids.is_empty() {
        return Err(CoreError::Invalid("Choose at least one page to export.".into()));
    }
    let mut seen = HashSet::new();
    let mut pages = Vec::new();
    for id in &input.page_ids {
        if seen.insert(id.as_str()) {
            pages.push(get_page(conn, id)?);
        }
    }
    let mut categories_by_id: HashMap<String, categories::Category> = HashMap::new();
    let mut groups_by_id: HashMap<String, Group> = HashMap::new();
    for p in &pages {
        if !categories_by_id.contains_key(&p.category_id) {
            categories_by_id.insert(p.category_id.clone(), categories::get(conn, &p.category_id)?);
        }
        if let Some(g) = &p.group_id
            && !groups_by_id.contains_key(g)
        {
            groups_by_id.insert(g.clone(), get_group(conn, g)?);
        }
    }
    let several_categories = categories_by_id.len() > 1;
    let buckets: BTreeSet<(&str, Option<&str>)> = pages
        .iter()
        .map(|p| (p.category_id.as_str(), p.group_id.as_deref()))
        .collect();
    let several_groups = !several_categories && buckets.len() > 1;

    // A stable order: category, then its pages in no group, then each group, then title.
    let lower = |s: &str| s.to_lowercase();
    pages.sort_by_cached_key(|p| {
        (
            lower(&categories_by_id[&p.category_id].name),
            p.category_id.clone(),
            p.group_id.as_ref().map(|g| (lower(&groups_by_id[g].name), g.clone())),
            lower(&p.title),
            p.id.clone(),
        )
    });

    let mut category_folders = BTreeSet::new();
    let mut category_folder: HashMap<String, String> = HashMap::new();
    let mut group_folders: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut group_folder: HashMap<String, String> = HashMap::new();
    let mut files_in: HashMap<String, BTreeSet<String>> = HashMap::new();
    let mut placed = Vec::new();
    for page in pages {
        let category = &categories_by_id[&page.category_id];
        let group = page.group_id.as_ref().map(|g| &groups_by_id[g]);
        let mut dirs = Vec::new();
        if several_categories {
            let name = category_folder.entry(category.id.clone()).or_insert_with(|| {
                unique(safe_file_name(&category.name, "Category"), &mut category_folders)
            });
            dirs.push(name.clone());
        }
        if let Some(g) = group
            && (several_categories || several_groups)
        {
            let taken = group_folders.entry(category.id.clone()).or_default();
            let name = group_folder
                .entry(g.id.clone())
                .or_insert_with(|| unique(safe_file_name(&g.name, "Group"), taken));
            dirs.push(name.clone());
        }
        let taken = files_in.entry(dirs.join("/")).or_default();
        let stem = unique(safe_file_name(&page.title, "page"), taken);
        placed.push(Placed {
            category: category.name.clone(),
            group: group.map(|g| g.name.clone()),
            page,
            dirs,
            stem,
        });
    }
    Ok(placed)
}

fn export_plan(root: &Path, placed: &[Placed]) -> ExportPlan {
    let files: Vec<ExportFile> = placed
        .iter()
        .map(|p| {
            let rel = p.rel();
            ExportFile {
                exists: root.join(&rel).exists(),
                path: rel,
                page_id: p.page.id.clone(),
                title: p.page.title.clone(),
            }
        })
        .collect();
    let replace = files.iter().filter(|f| f.exists).count() as u32;
    ExportPlan { files, replace }
}

/// FR-KN-MD-3/4: what exporting would write, and which files it would replace. Nothing is written.
pub fn plan_folder_export(db: &Db, input: &FolderExportInput) -> Result<ExportPlan> {
    let placed = place(db.conn(), input)?;
    Ok(export_plan(Path::new(&input.folder), &placed))
}

/// The link from a file in `from_dirs` to the file at `to` (path parts), each part encoded.
pub(super) fn relative_link(from_dirs: &[String], to: &[String]) -> String {
    let common = from_dirs
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count()
        .min(to.len().saturating_sub(1));
    let mut parts: Vec<String> = vec!["..".into(); from_dirs.len() - common];
    parts.extend(to[common..].iter().map(|p| link_encode(p)));
    parts.join("/")
}

/// FR-KN-MD-3/4: writes the pages. `confirmed` are the files (relative paths) the user agreed to
/// replace; if others would be replaced now, nothing is written and the user checks again.
pub fn export_folder(
    db: &Db,
    input: FolderExportInput,
    confirmed: Vec<String>,
) -> Result<FolderExportResult> {
    let root = PathBuf::from(&input.folder);
    if root.exists() && !root.is_dir() {
        return Err(CoreError::Invalid(format!(
            "\"{}\" is not a folder. Choose a folder.",
            root.display()
        )));
    }
    let placed = place(db.conn(), &input)?;
    let plan = export_plan(&root, &placed);
    let replacing: BTreeSet<&str> = plan
        .files
        .iter()
        .filter(|f| f.exists)
        .map(|f| f.path.as_str())
        .collect();
    if replacing != confirmed.iter().map(String::as_str).collect() {
        return Err(changed_since_check());
    }

    let by_id: HashMap<&str, &Placed> = placed.iter().map(|p| (p.page.id.as_str(), p)).collect();
    let conn = db.conn();
    let att_dir = db.paths().attachments_dir();
    let mut images = 0;
    for p in &placed {
        let files_dir = format!("{}_files", p.stem);
        let (md, used) = render(
            db,
            &p.page,
            &p.category,
            p.group.as_deref(),
            &files_dir,
            &mut |id| match by_id.get(id) {
                Some(other) => {
                    let title = other.page.title.replace(['[', ']'], "");
                    Some(format!("[{title}]({})", relative_link(&p.dirs, &other.rel_parts())))
                }
                None => find_page(conn, id).ok().flatten().map(|q| q.title),
            },
        );
        let dir = p.dirs.iter().fold(root.clone(), |d, part| d.join(part));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{}.md", p.stem)), md)?;
        if !used.is_empty() {
            let target = dir.join(&files_dir);
            std::fs::create_dir_all(&target)?;
            for name in &used {
                std::fs::copy(att_dir.join(name), target.join(name))?;
                images += 1;
            }
        }
    }
    tracing::info!(written = placed.len(), replaced = plan.replace, "Markdown folder exported");
    Ok(FolderExportResult {
        written: placed.len() as u32,
        replaced: plan.replace,
        images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_read_and_removed() {
        let (fm, body) = split_front_matter("---\ntitle: \"X\"\ntags: [\"a\", b]\nsource_info: \"Site visit\"\n---\n\n# Body\n");
        let (tags, source) = front_matter_fields(fm.unwrap());
        assert_eq!(tags, ["a", "b"]);
        assert_eq!(source.as_deref(), Some("Site visit"));
        assert_eq!(body, "\n# Body\n");
        let (tags, _) = front_matter_fields("tags:\n  - one\n  - 'two, three'\nother: x\n");
        assert_eq!(tags, ["one", "two, three"]);
        let (tags, source) = front_matter_fields("tags: a, b\nsource: web\n");
        assert_eq!((tags, source.as_deref()), (vec!["a".to_string(), "b".to_string()], Some("web")));
        assert_eq!(split_front_matter("No front matter\n---\n"), (None, "No front matter\n---\n"));
        assert_eq!(split_front_matter("---\nunclosed\n"), (None, "---\nunclosed\n"));
    }

    #[test]
    fn links_and_images_are_found() {
        let mut seen = Vec::new();
        let out = map_links(
            r#"See [a](b.md#x), ![pic](<img/a b.png> "t"), \[not](c.md) and [web](https://x.org)."#,
            &mut |image, text, target| {
                seen.push((image, text.to_string(), target.to_string()));
                (target == "b.md#x").then(|| "LINK".to_string())
            },
        );
        assert_eq!(out, r#"See LINK, ![pic](<img/a b.png> "t"), \[not](c.md) and [web](https://x.org)."#);
        assert_eq!(
            seen,
            [
                (false, "a".to_string(), "b.md#x".to_string()),
                (true, "pic".to_string(), "img/a b.png".to_string()),
                (false, "web".to_string(), "https://x.org".to_string()),
            ]
        );
        let base = Path::new("/n/sub/page.md");
        assert_eq!(local_target(base, "../img/a%20b.png"), Some(PathBuf::from("/n/img/a b.png")));
        assert_eq!(local_target(base, "https://x.org/a.png"), None);
        assert_eq!(local_target(base, "attachment:abc.png"), None);
        assert_eq!(local_target(base, "#top"), None);
    }

    #[test]
    fn relative_links_climb_out_of_folders() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(relative_link(&v(&["A", "G"]), &v(&["A", "G", "x.md"])), "x.md");
        assert_eq!(relative_link(&v(&["A", "G"]), &v(&["A", "y.md"])), "../y.md");
        assert_eq!(relative_link(&v(&[]), &v(&["B", "H", "z z.md"])), "B/H/z%20z.md");
        assert_eq!(relative_link(&v(&["A"]), &v(&["B", "q.md"])), "../B/q.md");
    }

    // ------------------------------------------------------------ with a database

    use crate::categories::{CategoryColor, NewCategory};

    fn category(db: &Db, name: &str, key: &str) -> String {
        categories::create(
            db.conn(),
            NewCategory {
                name: name.into(),
                key: key.into(),
                description: String::new(),
                color: CategoryColor::preset("blue"),
            },
        )
        .unwrap()
        .id
    }

    fn page(db: &mut Db, category: &str, group: Option<&str>, title: &str, body: &str) -> Page {
        create_page(
            db,
            NewPage {
                category_id: category.into(),
                title: title.into(),
                body: body.into(),
                group_id: group.map(str::to_string),
                tags: Some(vec!["old".into()]),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn group(db: &mut Db, category: &str, name: &str) -> String {
        create_group(
            db,
            NewGroup {
                category_id: category.into(),
                name: name.into(),
                description: None,
                parent_id: None,
            },
        )
        .unwrap()
        .id
    }

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// FR-KN-MD-1/2: subfolders, file-name titles, replacing after confirmation and a backup,
    /// duplicates and unreadable files skipped, images stored, links between files kept.
    #[test]
    fn a_folder_is_imported_into_one_category_and_group() {
        let (data, src) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut db = Db::open_at(data.path()).unwrap();
        let cat = category(&db, "Ops", "OPS");
        let pumps = page(&mut db, &cat, None, "Pumps", "Old text");
        let root = src.path();
        write(root, "Pumps.md", b"---\ntags: [new]\nsource: Manual v2\n---\n\nNew text ![d](img/d%201.png) and [hoses](sub/Hoses.md).\n");
        write(root, "sub/Hoses.md", b"Hoses, see [pumps](../Pumps.md#top) and ![gone](nope.png).\n");
        write(root, "sub/pumps.md", b"A second file named like the first.\n");
        write(root, "bad.md", &[0xff, 0xfe, 0x00]);
        write(root, "img/d 1.png", b"\x89PNG fake");
        write(root, ".hidden/x.md", b"hidden");
        write(root, "notes.txt", b"not markdown");

        let input = FolderImportInput {
            folder: root.to_string_lossy().into_owned(),
            category_id: cat.clone(),
            new_group: Some("Imported".into()),
            ..Default::default()
        };
        let plan = plan_folder_import(&db, &input).unwrap();
        assert_eq!((plan.create, plan.replace, plan.skip), (1, 1, 2));
        let by_path = |p: &str| plan.files.iter().find(|f| f.path == p).unwrap().clone();
        assert_eq!(by_path("Pumps.md").page_id.as_deref(), Some(pumps.id.as_str()));
        assert_eq!((by_path("Pumps.md").images, by_path("sub/Hoses.md").missing_images), (1, 1));
        assert_eq!(by_path("sub/pumps.md").action, ImportAction::Skip);
        assert_eq!(by_path("bad.md").reason.as_deref(), Some("The file is not UTF-8 text."));

        // Replacing a page nobody confirmed changes nothing.
        assert!(matches!(import_folder(&mut db, input.clone(), vec![]), Err(CoreError::Conflict(_))));
        assert_eq!(get_page(db.conn(), &pumps.id).unwrap().body, "Old text");

        let r = import_folder(&mut db, input, vec![pumps.id.clone()]).unwrap();
        assert_eq!((r.created, r.replaced, r.skipped, r.images), (1, 1, 2, 1));
        assert!(Path::new(r.backup_path.as_deref().unwrap()).is_file());

        let new = get_page(db.conn(), &pumps.id).unwrap();
        let hoses = list_pages(db.conn(), &PageFilter::default())
            .unwrap()
            .into_iter()
            .find(|p| p.title == "Hoses")
            .unwrap();
        assert!(new.body.starts_with("New text ![d](attachment:"), "{}", new.body);
        assert!(new.body.contains(&format!("[Hoses](page:{})", hoses.id)), "{}", new.body);
        assert_eq!(new.tags, ["new", "old"]);
        assert_eq!(new.source_info.as_deref(), Some("Manual v2"));
        assert_eq!((new.group_id, new.check_status), (None, CheckStatus::NotChecked));
        let hoses = get_page(db.conn(), &hoses.id).unwrap();
        assert_eq!(hoses.group_id, r.group_id);
        assert_eq!(hoses.origin, PageOrigin::File);
        assert_eq!(hoses.body, format!("Hoses, see [Pumps](page:{}) and ![gone](nope.png).\n", pumps.id));
    }

    /// FR-KN-MD-3/4: pages are chosen one by one; the layout follows their groups and categories;
    /// links between exported pages become relative; archived pages go too; files already there
    /// are replaced only when confirmed.
    #[test]
    fn pages_are_exported_with_a_folder_layout() {
        let (data, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut db = Db::open_at(data.path()).unwrap();
        let a = category(&db, "Ops", "OPS");
        let b = category(&db, "Learn: notes", "LRN");
        let (g1, g2) = (group(&mut db, &a, "Procedures"), group(&mut db, &a, "Reference"));
        let h = group(&mut db, &b, "Rust");
        let target = page(&mut db, &b, Some(&h), "Ownership", "Borrowing.");
        let p1 = page(&mut db, &a, Some(&g1), "Pump check", &format!("See [Ownership](page:{}).", target.id));
        let p2 = page(&mut db, &a, Some(&g1), "Hose check", "z");
        let terms = page(&mut db, &a, Some(&g2), "Glossary/terms", "x");
        let loose = page(&mut db, &a, None, "Loose", "y");
        set_archived(&mut db, &loose.id, true).unwrap();
        let input = |dir: &Path, pages: &[&Page]| FolderExportInput {
            folder: dir.to_string_lossy().into_owned(),
            page_ids: pages.iter().map(|p| p.id.clone()).collect(),
        };
        let paths = |plan: &ExportPlan| plan.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();

        // One page, or pages of one group: straight into the folder.
        assert_eq!(paths(&plan_folder_export(&db, &input(out.path(), &[&p1])).unwrap()), ["Pump check.md"]);
        assert_eq!(
            paths(&plan_folder_export(&db, &input(out.path(), &[&p1, &p2])).unwrap()),
            ["Hose check.md", "Pump check.md"]
        );
        // Several groups of one category (one page each is enough): a folder per group.
        assert_eq!(
            paths(&plan_folder_export(&db, &input(out.path(), &[&terms, &p1, &loose])).unwrap()),
            ["Loose.md", "Procedures/Pump check.md", "Reference/Glossary-terms.md"]
        );

        // Two categories: a folder each.
        let two = tempfile::tempdir().unwrap();
        let both = input(two.path(), &[&p1, &loose, &target]);
        let plan = plan_folder_export(&db, &both).unwrap();
        assert_eq!(paths(&plan), ["Learn- notes/Rust/Ownership.md", "Ops/Loose.md", "Ops/Procedures/Pump check.md"]);
        assert_eq!(plan.replace, 0);
        let r = export_folder(&db, both.clone(), vec![]).unwrap();
        assert_eq!((r.written, r.replaced), (3, 0));
        let pump = std::fs::read_to_string(two.path().join("Ops/Procedures/Pump check.md")).unwrap();
        assert!(pump.contains("See [Ownership](../../Learn-%20notes/Rust/Ownership.md)."), "{pump}");
        assert!(pump.contains("group: \"Procedures\""));
        assert!(two.path().join("Ops/Loose.md").is_file(), "archived pages are exported");

        // Again into the same folder: every file is there already, so each must be confirmed.
        let again = plan_folder_export(&db, &both).unwrap();
        assert_eq!(again.replace, 3);
        assert!(matches!(export_folder(&db, both.clone(), vec![]), Err(CoreError::Conflict(_))));
        update_page(&mut db, &p1.id, PagePatch { body: Some("Changed.".into()), ..Default::default() }).unwrap();
        let confirmed = again.files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(export_folder(&db, both, confirmed).unwrap().replaced, 3);
        let pump = std::fs::read_to_string(two.path().join("Ops/Procedures/Pump check.md")).unwrap();
        assert!(pump.ends_with("Changed.\n"));

        // Nothing chosen.
        assert!(matches!(plan_folder_export(&db, &input(out.path(), &[])), Err(CoreError::Invalid(_))));
    }
}
