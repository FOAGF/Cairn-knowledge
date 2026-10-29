//! The boundary between a page's file and its index row (D-72 contract, "The index's own forms").
//!
//! - [`parse`] reads a file: the byte-order mark and line endings, the front matter split into
//!   entries (so unknown keys can be written back as they were, in their order), `tags`,
//!   `source` / `source_info` and `archived`, and the body after it.
//! - [`to_index`] turns the body's relative links into the index's forms: a link to a page file
//!   becomes `[text](page:<id>)`, a link to another file of the folder `folder:<path>` (also an
//!   image, or a page link with a `#heading`), and an Obsidian `[[Page]]` / `[[Page|text]]` link a
//!   page link; `![[image.png]]` becomes an image. Links that don't resolve and web links stay.
//! - [`to_file`] is the reverse, with paths relative to the file (`attachment:` images, from
//!   internal mode or a task, are first copied into the folder by the caller's [`Places`]).
//! - [`render`] puts a file together: the front matter is written back from the parsed entries,
//!   only `tags`, `source` and `archived` being replaced (an entry whose value didn't change keeps
//!   its lines, an empty block goes); the body is the file's own text when its index form didn't
//!   change, else the body written from the index. Line endings and the BOM follow the old file;
//!   new files use `\n`.
//! - [`retarget`] replaces just the links that point to moved or renamed files (and, for a file
//!   that moved itself, its relative links), leaving every other byte as it was.
//!
//! Code (fenced blocks and inline code spans) is never touched, as for mentions.

use std::collections::HashSet;

use super::{dir_of, page_extension};
use crate::knowledge::export::link_encode;
use crate::knowledge::folders::{percent_decode, relative_link, split_list, yaml_scalar};
use crate::mentions::map_prose;

// ---------------------------------------------------------------- reading a file

/// One entry of a front matter block: a `key:` line and the lines that belong to it (a block
/// list, a block scalar, comments). Lines before the first key have no key.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub key: Option<String>,
    /// Without line endings.
    pub lines: Vec<String>,
}

/// A page file, read.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Parsed {
    pub bom: bool,
    pub crlf: bool,
    /// The front matter block as written: from the opening `---` to the end of its closing line.
    pub front_raw: String,
    /// The blank lines between the front matter and the body, as written.
    pub sep_raw: String,
    pub has_front: bool,
    pub entries: Vec<Entry>,
    /// The text after the front matter (and its blank lines), exactly as written.
    pub body_raw: String,
    /// `body_raw` with `\n` line endings: the page text in file form (relative links).
    pub body: String,
    /// As written in the front matter (cleaned like any page's tags by the caller).
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub archived: bool,
}

/// The file isn't UTF-8 text (such files stay out of the index).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NotText;

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Reads a page file's bytes.
pub(crate) fn parse(bytes: &[u8]) -> Result<Parsed, NotText> {
    let text = std::str::from_utf8(bytes).map_err(|_| NotText)?;
    let (bom, text) = match text.strip_prefix('\u{feff}') {
        Some(t) => (true, t),
        None => (false, text),
    };
    let crlf = text.contains("\r\n");
    let mut parsed = Parsed {
        bom,
        crlf,
        front_raw: String::new(),
        sep_raw: String::new(),
        has_front: false,
        entries: Vec::new(),
        body_raw: text.to_string(),
        body: lf(text),
        tags: Vec::new(),
        source: None,
        archived: false,
    };
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return Ok(parsed);
    };
    let open_len = text.len() - rest.len();
    let mut pos = 0;
    let mut close = None;
    for line in rest.split_inclusive('\n') {
        let t = line.trim_end_matches(['\r', '\n']);
        if t == "---" || t == "..." {
            close = Some((pos, pos + line.len()));
            break;
        }
        pos += line.len();
    }
    let Some((inner_end, block_end)) = close else {
        return Ok(parsed);
    };
    let inner = lf(&rest[..inner_end]);
    let after = &rest[block_end..];
    let body_raw = after.trim_start_matches(['\r', '\n']);
    parsed.has_front = true;
    parsed.front_raw = text[..open_len + block_end].to_string();
    parsed.sep_raw = after[..after.len() - body_raw.len()].to_string();
    parsed.body_raw = body_raw.to_string();
    parsed.body = lf(body_raw);
    parsed.entries = entries(&inner);
    let (tags, source, archived) = known_values(&parsed.entries);
    parsed.tags = tags;
    parsed.source = source;
    parsed.archived = archived;
    Ok(parsed)
}

/// Splits a front matter block (without its delimiters) into entries.
fn entries(inner: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for line in inner.lines() {
        let starts_key = !line.starts_with([' ', '\t', '#', '-'])
            && !line.trim().is_empty()
            && line.contains(':');
        if starts_key {
            let key = line.split(':').next().unwrap_or("").trim().to_string();
            out.push(Entry {
                key: Some(key),
                lines: vec![line.to_string()],
            });
        } else if let Some(last) = out.last_mut() {
            last.lines.push(line.to_string());
        } else {
            out.push(Entry {
                key: None,
                lines: vec![line.to_string()],
            });
        }
    }
    out
}

fn key_is(e: &Entry, names: &[&str]) -> bool {
    e.key
        .as_deref()
        .is_some_and(|k| names.iter().any(|n| k.eq_ignore_ascii_case(n)))
}

const TAG_KEYS: &[&str] = &["tags", "tag"];
const SOURCE_KEYS: &[&str] = &["source", "source_info"];
const ARCHIVED_KEYS: &[&str] = &["archived"];

/// The value after `key:` on an entry's first line.
fn first_value(e: &Entry) -> &str {
    e.lines[0].split_once(':').map_or("", |(_, v)| v.trim())
}

fn entry_tags(e: &Entry) -> Vec<String> {
    let value = first_value(e);
    if value.is_empty() {
        return e.lines[1..]
            .iter()
            .filter_map(|l| {
                let t = l.trim();
                let item = t.strip_prefix("- ").or_else(|| (t == "-").then_some(""))?;
                let v = yaml_scalar(item);
                (!v.is_empty()).then_some(v)
            })
            .collect();
    }
    match value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        Some(inner) => split_list(inner),
        None => split_list(value),
    }
}

fn entry_text(e: &Entry) -> Option<String> {
    let value = first_value(e);
    // A block scalar (`|` or `>`): the indented lines that follow.
    if value.starts_with(['|', '>']) {
        let lines: Vec<&str> = e.lines[1..].iter().map(|l| l.trim()).collect();
        let joined = if value.starts_with('|') {
            lines.join("\n")
        } else {
            lines.join(" ")
        };
        let t = joined.trim().to_string();
        return (!t.is_empty()).then_some(t);
    }
    let v = yaml_scalar(value);
    (!v.is_empty()).then_some(v)
}

fn entry_bool(e: &Entry) -> bool {
    matches!(
        yaml_scalar(first_value(e)).to_ascii_lowercase().as_str(),
        "true" | "yes" | "on"
    )
}

fn known_values(entries: &[Entry]) -> (Vec<String>, Option<String>, bool) {
    let tags = entries
        .iter()
        .find(|e| key_is(e, TAG_KEYS))
        .map(entry_tags)
        .unwrap_or_default();
    let source = entries
        .iter()
        .find(|e| key_is(e, SOURCE_KEYS))
        .and_then(entry_text);
    let archived = entries
        .iter()
        .find(|e| key_is(e, ARCHIVED_KEYS))
        .is_some_and(entry_bool);
    (tags, source, archived)
}

// ---------------------------------------------------------------- writing a file

/// The page fields a file carries besides its body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Meta {
    pub tags: Vec<String>,
    pub source: Option<String>,
    pub archived: bool,
}

/// Case-insensitive comparison of tag lists, as pages keep them.
fn same_tags(a: &[String], b: &[String]) -> bool {
    let norm = |v: &[String]| {
        let mut seen = HashSet::new();
        let mut out: Vec<String> = v
            .iter()
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty() && seen.insert(t.clone()))
            .collect();
        out.sort();
        out
    };
    norm(a) == norm(b)
}

/// A YAML scalar written plain when that is safe, else double-quoted (JSON strings are valid YAML).
fn yaml_value(s: &str) -> String {
    let plain = !s.is_empty()
        && s.trim() == s
        && !s.starts_with([
            '-', '?', ':', ',', '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`',
        ])
        && !s.contains(": ")
        && !s.contains(" #")
        && !s.ends_with(':')
        && !s.chars().any(|c| c.is_control())
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
        )
        && s.parse::<f64>().is_err();
    if plain {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
    }
}

/// A tag inside a flow list `[a, b]`: plain unless it has list or quoting characters.
fn yaml_tag(s: &str) -> String {
    if s.contains([',', '[', ']', '{', '}', '"', '\'']) {
        serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
    } else {
        yaml_value(s)
    }
}

/// The front matter for `meta`, keeping the old block's other entries, their order and the lines
/// of values that didn't change. `None`: no block (nothing to write). The text uses `\n`.
fn front_lines(old: Option<&Parsed>, meta: &Meta) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    let (mut tags_done, mut source_done, mut archived_done) = (false, false, false);
    for e in old.map(|p| p.entries.as_slice()).unwrap_or_default() {
        let key = e.key.clone().unwrap_or_default();
        if key_is(e, TAG_KEYS) {
            if tags_done {
                continue;
            }
            tags_done = true;
            if same_tags(&entry_tags(e), &meta.tags) {
                lines.extend(e.lines.iter().cloned());
            } else if !meta.tags.is_empty() {
                lines.push(tags_line(&key, &meta.tags));
            }
        } else if key_is(e, SOURCE_KEYS) {
            if source_done {
                continue;
            }
            source_done = true;
            if entry_text(e) == meta.source {
                lines.extend(e.lines.iter().cloned());
            } else if let Some(s) = &meta.source {
                lines.push(format!("{key}: {}", yaml_value(s)));
            }
        } else if key_is(e, ARCHIVED_KEYS) {
            if archived_done {
                continue;
            }
            archived_done = true;
            if entry_bool(e) == meta.archived {
                // Also `archived: false` written by someone else stays as they wrote it.
                lines.extend(e.lines.iter().cloned());
            } else if meta.archived {
                lines.push(format!("{key}: true"));
            }
        } else {
            lines.extend(e.lines.iter().cloned());
        }
    }
    if !tags_done && !meta.tags.is_empty() {
        lines.push(tags_line("tags", &meta.tags));
    }
    if !source_done && let Some(s) = &meta.source {
        lines.push(format!("source: {}", yaml_value(s)));
    }
    if !archived_done && meta.archived {
        lines.push("archived: true".into());
    }
    if lines.iter().all(|l| l.trim().is_empty()) {
        return None;
    }
    Some(format!("---\n{}\n---\n", lines.join("\n")))
}

fn tags_line(key: &str, tags: &[String]) -> String {
    let items: Vec<String> = tags.iter().map(|t| yaml_tag(t)).collect();
    format!("{key}: [{}]", items.join(", "))
}

fn with_endings(text: &str, crlf: bool) -> String {
    let text = lf(text);
    if crlf { text.replace('\n', "\r\n") } else { text }
}

/// Puts a page file together. `old` is the file as it is now (if any); `body` is the body in file
/// form, or `None` to keep the old file's body exactly as it is.
pub(crate) fn render(old: Option<&Parsed>, meta: &Meta, body: Option<&str>) -> String {
    let crlf = old.is_some_and(|p| p.crlf);
    let mut out = String::new();
    if old.is_some_and(|p| p.bom) {
        out.push('\u{feff}');
    }
    let unchanged_front = old.is_some_and(|p| {
        p.has_front && same_tags(&p.tags, &meta.tags) && p.source == meta.source && p.archived == meta.archived
    });
    let keep_body = body.is_none();
    match (unchanged_front, front_lines(old, meta)) {
        (true, _) => {
            let p = old.expect("checked above");
            out.push_str(&p.front_raw);
            out.push_str(&p.sep_raw);
        }
        (false, Some(front)) => {
            out.push_str(&with_endings(&front, crlf));
            let sep = old
                .filter(|p| p.has_front && !p.sep_raw.is_empty())
                .map_or_else(|| with_endings("\n", crlf), |p| p.sep_raw.clone());
            out.push_str(&sep);
        }
        (false, None) => {}
    }
    match body {
        Some(b) => out.push_str(&with_endings(b, crlf)),
        None => out.push_str(&old.map(|p| p.body_raw.clone()).unwrap_or_default()),
    }
    if !keep_body && !out.ends_with('\n') && !out.is_empty() {
        out.push_str(if crlf { "\r\n" } else { "\n" });
    }
    out
}

// ---------------------------------------------------------------- links

/// One inline link or image in a prose stretch.
#[derive(Debug, Clone)]
pub(crate) struct Link<'a> {
    pub image: bool,
    pub text: &'a str,
    pub target: &'a str,
    /// What follows the target inside the parentheses (a title such as `"Pump"`), trimmed.
    pub title: &'a str,
    /// Where the whole link (with its `!`) starts and ends in the text.
    pub span: std::ops::Range<usize>,
}

/// Calls `f` for every `[text](target)` and `![alt](target)` in `s`; a returned string replaces
/// the whole link.
pub(crate) fn map_links(s: &str, f: &mut dyn FnMut(&Link) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    let mut search = 0;
    while let Some(rel) = s[search..].find('[') {
        let open = search + rel;
        search = open + 1;
        if open > 0 && s.as_bytes()[open - 1] == b'\\' {
            continue;
        }
        if s[open..].starts_with("[[") {
            // A wiki link: see `map_wiki`.
            search = open + 2;
            continue;
        }
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
        let (target, title, end) = if s[inner_start..].starts_with('<') {
            let Some(gt) = s[inner_start..].find('>') else { continue };
            let target = &s[inner_start + 1..inner_start + gt];
            let Some(paren) = s[inner_start + gt..].find(')') else { continue };
            let title = s[inner_start + gt + 1..inner_start + gt + paren].trim();
            (target, title, inner_start + gt + paren + 1)
        } else {
            let Some(paren) = s[inner_start..].find(')') else { continue };
            let inner = &s[inner_start..inner_start + paren];
            if inner.contains('\n') {
                continue;
            }
            let inner = inner.trim();
            let (target, title) = match inner.find(char::is_whitespace) {
                Some(i) => (&inner[..i], inner[i..].trim()),
                None => (inner, ""),
            };
            (target, title, inner_start + paren + 1)
        };
        let image = open > 0 && s.as_bytes()[open - 1] == b'!';
        let start = if image { open - 1 } else { open };
        if start < pos {
            continue;
        }
        let link = Link {
            image,
            text: &s[open + 1..close],
            target,
            title,
            span: start..end,
        };
        if let Some(rep) = f(&link) {
            out.push_str(&s[pos..start]);
            out.push_str(&rep);
            pos = end;
            search = end;
        }
    }
    out.push_str(&s[pos..]);
    out
}

/// Calls `f(embed, inner)` for every `[[inner]]` and `![[inner]]` on one line; a returned string
/// replaces the whole link.
pub(crate) fn map_wiki(s: &str, f: &mut dyn FnMut(bool, &str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    let mut search = 0;
    while let Some(rel) = s[search..].find("[[") {
        let open = search + rel;
        search = open + 2;
        let Some(len) = s[open + 2..].find("]]") else { break };
        let inner = &s[open + 2..open + 2 + len];
        if inner.is_empty() || inner.contains(['\n', '[', ']']) {
            continue;
        }
        let embed = open > 0 && s.as_bytes()[open - 1] == b'!';
        let start = if embed { open - 1 } else { open };
        let end = open + 2 + len + 2;
        if start < pos {
            continue;
        }
        if let Some(rep) = f(embed, inner) {
            out.push_str(&s[pos..start]);
            out.push_str(&rep);
            pos = end;
        }
        search = end;
    }
    out.push_str(&s[pos..]);
    out
}

/// `[text](target "title")` or `![text](...)` again.
fn link_text(image: bool, text: &str, target: &str, title: &str) -> String {
    let bang = if image { "!" } else { "" };
    if title.is_empty() {
        format!("{bang}[{text}]({target})")
    } else {
        format!("{bang}[{text}]({target} {title})")
    }
}

/// The root-relative path a relative link target names, seen from a file in `dir`, and its
/// `#part`. `None` for web addresses and other schemes (also Cairn's `page:` and `folder:`),
/// anchors, absolute paths and paths that leave the root.
pub(crate) fn resolve(dir: &str, target: &str) -> Option<(String, Option<String>)> {
    let (path, frag) = match target.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string())),
        None => (target, None),
    };
    if path.is_empty()
        || path.contains("://")
        || path.starts_with(['/', '\\'])
        || path
            .split_once(':')
            .is_some_and(|(scheme, _)| !scheme.contains(['/', '\\', '.']))
    {
        return None;
    }
    let mut parts: Vec<String> = dir
        .split('/')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    for part in percent_decode(path).split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p.to_string()),
        }
    }
    let joined = parts.join("/");
    super::valid_rel(&joined).then_some((joined, frag))
}

/// `folder:` form of a root-relative path: each part percent-encoded where Markdown needs it.
pub(crate) fn encode_path(path: &str) -> String {
    path.split('/').map(link_encode).collect::<Vec<_>>().join("/")
}

/// The root-relative path in a `folder:` target, and its `#part`; `None` unless it is a path
/// inside the root (`valid_rel`: no `..`, `\`, drive or root prefixes).
pub(crate) fn folder_target(target: &str) -> Option<(String, Option<String>)> {
    let rest = target.strip_prefix("folder:")?;
    let (p, frag) = match rest.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string())),
        None => (rest, None),
    };
    let p = percent_decode(p);
    super::valid_rel(&p).then_some((p, frag))
}

/// The relative link from a file in `from_dir` to `to` (root-relative), with its `#part`.
pub(crate) fn relative(from_dir: &str, to: &str, frag: Option<&str>) -> String {
    let from: Vec<String> = from_dir
        .split('/')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    let to: Vec<String> = to.split('/').map(str::to_string).collect();
    let mut link = relative_link(&from, &to);
    if let Some(f) = frag {
        link.push('#');
        link.push_str(f);
    }
    link
}

/// What [`to_index`] needs to know about the folder.
pub(crate) trait Lookup {
    /// The id of the page whose file is at `path`.
    fn page_at(&self, path: &str) -> Option<String>;
    /// Whether a file or folder is at `path`.
    fn exists(&self, path: &str) -> bool;
    /// The page an Obsidian link `[[name]]` names, seen from the page at `here`: its id.
    fn page_named(&self, name: &str, here: &str) -> Option<String>;
    /// The file an embed `![[name]]` names, seen from the page at `here`: its path.
    fn file_named(&self, name: &str, here: &str) -> Option<String>;
}

/// The body of the page file at `here` (file form, `\n` line endings) in the index's forms.
pub(crate) fn to_index(body: &str, here: &str, look: &dyn Lookup) -> String {
    let dir = dir_of(here);
    map_prose(body, |seg| {
        let seg = map_wiki(seg, &mut |embed, inner| {
            let (name, alias) = match inner.split_once('|') {
                Some((n, a)) => (n.trim(), Some(a.trim())),
                None => (inner.trim(), None),
            };
            if name.contains('#') {
                return None;
            }
            if embed {
                let path = look.file_named(name, here)?;
                let alt = alias.unwrap_or(name);
                return Some(format!("![{alt}](folder:{})", encode_path(&path)));
            }
            let id = look.page_named(name, here)?;
            let text = alias.unwrap_or_else(|| name.rsplit('/').next().unwrap_or(name));
            Some(format!("[{text}](page:{id})"))
        });
        map_links(&seg, &mut |l| {
            let (path, frag) = resolve(dir, l.target)?;
            if !l.image
                && frag.is_none()
                && page_extension(super::name_of(&path)).is_some()
                && let Some(id) = look.page_at(&path)
            {
                return Some(format!("[{}](page:{id})", l.text));
            }
            if look.exists(&path) || look.page_at(&path).is_some() {
                let mut t = format!("folder:{}", encode_path(&path));
                if let Some(f) = frag {
                    t.push('#');
                    t.push_str(&f);
                }
                return Some(link_text(l.image, l.text, &t, l.title));
            }
            None
        })
    })
}

/// What [`to_file`] needs to know.
pub(crate) trait Places {
    /// The path of the page with this id, if it has a file.
    fn page_path(&self, id: &str) -> Option<String>;
    /// An `attachment:<name>` image of Cairn's data folder, copied into the folder: its path.
    fn attachment(&mut self, name: &str) -> Option<String>;
}

/// The index body of the page at `here` in file form: page links and `folder:` links as paths
/// relative to the file. A link to a page without a file becomes its text.
pub(crate) fn to_file(body: &str, here: &str, places: &mut dyn Places) -> String {
    let dir = dir_of(here);
    map_prose(body, |seg| {
        map_links(seg, &mut |l| {
            if let Some(id) = l.target.strip_prefix("page:") {
                return Some(match places.page_path(id) {
                    Some(p) => format!("[{}]({})", l.text, relative(dir, &p, None)),
                    None => l.text.to_string(),
                });
            }
            if let Some((p, frag)) = folder_target(l.target) {
                let t = relative(dir, &p, frag.as_deref());
                return Some(link_text(l.image, l.text, &t, l.title));
            }
            if let Some(name) = l.target.strip_prefix("attachment:") {
                let path = places.attachment(name)?;
                return Some(link_text(l.image, l.text, &relative(dir, &path, None), l.title));
            }
            None
        })
    })
}

/// The links a body (index form) makes to other files: `folder:` paths.
pub(crate) fn folder_paths(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    map_prose(body, |seg| {
        map_links(seg, &mut |l| {
            if let Some((p, _)) = folder_target(l.target) {
                out.push(p);
            }
            None
        });
        String::new()
    });
    out
}

/// For moves and renames: the file's text (as written) with only the links changed that must
/// change. `old_here` / `new_here` are the file's own path before and after (the same when only
/// other files moved); `moved` maps a path (or a path inside a moved folder) to its new place;
/// `retitle` gives the new title for a link whose text was the moved page's old title; `wiki`
/// gives the new name for a `[[name]]` link (the text's `n`th, counted from 0) that must name its
/// page differently. `None`: nothing to change.
pub(crate) fn retarget(
    text: &str,
    old_here: &str,
    new_here: &str,
    moved: &dyn Fn(&str) -> Option<String>,
    retitle: &dyn Fn(&str, &str) -> Option<String>,
    wiki: &dyn Fn(usize, &str) -> Option<String>,
) -> Option<String> {
    let (old_dir, new_dir) = (dir_of(old_here), dir_of(new_here));
    let mut nth = 0;
    let out = map_prose(text, |seg| {
        let seg = map_wiki(seg, &mut |embed, inner| {
            let cut = inner.find(['|', '#']).unwrap_or(inner.len());
            let (name, rest) = (inner[..cut].trim(), &inner[cut..]);
            nth += 1;
            let new_name = wiki(nth - 1, name).filter(|n| n != name)?;
            Some(format!("{}[[{new_name}{rest}]]", if embed { "!" } else { "" }))
        });
        map_links(&seg, &mut |l| {
            let (path, frag) = resolve(old_dir, l.target)?;
            let new_path = moved(&path);
            if new_path.is_none() && old_dir == new_dir {
                return None;
            }
            let target = relative(new_dir, new_path.as_deref().unwrap_or(&path), frag.as_deref());
            let text = retitle(&path, l.text).unwrap_or_else(|| l.text.to_string());
            let decoded_same = percent_decode(l.target) == percent_decode(&target);
            if decoded_same && text == l.text {
                return None;
            }
            Some(link_text(l.image, &text, &target, l.title))
        })
    });
    (out != text).then_some(out)
}

/// One link of a text, as [`link_spans`] finds it.
struct LinkSpan {
    span: std::ops::Range<usize>,
    /// The page it links to (`page:<id>`), in an index body.
    page: Option<String>,
    /// A `[[…]]` link (else a Markdown link or image).
    wiki: bool,
}

/// The links of a body (a file's or the index's) in the order of the text, code aside.
fn link_spans(body: &str) -> Vec<LinkSpan> {
    let mut out = Vec::new();
    map_prose(body, |seg| {
        // `map_prose` hands over parts of `body`.
        let at = seg.as_ptr() as usize - body.as_ptr() as usize;
        map_wiki(seg, &mut |embed, inner| {
            let open = inner.as_ptr() as usize - seg.as_ptr() as usize - 2 - usize::from(embed);
            let span = at + open..at + open + inner.len() + 4 + usize::from(embed);
            out.push(LinkSpan { span, page: None, wiki: true });
            None
        });
        map_links(seg, &mut |l| {
            let page = l.target.strip_prefix("page:").map(str::to_string);
            out.push(LinkSpan { span: at + l.span.start..at + l.span.end, page, wiki: false });
            None
        });
        String::new()
    });
    out.sort_by_key(|l| l.span.start);
    out
}

/// The page each `[[…]]` link of a file's text (`file_body`) finds as the index has it
/// (`index_body`, read from that text), in order; `None` when their links don't pair up.
pub(crate) fn wiki_targets(file_body: &str, index_body: &str) -> Option<Vec<Option<String>>> {
    let (file, index) = (link_spans(file_body), link_spans(index_body));
    (file.len() == index.len())
        .then(|| file.iter().zip(index).filter(|(f, _)| f.wiki).map(|(_, i)| i.page).collect())
}

/// `new` (a file's text just turned into the index's forms) with the page links of `old` (the
/// index's form of the same text, from before) put back where `keep(old page, new page)` says
/// so. Both come from one text, so their links pair up in order; `new` as it is when they don't.
pub(crate) fn keep_links(old: &str, new: &str, keep: &dyn Fn(&str, Option<&str>) -> bool) -> String {
    let (was, now) = (link_spans(old), link_spans(new));
    if was.len() != now.len() {
        return new.to_string();
    }
    let mut out = String::with_capacity(new.len());
    let mut pos = 0;
    for (w, n) in was.iter().zip(&now) {
        let Some(page) = &w.page else { continue };
        if n.page.as_ref() == Some(page) || !keep(page, n.page.as_deref()) {
            continue;
        }
        if n.span.start < pos {
            return new.to_string();
        }
        out.push_str(&new[pos..n.span.start]);
        out.push_str(&old[w.span.clone()]);
        pos = n.span.end;
    }
    out.push_str(&new[pos..]);
    out
}

#[cfg(test)]
mod tests {
    use super::super::title_of;
    use super::*;
    use std::collections::HashMap;

    struct Folder {
        pages: HashMap<String, String>,
        files: Vec<String>,
    }

    impl Folder {
        fn new() -> Folder {
            let pages: HashMap<String, String> = [
                ("Ops/Pumps.md", "p-pumps"),
                ("Ops/Hoses/Hose check.md", "p-hose"),
                ("Notes.md", "p-notes"),
                ("Learn/Rust ownership.markdown", "p-rust"),
            ]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
            Folder {
                pages,
                files: vec!["Ops/attachments/pump 1.png".into(), "Ops/spec.pdf".into()],
            }
        }
    }

    impl Lookup for Folder {
        fn page_at(&self, path: &str) -> Option<String> {
            self.pages.get(path).cloned()
        }
        fn exists(&self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
        fn page_named(&self, name: &str, _here: &str) -> Option<String> {
            self.pages
                .iter()
                .find(|(p, _)| title_of(p).eq_ignore_ascii_case(name))
                .map(|(_, id)| id.clone())
        }
        fn file_named(&self, name: &str, _here: &str) -> Option<String> {
            self.files.iter().find(|f| super::super::name_of(f) == name).cloned()
        }
    }

    impl Places for Folder {
        fn page_path(&self, id: &str) -> Option<String> {
            self.pages.iter().find(|(_, i)| *i == id).map(|(p, _)| p.clone())
        }
        fn attachment(&mut self, name: &str) -> Option<String> {
            Some(format!("Ops/attachments/{name}"))
        }
    }

    fn meta(tags: &[&str], source: Option<&str>, archived: bool) -> Meta {
        Meta {
            tags: tags.iter().map(|t| t.to_string()).collect(),
            source: source.map(str::to_string),
            archived,
        }
    }

    #[test]
    fn front_matter_keeps_unknown_keys_in_order() {
        let text = "---\ntitle: Pumps\ntags:\n  - pumps\n  - 'safety, site'\nowner: Kim # the lead\nsource: \"Manual v3\"\n---\n\n# Pumps\n";
        let p = parse(text.as_bytes()).unwrap();
        assert!(p.has_front && !p.bom);
        assert_eq!(p.tags, ["pumps", "safety, site"]);
        assert_eq!(p.source.as_deref(), Some("Manual v3"));
        assert!(!p.archived);
        assert_eq!(p.body, "# Pumps\n");
        // Nothing changed: the file comes back byte for byte.
        let same = meta(&["Pumps", "safety, site"], Some("Manual v3"), false);
        assert_eq!(render(Some(&p), &same, None), text);
        // Archived: one line added at the end, the other entries untouched.
        let out = render(Some(&p), &meta(&["pumps", "safety, site"], Some("Manual v3"), true), None);
        assert_eq!(
            out,
            "---\ntitle: Pumps\ntags:\n  - pumps\n  - 'safety, site'\nowner: Kim # the lead\nsource: \"Manual v3\"\narchived: true\n---\n\n# Pumps\n"
        );
        // New tags replace the block list in place; no source removes its entry.
        let out = render(Some(&p), &meta(&["pumps", "c#"], None, false), None);
        assert_eq!(
            out,
            "---\ntitle: Pumps\ntags: [pumps, c#]\nowner: Kim # the lead\n---\n\n# Pumps\n"
        );
    }

    #[test]
    fn a_block_that_becomes_empty_goes_and_new_files_get_one_only_when_needed() {
        let p = parse(b"---\ntags: [a]\n---\nText\n").unwrap();
        assert_eq!(render(Some(&p), &meta(&[], None, false), None), "Text\n");
        assert_eq!(render(None, &meta(&[], None, false), Some("Body\n")), "Body\n");
        assert_eq!(
            render(None, &meta(&["知识"], Some("Site visit: 2026"), false), Some("Body\n")),
            "---\ntags: [知识]\nsource: \"Site visit: 2026\"\n---\n\nBody\n"
        );
        // An unclosed block is body text.
        let p = parse(b"---\nnot closed\n").unwrap();
        assert!(!p.has_front);
        assert_eq!(p.body, "---\nnot closed\n");
    }

    #[test]
    fn line_endings_and_the_bom_are_kept() {
        let p = parse("\u{feff}---\r\ntags: [x]\r\n---\r\n\r\nLine one\r\nLine two\r\n".as_bytes()).unwrap();
        assert!(p.bom && p.crlf);
        assert_eq!(p.body, "Line one\nLine two\n");
        let out = render(Some(&p), &meta(&["x", "y"], None, false), Some("New\nText\n"));
        assert_eq!(out, "\u{feff}---\r\ntags: [x, y]\r\n---\r\n\r\nNew\r\nText\r\n");
        assert!(parse(&[0xff, 0xfe, 0x41]).is_err());
    }

    #[test]
    fn relative_links_become_index_forms_and_back() {
        let mut f = Folder::new();
        let here = "Ops/Hoses/Hose check.md";
        let body = "See [the pumps](../Pumps.md), [notes](../../Notes.md), [[Rust ownership]], \
                    [[Notes|my notes]], ![pump](../attachments/pump%201.png), [spec](../spec.pdf \"Spec\"), \
                    [part](../Pumps.md#Intake), [gone](Nope.md), [web](https://x.org) and `[code](../Pumps.md)`.\n";
        let index = to_index(body, here, &f);
        assert_eq!(
            index,
            "See [the pumps](page:p-pumps), [notes](page:p-notes), [Rust ownership](page:p-rust), \
             [my notes](page:p-notes), ![pump](folder:Ops/attachments/pump%201.png), [spec](folder:Ops/spec.pdf \"Spec\"), \
             [part](folder:Ops/Pumps.md#Intake), [gone](Nope.md), [web](https://x.org) and `[code](../Pumps.md)`.\n"
        );
        let back = to_file(&index, here, &mut f);
        assert_eq!(
            back,
            "See [the pumps](../Pumps.md), [notes](../../Notes.md), [Rust ownership](../../Learn/Rust%20ownership.markdown), \
             [my notes](../../Notes.md), ![pump](../attachments/pump%201.png), [spec](../spec.pdf \"Spec\"), \
             [part](../Pumps.md#Intake), [gone](Nope.md), [web](https://x.org) and `[code](../Pumps.md)`.\n"
        );
        // Reading what was written gives the same index body.
        assert_eq!(to_index(&back, here, &f), index);
        // A page without a file is written as its text; a pasted image is copied.
        assert_eq!(
            to_file("[Old](page:zz) ![i](attachment:ab.png)", "Ops/x.md", &mut f),
            "Old ![i](attachments/ab.png)"
        );
        // Embeds.
        assert_eq!(to_index("![[pump 1.png]]", "Ops/x.md", &f), "![pump 1.png](folder:Ops/attachments/pump%201.png)");
    }

    #[test]
    fn links_are_retargeted_and_nothing_else_moves() {
        let text = "Intro *keep* this  \r\n[Hose](Hoses/Hose%20check.md) and [[Hose check|hoses]] and ![p](attachments/pump%201.png)\r\n`[Hose](Hoses/Hose check.md)`\r\n";
        let moved = |p: &str| (p == "Ops/Hoses/Hose check.md").then(|| "Ops/Hoses/Hose test.md".to_string());
        let retitle = |p: &str, t: &str| (p == "Ops/Hoses/Hose check.md" && t == "Hose check").then(|| "Hose test".to_string());
        let wiki = |_: usize, n: &str| (n == "Hose check").then(|| "Hose test".to_string());
        let out = retarget(text, "Ops/Pumps.md", "Ops/Pumps.md", &moved, &retitle, &wiki).unwrap();
        assert_eq!(
            out,
            "Intro *keep* this  \r\n[Hose](Hoses/Hose%20test.md) and [[Hose test|hoses]] and ![p](attachments/pump%201.png)\r\n`[Hose](Hoses/Hose check.md)`\r\n"
        );
        // The file itself moves one folder down: its own relative links follow.
        let none = |_: &str| None;
        let no_title = |_: &str, _: &str| None;
        let no_wiki = |_: usize, _: &str| None;
        let out = retarget("[n](../Notes.md) [w](https://x.org)\n", "Ops/Pumps.md", "Ops/Sub/Pumps.md", &none, &no_title, &no_wiki).unwrap();
        assert_eq!(out, "[n](../../Notes.md) [w](https://x.org)\n");
        assert_eq!(retarget("[n](../Notes.md)\n", "Ops/a.md", "Ops/b.md", &none, &no_title, &no_wiki), None);
    }
}
