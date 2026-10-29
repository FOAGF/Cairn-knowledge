//! Knowledge types shared by the core, the commands and (through generated bindings) the UI.
//! This is the M3 contract: change it only together with `app/src/api/knowledge.ts`.

use serde::{Deserialize, Serialize};
use specta::Type;

/// §5.1 / §5.4: pages start "not checked" until the checks ran (M5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Checked,
    NotChecked,
}

/// How the page was created (§5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PageOrigin {
    Manual,
    File,
    Task,
    /// External mode: found in the folder ("Found in the folder"), not made in Cairn (FR-KN-EXT-6).
    Folder,
}

/// A group of pages inside a category (§5.7). One level in internal mode; in external mode a
/// group is a folder and can hold groups (FR-KN-EXT-2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub category_id: String,
    /// External mode: the group this one is inside; `None` directly in the category.
    pub parent_id: Option<String>,
    pub name: String,
    /// Short description, given to the LLM as context (DM-7). May be empty. In external mode it
    /// is kept in Cairn's index (FR-KN-EXT-13).
    pub description: String,
    /// External mode: the group's folder relative to the root, `/`-separated; `None` in internal mode.
    pub path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewGroup {
    pub category_id: String,
    pub name: String,
    #[specta(optional)]
    pub description: Option<String>,
    /// External mode only: a group of the same category to make this one in. Internal mode
    /// refuses it (groups there are one level).
    #[specta(optional)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupPatch {
    #[specta(optional)]
    pub name: Option<String>,
    #[specta(optional)]
    pub description: Option<String>,
    /// External mode only: moves the group (its folder) into another group of the same category;
    /// an empty string moves it directly into the category.
    #[specta(optional)]
    pub parent_id: Option<String>,
}

/// A knowledge page (§5.1). `body` is Markdown; page mentions are `[Title](page:<id>)`,
/// task mentions `#KEY`, images `![alt](attachment:<name>)`. In external mode (D-72) other files
/// of the folder are `folder:<path from the root>` (each path part percent-encoded where a
/// Markdown link needs it, e.g. `folder:Ops/attachments/a%20b.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub category_id: String,
    pub group_id: Option<String>,
    pub title: String,
    pub body: String,
    /// Sorted, case-insensitively unique.
    pub tags: Vec<String>,
    pub source_info: Option<String>,
    pub origin: PageOrigin,
    pub origin_task_id: Option<String>,
    pub check_status: CheckStatus,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    /// External mode: the page's file relative to the root, `/`-separated; `None` in internal mode.
    pub path: Option<String>,
    /// External mode: the file's hash when the index last read or wrote it. The editor keeps the
    /// value from when editing started and sends it as `PagePatch::base_hash` (FR-KN-EXT-11).
    pub file_hash: Option<String>,
}

/// A page without its body, for trees, lists, cards and mention chips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageSummary {
    pub id: String,
    pub category_id: String,
    pub group_id: Option<String>,
    pub title: String,
    pub tags: Vec<String>,
    pub source_info: Option<String>,
    pub origin: PageOrigin,
    pub origin_task_id: Option<String>,
    pub check_status: CheckStatus,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Words in the body; each Chinese character counts as one (FR-KN-ED-4).
    pub word_count: u32,
    /// As [`Page::path`].
    pub path: Option<String>,
    /// As [`Page::file_hash`].
    pub file_hash: Option<String>,
}

/// Input for `create_page`. Only `categoryId`, `title` and `body` are required.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewPage {
    pub category_id: String,
    pub title: String,
    pub body: String,
    #[specta(optional)]
    pub group_id: Option<String>,
    #[specta(optional)]
    pub tags: Option<Vec<String>>,
    #[specta(optional)]
    pub source_info: Option<String>,
    #[specta(optional)]
    pub origin: Option<PageOrigin>,
    #[specta(optional)]
    pub origin_task_id: Option<String>,
    /// Default `not_checked`; the (simulated until M5) save pipeline may pass `checked`.
    #[specta(optional)]
    pub check_status: Option<CheckStatus>,
}

/// Input for `update_page`: only the given fields change. The category changes through `move_page`.
/// In external mode a new title is cleaned for a file name (FR-KN-EXT-15) and the page's file is
/// written (renamed or moved when needed).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PagePatch {
    #[specta(optional)]
    pub title: Option<String>,
    #[specta(optional)]
    pub body: Option<String>,
    #[specta(optional)]
    pub tags: Option<Vec<String>>,
    /// An empty string clears it.
    #[specta(optional)]
    pub source_info: Option<String>,
    /// A group of the page's category; an empty string makes the page ungrouped.
    #[specta(optional)]
    pub group_id: Option<String>,
    /// External mode (FR-KN-EXT-11): the `fileHash` of the version the text was made from. With a
    /// new `body`, the save is refused with the error kind `changed_on_disk` when the file now
    /// differs from it; without it, when the file differs from the version the index has (so text
    /// from an older version never overwrites an edit made outside). *Save* sends the page's
    /// `fileHash` from when editing started; *Keep mine* sends the disk version's `fileHash` (from
    /// `get_page_on_disk`). Changes without a new body read an outside edit in first and keep it.
    /// Ignored in internal mode.
    #[specta(optional)]
    pub base_hash: Option<String>,
}

/// One change for many pages at once (FR-KN-UI-3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageBulkChange {
    Archive,
    Restore,
    /// Into `category_id` (pages from other categories move as in `move_page`) and `group_id`, a
    /// group of that category, or no group when `None`.
    Move {
        #[serde(rename = "categoryId")]
        category_id: String,
        #[serde(rename = "groupId")]
        group_id: Option<String>,
    },
    AddTag {
        tag: String,
    },
    /// Matched ignoring case.
    RemoveTag {
        tag: String,
    },
    MarkChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PageSortField {
    /// Newest first by default.
    #[default]
    Updated,
    Created,
    Title,
}

/// Filters for page lists (FR-KN-UI-1/2). Text search goes through `search` instead.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageFilter {
    #[specta(optional)]
    pub category_ids: Option<Vec<String>>,
    #[specta(optional)]
    pub group_id: Option<String>,
    /// Only pages without a group.
    #[specta(optional)]
    pub ungrouped: Option<bool>,
    /// Pages with any of these tags.
    #[specta(optional)]
    pub tags: Option<Vec<String>>,
    #[specta(optional)]
    pub check_status: Option<CheckStatus>,
    /// Archived pages are hidden unless true.
    #[specta(optional)]
    pub include_archived: Option<bool>,
    #[specta(optional)]
    pub sort: Option<PageSortField>,
    /// Default: true for updated/created, false for title.
    #[specta(optional)]
    pub descending: Option<bool>,
    #[specta(optional)]
    pub limit: Option<u32>,
}

/// External mode: two pages a bulk change would give the same file name in one folder
/// (FR-KN-EXT-10). A change with any of these is refused as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NameClash {
    /// The folder, relative to the root (`""` is the root).
    pub folder: String,
    /// The file name that would be used twice, e.g. `Pump check.md`.
    pub name: String,
    /// The pages of the change that would get it (one when the name is taken by a file already there).
    pub page_ids: Vec<String>,
    /// The titles of those pages, in the same order.
    pub titles: Vec<String>,
}

/// A pasted image stored for a page (FR-KN-ED-3, FR-KN-EXT-3): `target` goes into the Markdown as
/// `![alt](<target>)`. Internal mode: `attachment:<name>` in Cairn's data folder; external mode:
/// `folder:<category folder>/attachments/<name>` in the knowledge folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedImage {
    pub target: String,
    pub name: String,
    pub mime: String,
    pub size: u32,
    /// The image file's absolute path, for showing it (Tauri asset protocol).
    pub file: String,
}

/// Page fields kept in the daily history (FR-KN-ED-5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageSnapshot {
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub group_id: Option<String>,
    pub source_info: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageVersion {
    pub page_id: String,
    /// `YYYY-MM-DD`, local.
    pub date: String,
    pub snapshot: PageSnapshot,
}

/// DM-5: a manual link between a task and a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskPageLink {
    pub id: String,
    pub task_id: String,
    pub page_id: String,
    pub created_at: String,
}

/// What mentions something (DM-MN-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MentionSource {
    Page,
    /// A task description.
    Task,
    Comment,
}

/// What can be mentioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MentionTarget {
    Page,
    Task,
}

/// One "Mentioned in" entry. For a comment, `taskId` is the task it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub source: MentionSource,
    pub source_id: String,
    /// Page id when `source` is page.
    pub page_id: Option<String>,
    /// Task id when `source` is task or comment.
    pub task_id: Option<String>,
    /// Page title, or task key + title, for display.
    pub label: String,
    /// When the source was last changed.
    pub at: String,
}

/// A stored image (FR-KN-ED-3). Use it in Markdown as `![alt](attachment:<name>)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub size: u32,
}

/// FR-KN-ED-10: unsaved editor content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// page | new_page | task_description | comment | new_comment
    pub kind: String,
    pub ref_id: String,
    /// JSON chosen by the UI.
    pub content: String,
    pub updated_at: String,
}

/// TC-8a: how a page is exported as Markdown. The UI asks each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MarkdownExportMode {
    /// `<title>.md` plus `<title>_files/` next to it.
    Folder,
    /// One `.zip` with both.
    Zip,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PageExport {
    /// The `.md` or `.zip` file written.
    pub path: String,
    pub images: u32,
}

/// Debug builds only: the prototype's sample knowledge, inserted where ids are missing.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSeed {
    pub groups: Vec<Group>,
    pub pages: Vec<Page>,
    pub versions: Vec<PageVersion>,
    pub task_page_links: Vec<TaskPageLink>,
}
