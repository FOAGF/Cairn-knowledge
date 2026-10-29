//! The LLM prompt templates in `crates/cairn-core/prompts/` and their rendering (M5, D-41).
//!
//! The files' format, rendering rules and list formats are in `prompts/README.md`; this module
//! follows it exactly, and `poc/h-create/prompts.py` renders the same files the same way (the
//! `golden` test compares the two). The templates are written by a Fable sub-agent and checked on
//! the owner's models with `poc/h-create/llm_check.py` before use (owner's rule, 2026-10-15).
//!
//! A builder per template returns the messages for the sidecar's `llm.complete`
//! (`sidecar/PROTOCOL.md`); the Tauri layer sends them. Builders never touch the database: the
//! caller passes the values (glossary lines from [`crate::glossary::prompt_entries`]).

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;

use crate::glossary::PromptEntry;
use crate::{CoreError, Result};

// ---------------------------------------------------------------- messages

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// One content block of a message with images (PROTOCOL.md `llm.complete`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Text { text: String },
    /// An image file the sidecar reads itself (`file.extract` output).
    Image { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Blocks(Vec<Block>),
}

/// Which template a message was rendered from, for the one-line LLM call log (NFR-SEC-3). It
/// never goes to the sidecar and never counts in comparisons.
#[derive(Debug, Clone, Copy, Default)]
pub struct FromTemplate(pub Option<&'static str>);

impl PartialEq for FromTemplate {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl Eq for FromTemplate {}

/// A message as `llm.complete` takes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LlmMessage {
    pub role: Role,
    pub content: Content,
    #[serde(skip)]
    pub template: FromTemplate,
}

impl LlmMessage {
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: Content::Text(text.into()),
            template: FromTemplate::default(),
        }
    }

    /// The text of a plain message, or of its first text block.
    pub fn text(&self) -> &str {
        match &self.content {
            Content::Text(t) => t,
            Content::Blocks(blocks) => blocks
                .iter()
                .find_map(|b| match b {
                    Block::Text { text } => Some(text.as_str()),
                    Block::Image { .. } => None,
                })
                .unwrap_or(""),
        }
    }
}

// ---------------------------------------------------------------- templates

/// Which model a template runs on (README "Format", FR-KN-UP-9, FR-LLM-AS-6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRole {
    /// The model picked in the conversation (FR-LLM-ST-2).
    Conversation,
    /// The default background model (Settings › Models & defaults).
    Default,
    /// The OCR model (falls back as `llm::resolve` in the Tauri layer describes).
    Ocr,
}

/// What the reply is (README "Format").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    Page,
    Json,
    Text,
    PageOrText,
}

/// A parsed template file with its fragments expanded.
#[derive(Debug)]
pub struct Template {
    pub id: &'static str,
    pub model: ModelRole,
    pub output: Output,
    pub stream: bool,
    variables: Vec<String>,
    /// (role, text) in file order; text still holds the `{{name}}` placeholders.
    messages: Vec<(Role, String)>,
}

const TEMPLATES: &[(&str, &str)] = &[
    ("ocr", include_str!("../prompts/ocr.md")),
    ("draft-file", include_str!("../prompts/draft-file.md")),
    ("draft-part", include_str!("../prompts/draft-part.md")),
    ("draft-combine", include_str!("../prompts/draft-combine.md")),
    ("revise", include_str!("../prompts/revise.md")),
    ("task-draft", include_str!("../prompts/task-draft.md")),
    ("check", include_str!("../prompts/check.md")),
    ("check-retry", include_str!("../prompts/check-retry.md")),
    ("keep-one-topic", include_str!("../prompts/keep-one-topic.md")),
    ("suggest-merge", include_str!("../prompts/suggest-merge.md")),
    ("suggest-groups", include_str!("../prompts/suggest-groups.md")),
    // M6, the assistant (D-44).
    ("ask-plan", include_str!("../prompts/ask-plan.md")),
    ("ask-answer", include_str!("../prompts/ask-answer.md")),
    ("ask-action", include_str!("../prompts/ask-action.md")),
    ("ask-page", include_str!("../prompts/ask-page.md")),
    ("edit-page", include_str!("../prompts/edit-page.md")),
    ("doc-terms", include_str!("../prompts/doc-terms.md")),
    ("ask-web", include_str!("../prompts/ask-web.md")),
];

const FRAGMENTS: &[(&str, &str)] = &[
    ("page-rules", include_str!("../prompts/_page-rules.md")),
    ("glossary", include_str!("../prompts/_glossary.md")),
    ("term-rules", include_str!("../prompts/_term-rules.md")),
    ("answer-rules", include_str!("../prompts/_answer-rules.md")),
    ("cite-rules", include_str!("../prompts/_cite-rules.md")),
];

/// The sentence `excerpt_note` holds when the source is excerpts (README "Other conventions").
pub const EXCERPT_NOTE: &str = "These are excerpts of a longer text, picked by meaning for the instruction or request because the whole text is too long to send; \"[...]\" marks where text was left out. Work from the excerpts only.";

/// The line between two excerpts.
pub const EXCERPT_GAP: &str = "[...]";

fn template_error(id: &str, what: impl std::fmt::Display) -> CoreError {
    CoreError::Invalid(format!("Prompt template \"{id}\" is broken: {what}"))
}

/// `{{name}}` / `{{> name}}` occurrences: (start, end, name, is_include). Spaces inside the braces
/// are allowed, as in the Python renderer.
fn tags(text: &str) -> Vec<(usize, usize, &str, bool)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find("{{") {
        let start = from + rel;
        let Some(close_rel) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + close_rel + 2;
        let inner = text[start + 2..end - 2].trim();
        let (include, name) = match inner.strip_prefix('>') {
            Some(rest) => (true, rest.trim()),
            None => (false, inner),
        };
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            out.push((start, end, name, include));
            from = end;
        } else {
            from = start + 2;
        }
    }
    out
}

fn parse(id: &'static str, raw: &str) -> Result<Template> {
    // Git may check templates out with CRLF on Windows; delimiters and rendered text use LF.
    let raw = raw.replace("\r\n", "\n");
    let rest = raw
        .strip_prefix("---\n")
        .ok_or_else(|| template_error(id, "no front matter"))?;
    let end = rest
        .find("\n---\n")
        .ok_or_else(|| template_error(id, "front matter not closed"))?;
    let (front, body) = (&rest[..end], &rest[end + 5..]);

    let mut meta: HashMap<&str, &str> = HashMap::new();
    let mut variables = Vec::new();
    for line in front.lines() {
        if let Some(item) = line.strip_prefix("  - ") {
            let name = item.split(':').next().unwrap_or("").trim();
            variables.push(name.to_string());
        } else if !line.trim().is_empty() {
            let (k, v) = line.split_once(':').unwrap_or((line, ""));
            meta.insert(k.trim(), v.trim());
        }
    }
    if meta.get("variables") == Some(&"[]") {
        variables.clear();
    }
    if meta.get("id") != Some(&id) {
        return Err(template_error(id, "its id doesn't match the file name"));
    }
    let model = match meta.get("model").copied() {
        Some("conversation") => ModelRole::Conversation,
        Some("default") => ModelRole::Default,
        Some("ocr") => ModelRole::Ocr,
        other => return Err(template_error(id, format!("unknown model {other:?}"))),
    };
    let output = match meta.get("output").copied() {
        Some("page") => Output::Page,
        Some("json") => Output::Json,
        Some("text") => Output::Text,
        Some("page-or-text") => Output::PageOrText,
        other => return Err(template_error(id, format!("unknown output {other:?}"))),
    };
    let stream = meta.get("stream") == Some(&"true");

    // 1. Includes: the fragment's content as it is.
    let mut expanded = String::with_capacity(body.len() * 2);
    let mut last = 0;
    for (start, end, name, include) in tags(body) {
        if !include {
            continue;
        }
        let fragment = FRAGMENTS
            .iter()
            .find(|(n, _)| *n == name)
            .ok_or_else(|| template_error(id, format!("no fragment _{name}.md")))?
            .1;
        expanded.push_str(&body[last..start]);
        expanded.push_str(&fragment.replace("\r\n", "\n"));
        last = end;
    }
    expanded.push_str(&body[last..]);

    // Messages: a marker line starts each one.
    let mut messages: Vec<(Role, String)> = Vec::new();
    let mut before = String::new();
    for line in expanded.split_inclusive('\n') {
        let marker = match line.trim_end_matches(['\n', ' ', '\t']) {
            "<<<system>>>" => Some(Role::System),
            "<<<user>>>" => Some(Role::User),
            _ => None,
        };
        match (marker, messages.last_mut()) {
            (Some(role), _) => messages.push((role, String::new())),
            (None, Some((_, text))) => text.push_str(line),
            (None, None) => before.push_str(line),
        }
    }
    if !before.trim().is_empty() {
        return Err(template_error(id, "text before the first message marker"));
    }
    for (_, text) in &mut messages {
        *text = text.trim_matches('\n').to_string();
    }
    for (_, text) in &messages {
        for (_, _, name, include) in tags(text) {
            if include {
                return Err(template_error(id, "a fragment includes another fragment"));
            }
            if !variables.iter().any(|v| v == name) {
                return Err(template_error(id, format!("{{{{{name}}}}} is not declared")));
            }
        }
    }
    Ok(Template {
        id,
        model,
        output,
        stream,
        variables,
        messages,
    })
}

/// Every template, parsed once.
fn all() -> &'static Vec<Template> {
    static ALL: OnceLock<Vec<Template>> = OnceLock::new();
    ALL.get_or_init(|| {
        TEMPLATES
            .iter()
            .map(|(id, raw)| parse(id, raw).unwrap_or_else(|e| panic!("{e}")))
            .collect()
    })
}

/// The template `id`. Panics on an unknown id: ids are constants in the code.
pub fn template(id: &str) -> &'static Template {
    all()
        .iter()
        .find(|t| t.id == id)
        .unwrap_or_else(|| panic!("no prompt template \"{id}\""))
}

impl Template {
    /// The messages with `values` inserted in one pass (README "Rendering rules" 2–4).
    pub fn render(&self, values: &[(&str, &str)]) -> Result<Vec<LlmMessage>> {
        for (name, _) in values {
            if !self.variables.iter().any(|v| v == name) {
                return Err(template_error(self.id, format!("{name} is not declared")));
            }
        }
        for v in &self.variables {
            if !values.iter().any(|(name, _)| name == v) {
                return Err(template_error(self.id, format!("{v} was not passed")));
            }
        }
        Ok(self
            .messages
            .iter()
            .map(|(role, text)| {
                let mut out = String::with_capacity(text.len());
                let mut last = 0;
                for (start, end, name, _) in tags(text) {
                    out.push_str(&text[last..start]);
                    // Declared and passed: checked above and at parse time.
                    out.push_str(values.iter().find(|(n, _)| *n == name).map_or("", |(_, v)| v));
                    last = end;
                }
                out.push_str(&text[last..]);
                let mut m = LlmMessage::new(*role, out.trim_matches('\n'));
                m.template = FromTemplate(Some(self.id));
                m
            })
            .collect())
    }
}

// ---------------------------------------------------------------- list variables (README table)

/// A group as the `groups` variable lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupLine {
    pub name: String,
    pub description: String,
    /// `suggest-groups`: proposed by an earlier batch of the same run.
    pub proposed: bool,
}

/// `- Term (Other, Other) [global]: description` per entry, or `[category: Name]`.
pub fn glossary_lines(entries: &[PromptEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let other = if e.other_names.is_empty() {
                String::new()
            } else {
                format!(" ({})", e.other_names.join(", "))
            };
            let scope = match &e.category_name {
                None => "global".to_string(),
                Some(name) => format!("category: {name}"),
            };
            let description = e.description.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("- {}{other} [{scope}]: {description}", e.term)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn groups_lines(groups: &[GroupLine]) -> String {
    groups
        .iter()
        .map(|g| {
            let mut line = format!("- {}", g.name);
            if !g.description.trim().is_empty() {
                line.push_str(&format!(": {}", g.description.trim()));
            }
            if g.proposed {
                line.push_str(if g.description.trim().is_empty() {
                    ": (proposed in this run)"
                } else {
                    " (proposed in this run)"
                });
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `[YYYY-MM-DD] text`, oldest first; `comments` are (date, text) with the date already in that form.
pub fn comments_lines(comments: &[(String, String)]) -> String {
    comments
        .iter()
        .map(|(date, text)| format!("[{date}] {text}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn topics_lines(topics: &[String]) -> String {
    topics
        .iter()
        .map(|t| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parts_block(drafts: &[String]) -> String {
    let n = drafts.len();
    drafts
        .iter()
        .enumerate()
        .map(|(i, d)| format!("Part {} of {n}:\n<part>\n{d}\n</part>", i + 1))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Characters of a page body shown in `suggest-groups`.
pub const PAGE_START_CHARS: usize = 300;

/// `- [id] Title: start of body`; `pages` are (batch id, title, body).
pub fn pages_lines(pages: &[(String, String, String)]) -> String {
    pages
        .iter()
        .map(|(id, title, body)| {
            let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
            let start: String = flat.chars().take(PAGE_START_CHARS).collect();
            format!("- [{id}] {title}: {}", start.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------- builders, one per template

fn note(excerpts: bool) -> &'static str {
    if excerpts { EXCERPT_NOTE } else { "" }
}

/// `ocr`: the rendered text first, then one image block per page image, in order.
pub fn ocr(image_paths: &[String]) -> Result<Vec<LlmMessage>> {
    let mut messages = template("ocr").render(&[])?;
    if let Some(last) = messages.last_mut() {
        let mut blocks = vec![Block::Text {
            text: last.text().to_string(),
        }];
        blocks.extend(image_paths.iter().map(|p| Block::Image { path: p.clone() }));
        last.content = Content::Blocks(blocks);
    }
    Ok(messages)
}

pub fn draft_file(
    file_name: &str,
    source: &str,
    instruction: &str,
    excerpts: bool,
    glossary: &str,
) -> Result<Vec<LlmMessage>> {
    template("draft-file").render(&[
        ("file_name", file_name),
        ("source", source),
        ("instruction", instruction.trim()),
        ("excerpt_note", note(excerpts)),
        ("glossary", glossary),
    ])
}

pub fn draft_part(
    file_name: &str,
    index: usize,
    count: usize,
    source: &str,
    glossary: &str,
) -> Result<Vec<LlmMessage>> {
    template("draft-part").render(&[
        ("file_name", file_name),
        ("part_index", &index.to_string()),
        ("part_count", &count.to_string()),
        ("source", source),
        ("glossary", glossary),
    ])
}

pub fn draft_combine(file_name: &str, parts: &[String], glossary: &str) -> Result<Vec<LlmMessage>> {
    template("draft-combine").render(&[
        ("file_name", file_name),
        ("part_count", &parts.len().to_string()),
        ("parts", &parts_block(parts)),
        ("glossary", glossary),
    ])
}

/// `revise`: system and source from the template, then the current draft, the earlier turns about
/// it (user and assistant messages, in order), and the new request as typed.
#[allow(clippy::too_many_arguments)]
pub fn revise(
    file_name: &str,
    source: &str,
    instruction: &str,
    excerpts: bool,
    glossary: &str,
    draft: &str,
    earlier: &[LlmMessage],
    request: &str,
) -> Result<Vec<LlmMessage>> {
    let mut messages = template("revise").render(&[
        ("file_name", file_name),
        ("source", source),
        ("instruction", instruction.trim()),
        ("excerpt_note", note(excerpts)),
        ("glossary", glossary),
    ])?;
    messages.push(LlmMessage::new(Role::Assistant, draft));
    messages.extend(earlier.iter().cloned());
    messages.push(LlmMessage::new(Role::User, request));
    Ok(messages)
}

pub fn task_draft(
    task_key: &str,
    task_title: &str,
    category: &str,
    description: &str,
    comments: &[(String, String)],
    glossary: &str,
) -> Result<Vec<LlmMessage>> {
    template("task-draft").render(&[
        ("task_key", task_key),
        ("task_title", task_title),
        ("category", category),
        ("description", description),
        ("comments", &comments_lines(comments)),
        ("glossary", glossary),
    ])
}

pub fn check(category: &str, groups: &[GroupLine], glossary: &str, page: &str) -> Result<Vec<LlmMessage>> {
    template("check").render(&[
        ("category", category),
        ("groups", &groups_lines(groups)),
        ("glossary", glossary),
        ("page", page),
    ])
}

/// The retry after an unreadable JSON reply: append the bad reply as an assistant message, then this.
pub fn check_retry() -> Result<LlmMessage> {
    let mut messages = template("check-retry").render(&[])?;
    messages
        .pop()
        .ok_or_else(|| template_error("check-retry", "no message"))
}

pub fn keep_one_topic(topic: &str, topics: &[String], draft: &str, glossary: &str) -> Result<Vec<LlmMessage>> {
    template("keep-one-topic").render(&[
        ("topic", topic),
        ("topics", &topics_lines(topics)),
        ("draft", draft),
        ("glossary", glossary),
    ])
}

pub fn suggest_merge(existing: &str, new_content: &str, glossary: &str) -> Result<Vec<LlmMessage>> {
    template("suggest-merge").render(&[
        ("existing", existing),
        ("new_content", new_content),
        ("glossary", glossary),
    ])
}

pub fn suggest_groups(
    category: &str,
    groups: &[GroupLine],
    pages: &[(String, String, String)],
    glossary: &str,
) -> Result<Vec<LlmMessage>> {
    template("suggest-groups").render(&[
        ("category", category),
        ("groups", &groups_lines(groups)),
        ("pages", &pages_lines(pages)),
        ("glossary", glossary),
    ])
}

// ---------------------------------------------------------------- M6: the assistant (D-44)
//
// The list variables come formatted from `crate::assistant::formats`. For `ask-answer`,
// `ask-action`, `ask-page` and `edit-page` the earlier turns go between the system message and the
// rendered user message (README "What the app appends"; `formats::history_messages`).

/// The rendered `[system, user]` with `history` in between.
fn with_history(rendered: Vec<LlmMessage>, history: &[LlmMessage]) -> Vec<LlmMessage> {
    let mut out = Vec::with_capacity(rendered.len() + history.len());
    let mut rest = rendered.into_iter();
    if let Some(first) = rest.next() {
        if first.role == Role::System {
            out.push(first);
            out.extend(history.iter().cloned());
        } else {
            out.extend(history.iter().cloned());
            out.push(first);
        }
    }
    out.extend(rest);
    out
}

/// `ask-plan`: `attachment` is the attached file's name (`None` = "none"); `message` may be `""`.
pub struct AskPlan<'a> {
    pub today: &'a str,
    pub categories: &'a str,
    pub scope: &'a str,
    pub history: &'a str,
    pub cited: &'a str,
    pub attachment: Option<&'a str>,
    pub message: &'a str,
}

pub fn ask_plan(v: &AskPlan<'_>) -> Result<Vec<LlmMessage>> {
    template("ask-plan").render(&[
        ("today", v.today),
        ("categories", v.categories),
        ("scope", v.scope),
        ("history", v.history),
        ("cited", v.cited),
        ("attachment", v.attachment.unwrap_or("none")),
        ("message", v.message.trim()),
    ])
}

pub struct AskAnswer<'a> {
    pub today: &'a str,
    pub scope: &'a str,
    pub out_of_scope_note: &'a str,
    pub refs_note: &'a str,
    pub sources: &'a str,
    pub glossary: &'a str,
    pub question: &'a str,
}

pub fn ask_answer(v: &AskAnswer<'_>, history: &[LlmMessage]) -> Result<Vec<LlmMessage>> {
    let rendered = template("ask-answer").render(&[
        ("today", v.today),
        ("scope", v.scope),
        ("out_of_scope_note", v.out_of_scope_note),
        ("refs_note", v.refs_note),
        ("sources", v.sources),
        ("glossary", v.glossary),
        ("question", v.question.trim()),
    ])?;
    Ok(with_history(rendered, history))
}

pub struct AskAction<'a> {
    pub today: &'a str,
    pub categories: &'a str,
    pub scope: &'a str,
    pub candidates: &'a str,
    /// This turn's sources (`formats::sources_block`): the referenced items whole, then the
    /// search's best pieces (D-83). "" when there are none.
    pub sources: &'a str,
    pub glossary: &'a str,
    pub request: &'a str,
}

pub fn ask_action(v: &AskAction<'_>, history: &[LlmMessage]) -> Result<Vec<LlmMessage>> {
    let rendered = template("ask-action").render(&[
        ("today", v.today),
        ("categories", v.categories),
        ("scope", v.scope),
        ("candidates", v.candidates),
        ("sources", v.sources),
        ("glossary", v.glossary),
        ("request", v.request.trim()),
    ])?;
    Ok(with_history(rendered, history))
}

pub struct AskPage<'a> {
    pub today: &'a str,
    /// The category's name.
    pub category: &'a str,
    pub title: &'a str,
    pub content: &'a str,
    pub sources: &'a str,
    pub glossary: &'a str,
    pub request: &'a str,
}

pub fn ask_page(v: &AskPage<'_>, history: &[LlmMessage]) -> Result<Vec<LlmMessage>> {
    let rendered = template("ask-page").render(&[
        ("today", v.today),
        ("category", v.category),
        ("title", v.title),
        ("content", v.content),
        ("sources", v.sources),
        ("glossary", v.glossary),
        ("request", v.request.trim()),
    ])?;
    Ok(with_history(rendered, history))
}

pub fn edit_page(
    page: &str,
    change: &str,
    sources: &str,
    glossary: &str,
    history: &[LlmMessage],
) -> Result<Vec<LlmMessage>> {
    let rendered = template("edit-page").render(&[
        ("page", page),
        ("change", change.trim()),
        ("sources", sources),
        ("glossary", glossary),
    ])?;
    Ok(with_history(rendered, history))
}

/// `doc-terms`, on the default model, once per attached document (FR-KN-GL-7).
pub fn doc_terms(file_name: &str, source: &str, excerpts: bool, glossary: &str) -> Result<Vec<LlmMessage>> {
    template("doc-terms").render(&[
        ("file_name", file_name),
        ("source", source),
        ("excerpt_note", note(excerpts)),
        ("glossary", glossary),
    ])
}

/// The `search` text of `ask-web` when the message is sent with the provider's web search tool
/// (README, Other conventions; word for word).
pub const WEB_SEARCH_ON: &str = "The user turned web search on for this message, so search the web before you answer, even when you think you already know the answer, and even for a simple question: the user wants the answer checked against current pages, not recalled. Base the answer on the pages the search returned and cite the ones you used. Do not write web addresses from memory: the citations are attached to your answer for you. When the search finds nothing reliable, say so plainly instead of guessing.";

/// The `search` text of `ask-web` when the provider refused the search and the message is answered
/// again without the tool (README, Other conventions; word for word).
pub const WEB_SEARCH_OFF: &str = "Web search is not available for this message, so answer from what you already know. Say plainly in your first sentence that this answer is not from a web search and may be out of date. Do not write web addresses from memory. When the question needs current information you cannot know, say so instead of guessing.";

/// `ask-web`: only today, the mode's fixed search text and what the user typed (FR-LLM-AS-12).
/// `web` is true when the message goes out with the web search tool, false for the re-answer after
/// the provider refused it.
pub fn ask_web(today: &str, message: &str, web: bool) -> Result<Vec<LlmMessage>> {
    let search = if web { WEB_SEARCH_ON } else { WEB_SEARCH_OFF };
    template("ask-web").render(&[("today", today), ("search", search), ("message", message.trim())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_templates_render_the_same_messages_as_lf() {
        let lf = "---\nid: newline-test\nmodel: conversation\noutput: text\nvariables: []\n---\n<<<system>>>\nSystem text.\n<<<user>>>\nUser text.\n";
        let expected = parse("newline-test", lf).unwrap().render(&[]).unwrap();
        let actual = parse("newline-test", &lf.replace('\n', "\r\n")).unwrap().render(&[]).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual[0].text(), "System text.");
        assert_eq!(actual[1].text(), "User text.");
    }


    #[test]
    fn history_goes_between_system_and_user() {
        let history = [
            LlmMessage::new(Role::User, "earlier question"),
            LlmMessage::new(Role::Assistant, "earlier answer"),
        ];
        let m = edit_page("# P\n\nx", "add y", "", "", &history).unwrap();
        let roles: Vec<Role> = m.iter().map(|x| x.role).collect();
        assert_eq!(roles, [Role::System, Role::User, Role::Assistant, Role::User]);
        assert!(m[3].text().contains("add y"));
        let web = ask_web("2026-10-15 (Thursday)", " what's new? ", true).unwrap();
        assert_eq!(web.last().unwrap().text(), "what's new?");
    }

    #[test]
    fn ask_web_renders_the_mode_text() {
        let on = ask_web("2026-10-15 (Thursday)", "what's new?", true).unwrap();
        let off = ask_web("2026-10-15 (Thursday)", "what's new?", false).unwrap();
        for m in [&on, &off] {
            assert_eq!(m.len(), 2);
            assert!(m[0].text().starts_with("You answer the user's message."), "the mocks match on the first words");
            assert!(m[0].text().contains("Today is 2026-10-15 (Thursday)."));
            assert_eq!(m[1].text(), "what's new?");
        }
        assert!(on[0].text().contains(WEB_SEARCH_ON));
        assert!(!on[0].text().contains(WEB_SEARCH_OFF));
        assert!(off[0].text().contains(WEB_SEARCH_OFF));
        assert!(!off[0].text().contains(WEB_SEARCH_ON));
        assert!(off[0].text().contains("not from a web search"));
    }

    #[test]
    fn every_template_parses() {
        for (id, _) in TEMPLATES {
            let t = template(id);
            assert!(!t.messages.is_empty(), "{id}");
            assert_eq!(t.messages.last().unwrap().0, Role::User, "{id}: ends with the user message");
        }
        assert_eq!(template("draft-file").model, ModelRole::Conversation);
        assert_eq!(template("check").output, Output::Json);
        assert!(template("draft-file").stream);
    }

    #[test]
    fn variables_must_match_exactly() {
        let t = template("keep-one-topic");
        assert!(t.render(&[("topic", "a")]).is_err(), "missing ones");
        let all = [("topic", "a"), ("topics", "- a"), ("draft", "# A"), ("glossary", "")];
        assert!(t.render(&all).is_ok());
        let mut extra = all.to_vec();
        extra.push(("nope", "x"));
        assert!(t.render(&extra).is_err(), "an undeclared one");
    }

    #[test]
    fn values_are_not_scanned_again() {
        let m = suggest_merge("# Old {{new_content}}", "# New {{> page-rules}}", "").unwrap();
        let user = m.last().unwrap().text();
        assert!(user.contains("# Old {{new_content}}"));
        assert!(user.contains("# New {{> page-rules}}"));
    }

    #[test]
    fn ocr_appends_the_images_in_order() {
        let m = ocr(&["/t/p1.jpg".into(), "/t/p2.jpg".into()]).unwrap();
        let json = serde_json::to_value(&m).unwrap();
        let blocks = json.as_array().unwrap().last().unwrap()["content"].as_array().unwrap().clone();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1], serde_json::json!({"type": "image", "path": "/t/p1.jpg"}));
        assert_eq!(blocks[2]["path"], "/t/p2.jpg");
    }

    #[test]
    fn revise_appends_draft_turns_and_request() {
        let earlier = [LlmMessage::new(Role::User, "shorter"), LlmMessage::new(Role::Assistant, "# Short")];
        let m = revise("f.pdf", "src", "", false, "", "# Draft", &earlier, "and a title").unwrap();
        let roles: Vec<Role> = m.iter().map(|x| x.role).collect();
        assert_eq!(roles, [Role::System, Role::User, Role::Assistant, Role::User, Role::Assistant, Role::User]);
        assert_eq!(m[2].text(), "# Draft");
        assert_eq!(m.last().unwrap().text(), "and a title");
    }

    #[test]
    fn list_formats_follow_the_readme() {
        let e = PromptEntry {
            term: "MMU".into(),
            other_names: vec!["Mobile Manufacturing Unit".into()],
            description: "Truck that mixes\nbulk explosive.".into(),
            category_name: None,
        };
        let p = PromptEntry {
            term: "auger".into(),
            other_names: vec![],
            description: "The arm.".into(),
            category_name: Some("Site operations".into()),
        };
        assert_eq!(
            glossary_lines(&[e, p]),
            "- MMU (Mobile Manufacturing Unit) [global]: Truck that mixes bulk explosive.\n- auger [category: Site operations]: The arm."
        );
        let g = [
            GroupLine { name: "Safety".into(), description: String::new(), proposed: false },
            GroupLine { name: "Pumps".into(), description: "Pump service.".into(), proposed: true },
        ];
        assert_eq!(groups_lines(&g), "- Safety\n- Pumps: Pump service. (proposed in this run)");
        assert_eq!(parts_block(&["# A".into(), "# B".into()]), "Part 1 of 2:\n<part>\n# A\n</part>\n\nPart 2 of 2:\n<part>\n# B\n</part>");
        assert_eq!(pages_lines(&[("p1".into(), "T".into(), "a\n\nb".into())]), "- [p1] T: a b");
    }

    /// The Rust and Python renderers give the same messages (`testdata/golden.json`, written by
    /// `poc/h-create/golden.py` after a template changes).
    #[test]
    fn golden_matches_the_python_renderer() {
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../prompts/testdata/golden.json")).unwrap();
        for (id, _) in TEMPLATES {
            let t = template(id);
            let values: Vec<(String, String)> =
                t.variables.iter().map(|v| (v.clone(), format!("<{v}: {{{{x}}}} value>"))).collect();
            let refs: Vec<(&str, &str)> = values.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            let rendered = serde_json::to_value(t.render(&refs).unwrap()).unwrap();
            assert_eq!(rendered, golden[*id], "{id} differs from the Python renderer; re-run poc/h-create/golden.py");
        }
    }
}
