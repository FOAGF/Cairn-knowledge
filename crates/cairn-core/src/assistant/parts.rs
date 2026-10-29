//! The parts an assistant message carries besides its text (M6, D-44), stored in
//! `messages.payload` with the message (`conversations`). The UI renders them: the answer's
//! sources and labels (UI-AS-2), a find list (FR-LLM-AS-2), a proposal card (FR-LLM-AS-3).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::conversations::TermSuggestion;
use crate::tasks::{Priority, TaskStatus, TaskType};

/// What an answer rests on; the UI shows it as a label (FR-LLM-AS-1, -9, -12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AnswerBasis {
    /// It cites at least one source.
    #[default]
    Notes,
    /// The notes were searched and nothing was cited: *Not from your notes*.
    NotInNotes,
    /// A general question (the first step's `general`): *General knowledge*.
    General,
    /// A 🌐 message that searched the web: *From the web*.
    Web,
    /// A 🌐 message with no search made: *No web search was made*; links in the text are not
    /// sources (FR-LLM-AS-12).
    NoWebSearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Page,
    Task,
    /// A part of the file attached to the question (FR-LLM-AS-11).
    File,
    /// A web page the provider's search returned (FR-LLM-AS-12).
    Web,
}

/// One numbered source of an answer, or one item of *N more searched*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub kind: SourceKind,
    /// The page or task; `None` for a file part or a web page.
    pub id: Option<String>,
    /// Page title, task title, file name or web page title.
    pub title: String,
    /// Task key.
    pub key: Option<String>,
    pub category_id: Option<String>,
    /// Which piece: "Pre-start › Hoses", "Description", "Comment 2026-09-10", "Part 2 of 5";
    /// `None` for a web page.
    pub part: Option<String>,
    /// The start of the piece's own text (never its glossary lines, D-44), or the web citation's
    /// quoted text.
    pub snippet: String,
    /// A web page's address.
    pub url: Option<String>,
}

/// The answer to a question (FR-LLM-AS-1, -9, -11, -12).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnswerPart {
    pub basis: AnswerBasis,
    /// The sources sent with the question, in order: `[n]` in the text is `sources[n - 1]`. For a
    /// web answer, the pages the provider cited.
    pub sources: Vec<SourceRef>,
    /// The source numbers the text cites, in order of first use (after `markers::read`). A web
    /// answer cites all its sources.
    pub cited: Vec<u32>,
    /// Items the search found that were not sent (*N more searched*), best first.
    pub more: Vec<SourceRef>,
    /// FR-LLM-AS-4: categories the message named that are outside the scope; the UI offers
    /// *Search all categories*.
    pub outside_scope: Vec<String>,
    /// FR-KN-GL-7: glossary suggestions from the attached document, after `checks::filter_terms`.
    pub terms: Vec<TermSuggestion>,
    /// Every term ever shown for this document, so it isn't shown again.
    pub shown_terms: Vec<String>,
    /// The category the terms are suggested for (*Add* pre-fills it; *Ignore* is per category).
    pub terms_category_id: Option<String>,
    /// FR-LLM-AS-12: the warning when this message removed the model's *Web search* tick.
    pub web_unticked: Option<String>,
}

/// One row of a find list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FoundItem {
    /// `page` or `task`.
    pub kind: SourceKind,
    pub id: String,
    pub title: String,
    pub key: Option<String>,
    pub category_id: String,
}

/// A find request's answer (FR-LLM-AS-2): the model's intro line and every match, as Rust found
/// them. The UI shows the live status and priority of each task.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FindPart {
    pub intro: String,
    pub items: Vec<FoundItem>,
    pub outside_scope: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// Task ↔ task.
    Relates,
    /// The task becomes a sub-task of the target task.
    Parent,
    /// Task ↔ page.
    Page,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkTo {
    pub kind: LinkKind,
    /// The task or page id.
    pub target_id: String,
    /// Its key (task) or title (page), for the card.
    pub target_name: String,
}

/// One change the assistant proposes (FR-LLM-AS-3). There is no delete: nothing is ever deleted
/// through the chat. A new page is not a change here: it is the message's draft card (M5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "op",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ProposedChange {
    CreateTask {
        category_id: String,
        title: String,
        task_type: TaskType,
        priority: Priority,
        due: Option<String>,
        labels: Vec<String>,
        description: String,
        links: Vec<LinkTo>,
    },
    /// Only the `Some` fields change; `links` are added.
    UpdateTask {
        task_id: String,
        key: String,
        title: String,
        status: Option<TaskStatus>,
        priority: Option<Priority>,
        task_type: Option<TaskType>,
        due: Option<String>,
        /// The due date is removed (`due` is then `None`).
        clear_due: bool,
        /// Replaces the whole list.
        labels: Option<Vec<String>>,
        description: Option<String>,
        links: Vec<LinkTo>,
    },
    Comment {
        task_id: String,
        key: String,
        text: String,
    },
    /// The page with the change made (`edit-page`), shown as a diff; saved through the page
    /// history (FR-KN-ED-5). Fails on Confirm when the page changed since (`base_updated_at`).
    UpdatePage {
        page_id: String,
        title: String,
        change: String,
        old_body: String,
        new_body: String,
        base_updated_at: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RowState {
    #[default]
    Pending,
    Done,
    Rejected,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProposalRow {
    pub change: ProposedChange,
    /// Starts ticked; *Confirm* applies the ticked rows.
    pub ticked: bool,
    pub state: RowState,
    /// After *Confirm*: "Created OPS-48", "Marked OPS-41 as done"; or why it failed.
    pub result: Option<String>,
    /// The created or changed task or page, for the link.
    pub result_id: Option<String>,
}

/// A create or change request's card (FR-LLM-AS-3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    /// The model's one sentence shown with the card: what it proposes, the question it asks when
    /// the target is unclear, or why there is nothing to propose.
    pub reply: String,
    pub rows: Vec<ProposalRow>,
    /// FR-KN-GL-7: glossary suggestions for the created tasks, after the filter.
    pub terms: Vec<TermSuggestion>,
    pub shown_terms: Vec<String>,
    pub terms_category_id: Option<String>,
}
