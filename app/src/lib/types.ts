// Data model for Cairn. Mirrors docs/requirements.md §3.2.
// In the UI prototype these objects live in the in-memory store (src/store);
// later they will come from the Rust core.

export type ID = string
/** Calendar date, `YYYY-MM-DD`. */
export type DateStr = string
/** Timestamp, ISO 8601. */
export type DateTimeStr = string

export type TintColor = 'blue' | 'green' | 'purple' | 'amber' | 'rose' | 'slate'

/** A category's colour (FR-PRJ-6, D-40): a preset `TintColor` name, or any custom colour as `#rrggbb`. */
export type CategoryColor = string

export interface Category {
  id: ID
  name: string
  /** Short prefix for task keys, e.g. `PRX` → `PRX-12`. */
  key: string
  /** Short description; given to the LLM as context when searching and answering. */
  description: string
  color: CategoryColor
  status: 'active' | 'archived'
  /** The built-in "General" category cannot be deleted (DM-4). */
  isGeneral?: boolean
  createdAt: DateTimeStr
}

// ---------------------------------------------------------------- Tasks (§4)

export type TaskStatus = 'todo' | 'in_progress' | 'blocked' | 'done' | 'wont_do'
export type Priority = 'high' | 'medium' | 'low'
/** Jira-like issue type. */
export type TaskType = 'bug' | 'feature' | 'general'

export interface Task {
  id: ID
  /** `<CATEGORY-KEY>-<n>`, never reused. */
  key: string
  categoryId: ID
  title: string
  type: TaskType
  /** Markdown, supports mentions. */
  description: string
  status: TaskStatus
  priority: Priority
  labels: string[]
  createdAt: DateTimeStr
  /** Set when status becomes done / wont_do, cleared on reopen (FR-TASK-ST-4). */
  completedAt?: DateTimeStr
  dueDate?: DateStr
  updatedAt: DateTimeStr
}

export type TaskLinkType = 'relates' | 'parent'

export interface TaskLink {
  id: ID
  type: TaskLinkType
  /** For `parent`: fromTaskId is the parent, toTaskId the sub-task. */
  fromTaskId: ID
  toTaskId: ID
}

export interface TaskComment {
  id: ID
  taskId: ID
  /** Markdown, supports mentions. */
  text: string
  createdAt: DateTimeStr
  updatedAt?: DateTimeStr
}

export type TaskSnapshot = Pick<
  Task,
  'title' | 'type' | 'description' | 'status' | 'priority' | 'labels' | 'dueDate' | 'categoryId'
>

/** At most one version per task per calendar day (FR-TASK-HI-1). */
export interface TaskHistoryVersion {
  taskId: ID
  date: DateStr
  snapshot: TaskSnapshot
}

// ------------------------------------------------------------ Knowledge (§5)

export interface KnowledgeGroup {
  id: ID
  categoryId: ID
  name: string
  /** Short description; given to the LLM as context (search, group suggestions). */
  description?: string
}

/** `folder`: external mode, found in the knowledge folder (D-72). */
export type PageOrigin = 'manual' | 'file' | 'task' | 'folder'

export interface KnowledgePage {
  id: ID
  categoryId: ID
  groupId?: ID
  title: string
  /** Markdown, supports mentions and pasted images. */
  body: string
  tags: string[]
  origin: PageOrigin
  originTaskId?: ID
  /** Optional free text: where the knowledge came from. */
  sourceInfo?: string
  checkStatus: 'checked' | 'not_checked'
  archived?: boolean
  createdAt: DateTimeStr
  updatedAt: DateTimeStr
}

export type PageSnapshot = Pick<KnowledgePage, 'title' | 'body' | 'tags' | 'groupId' | 'sourceInfo'>

export interface PageHistoryVersion {
  pageId: ID
  date: DateStr
  snapshot: PageSnapshot
}

/** Manual link between a task and a knowledge page (DM-5). */
export interface TaskPageLink {
  id: ID
  taskId: ID
  pageId: ID
}

// ------------------------------------------------------------ Assistant (§6)

/** A level from the model's effort settings (FR-LLM-EF-4), e.g. `low` or `high`. */
export type Effort = string

// ---------------------------------------------------------------- Activity

export interface ActivityEvent {
  id: ID
  kind: 'page_created' | 'page_updated' | 'task_created' | 'task_completed' | 'task_updated' | 'task_commented'
  refId: ID
  at: DateTimeStr
}
