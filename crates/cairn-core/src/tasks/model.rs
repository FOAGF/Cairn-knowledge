//! Task types shared by the core, the commands and (through generated bindings) the UI.
//! This is the M2 contract: change it only together with `app/src/api/tasks.ts`.

use serde::{Deserialize, Serialize};
use specta::Type;

/// FR-TASK-ST-1. `done` and `wont_do` are closed (FR-TASK-ST-3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
    WontDo,
}

impl TaskStatus {
    pub fn is_closed(self) -> bool {
        matches!(self, TaskStatus::Done | TaskStatus::WontDo)
    }
}

/// FR-TASK-PR-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    High,
    Medium,
    Low,
}

/// FR-TASK-TY-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    Bug,
    Feature,
    General,
}

/// A task (§4.1). Timestamps are ISO 8601 UTC; `dueDate` is a local `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    /// `<CATEGORY-KEY>-<n>`, never reused.
    pub key: String,
    pub category_id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub task_type: TaskType,
    /// Markdown with mentions.
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    /// Sorted, case-insensitively unique.
    pub labels: Vec<String>,
    pub due_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Set when the task moves to done / wont_do, cleared when reopened (FR-TASK-ST-4).
    pub completed_at: Option<String>,
}

/// Input for `create`. Only `categoryId` and `title` are required; the other fields may be left out.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub category_id: String,
    pub title: String,
    #[serde(rename = "type")]
    #[specta(optional)]
    pub task_type: Option<TaskType>,
    #[specta(optional)]
    pub description: Option<String>,
    #[specta(optional)]
    pub status: Option<TaskStatus>,
    #[specta(optional)]
    pub priority: Option<Priority>,
    #[specta(optional)]
    pub labels: Option<Vec<String>>,
    #[specta(optional)]
    pub due_date: Option<String>,
}

/// Input for `update`: only the given fields change. The category changes through `move_task`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    #[specta(optional)]
    pub title: Option<String>,
    #[serde(rename = "type")]
    #[specta(optional)]
    pub task_type: Option<TaskType>,
    #[specta(optional)]
    pub description: Option<String>,
    #[specta(optional)]
    pub status: Option<TaskStatus>,
    #[specta(optional)]
    pub priority: Option<Priority>,
    #[specta(optional)]
    pub labels: Option<Vec<String>>,
    /// `YYYY-MM-DD`; an empty string clears the due date; `null` leaves it unchanged.
    #[specta(optional)]
    pub due_date: Option<String>,
}

/// One change for many tasks at once (FR-TASK-UI-7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskBulkChange {
    Status {
        status: TaskStatus,
    },
    Priority {
        priority: Priority,
    },
    /// New keys as in `move_task` (FR-TASK-UI-6).
    Category {
        #[serde(rename = "categoryId")]
        category_id: String,
    },
    AddLabel {
        label: String,
    },
    /// Matched ignoring case.
    RemoveLabel {
        label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DueFilter {
    /// Open tasks due before today.
    Overdue,
    /// Due today.
    Today,
    /// Due Monday–Sunday of the current week.
    ThisWeek,
    /// No due date.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskSortField {
    /// Soonest first; tasks without a due date last.
    #[default]
    Due,
    /// High first.
    Priority,
    Created,
    Updated,
    /// Category key, then number.
    Key,
}

/// Filters for the list view (FR-TASK-UI-1). Every field is optional; empty lists mean "any".
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskFilter {
    #[specta(optional)]
    pub category_ids: Option<Vec<String>>,
    /// Explicit statuses. When given, `includeClosed` is ignored.
    #[specta(optional)]
    pub statuses: Option<Vec<TaskStatus>>,
    /// Without explicit statuses, closed tasks are hidden unless this is true (FR-TASK-ST-3).
    #[specta(optional)]
    pub include_closed: Option<bool>,
    #[specta(optional)]
    pub types: Option<Vec<TaskType>>,
    #[specta(optional)]
    pub priorities: Option<Vec<Priority>>,
    /// Tasks with any of these labels.
    #[specta(optional)]
    pub labels: Option<Vec<String>>,
    #[specta(optional)]
    pub due: Option<DueFilter>,
    /// Due on this day (`YYYY-MM-DD`): the Home calendar's selected day (FR-HOME-5).
    #[specta(optional)]
    pub due_on: Option<String>,
    /// Case-insensitive substring of key, title or description (works for Chinese).
    #[specta(optional)]
    pub text: Option<String>,
    #[specta(optional)]
    pub sort: Option<TaskSortField>,
    #[specta(optional)]
    pub descending: Option<bool>,
    #[specta(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TaskLinkType {
    Relates,
    Parent,
}

/// FR-TASK-LN-1..2, -4. For `parent`, `fromTaskId` is the parent and `toTaskId` the sub-task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskLink {
    pub id: String,
    #[serde(rename = "type")]
    pub link_type: TaskLinkType,
    pub from_task_id: String,
    pub to_task_id: String,
    pub created_at: String,
}

/// FR-TASK-CM-1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskComment {
    pub id: String,
    pub task_id: String,
    pub text: String,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Task fields kept in the daily history (FR-TASK-HI-2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub title: String,
    #[serde(rename = "type")]
    pub task_type: TaskType,
    pub description: String,
    pub status: TaskStatus,
    pub priority: Priority,
    pub labels: Vec<String>,
    pub due_date: Option<String>,
    pub category_id: String,
}

/// One version per task per local calendar day (FR-TASK-HI-1, -4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskVersion {
    pub task_id: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub snapshot: TaskSnapshot,
}

/// Completion summary of a done task (FR-TASK-SUM-1..4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CompletionSummary {
    pub working_days: u32,
    pub blocked_days: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    TaskCreated,
    TaskCompleted,
    TaskUpdated,
    /// A comment was added (M3).
    TaskCommented,
    /// From M3.
    PageCreated,
    /// One per page per local day (replaced), from M3.
    PageUpdated,
}

/// Recent activity on Home and category pages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: String,
    pub kind: ActivityKind,
    pub ref_id: String,
    pub at: String,
}

/// Debug builds only: sample data from the UI prototype, inserted with its ids where missing.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskSeed {
    pub tasks: Vec<Task>,
    pub links: Vec<TaskLink>,
    pub comments: Vec<TaskComment>,
    pub history: Vec<TaskVersion>,
    pub activity: Vec<ActivityEvent>,
}
