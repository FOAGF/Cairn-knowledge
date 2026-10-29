import type { ReactNode } from 'react'
import { plural } from '../util'

const tasksCount = (n: number) => plural(n, '# task', '# tasks')

// The Tasks screens: the list and board, a task's page, and the new-task dialog (features/tasks).
export const tasks = {
  /** The description box of a task, in the dialog and on its page. */
  markdownHint: 'Markdown supported. Type # to mention a task, [[ to link a page.',
  page: {
    count: (n: number, filtered: boolean) => `${tasksCount(n)}${filtered ? ' match your filters' : ''}`,
    view: 'View',
    listView: 'List view',
    boardView: 'Board view',
    list: 'List',
    board: 'Board',
    search: 'Search tasks',
    /** A filter's first choice, e.g. "Status: all". */
    all: (filter: string) => `${filter}: all`,
    active: 'Active (in progress + blocked)',
    label: 'Label',
    due: 'Due',
    dueOptions: { overdue: 'Overdue', today: 'Due today', week: 'Due this week', none: 'No due date' },
    clear: 'Clear',
    sortBy: 'Sort by',
    sort: 'Sort:',
    sortOptions: { due: 'Due date', priority: 'Priority', created: 'Created', updated: 'Updated', key: 'Key' },
    showClosed: 'Show closed',
    noMatch: 'No tasks match',
    emptyHint: (filtered: boolean, showClosed: boolean) =>
      `${filtered ? 'Try removing a filter, or ' : 'Nothing here yet. '}${showClosed ? '' : 'turn on "Show closed" to see finished tasks.'}`,
  },
  list: {
    selectAll: 'Select all shown tasks',
    selectedOf: (ticked: number, shown: number) => `${ticked} of ${shown} selected`,
    selectAllShown: (shown: number) => `Select all shown (${shown})`,
    select: (key: string) => `Select ${key}`,
  },
  board: {
    expand: (status: string) => `Expand ${status}`,
    collapse: (status: string) => `Collapse ${status}`,
    dropHere: 'Drop a task here',
    /** A collapsed column, written top to bottom. */
    dropTo: (status: string) => `Drop to ${status}`,
  },
  bulk: {
    addLabel: 'Add label',
    removeLabel: 'Remove label',
    same: 'The history and the activity of each task record the change, as for one task.',
    statusTitle: (n: number, status: string) => `Set ${tasksCount(n)} to ${status}?`,
    statusDescription: (pill: ReactNode, same: string) => (
      <>
        Each task becomes {pill}. {same} Their status now:
      </>
    ),
    setStatus: (status: string) => `Set to ${status}`,
    statusDone: (n: number, status: string) => `${tasksCount(n)} set to ${status}`,
    priorityTitle: (n: number, priority: string) => `Set ${tasksCount(n)} to ${priority} priority?`,
    priorityDescription: (icon: ReactNode, same: string) => (
      <>
        Each task becomes {icon}. {same} Their priority now:
      </>
    ),
    setPriority: (priority: string) => `Set to ${priority}`,
    priorityDone: (n: number, priority: string) => `${tasksCount(n)} set to ${priority} priority`,
    moveTitle: (n: number, category: string) => `Move ${tasksCount(n)} to ${category}?`,
    moveDescription: (category: string) => `Each task gets a new key in ${category}; its old key keeps working in mentions and links.`,
    moveDone: (n: number, category: string) => `${tasksCount(n)} moved to ${category}`,
    addLabelTitle: (n: number) => `Add a label to ${tasksCount(n)}`,
    addLabelDescription: 'Type a label, or pick one of the existing labels. Tasks that already have it keep it once.',
    addLabelDone: (label: string, n: number) => `Label "${label}" added to ${tasksCount(n)}`,
    removeLabelTitle: (n: number) => `Remove a label from ${tasksCount(n)}`,
    removeLabelDescription: 'Tasks without the label stay as they are.',
    removeLabelDone: (label: string, n: number) => `Label "${label}" removed from ${tasksCount(n)}`,
    deleteTitle: (n: number) => `Delete ${tasksCount(n)}?`,
    deleteDescription:
      'They are deleted for good, with their comments, history and links. This cannot be undone; a backup can bring them back. Their keys are never reused.',
    deleteConfirm: (n: number) => `Delete ${tasksCount(n)}`,
    deleteDone: (n: number) => `${tasksCount(n)} deleted`,
  },
  detail: {
    titleLabel: 'Task title',
    noDescription: 'No description yet.',
    details: 'Details',
    created: (date: string) => `Created ${date}`,
    completed: (date: string) => `Completed ${date}`,
    notFound: (key: string) => `Task ${key} not found`,
    loadFailed: 'Could not load the task',
    maybeDeleted: 'It may have been deleted. ',
    backToAll: 'Back to all tasks',
    createKnowledge: 'Create knowledge from task',
    deleteTask: 'Delete task',
    updates: (n: number) => `Updates (${n})`,
    history: (n: number) => `History (${n})`,
    deleteTitle: (key: string) => `Delete ${key}?`,
    deleteDescription: (title: string) => `“${title}” and its comments, links and history will be removed. This cannot be undone.`,
    deleted: (key: string) => `Deleted ${key}`,
  },
  links: {
    remove: (key: string) => `Remove link to ${key}`,
    add: 'Add link',
    addDescription: (key: string) => `Link ${key} to any task, including tasks in other categories.`,
    relatesTo: 'Relates to',
    parentOf: 'Is parent of (the picked task becomes a sub-task)',
    subtaskOf: 'Is sub-task of (the picked task becomes the parent)',
    search: 'Search by key or title',
    noTasks: 'No matching tasks.',
    pageTitle: 'Link a knowledge page',
    pageDescription: (key: string) => `Pick a page related to ${key}.`,
    searchPages: 'Search pages',
    noPages: 'No matching pages.',
    taskLinks: 'Task links',
    noLinkedTasks: 'No linked tasks.',
    parent: 'Parent',
    subtasks: 'Sub-tasks',
    knowledgePages: 'Knowledge pages',
    linkPage: 'Link page',
    noLinkedPages: 'No linked pages.',
    unlink: (title: string) => `Unlink ${title}`,
    mentionedIn: 'Mentioned in',
    /** Marks a mention that is in a comment. */
    comment: 'comment',
  },
  newTask: {
    description: 'The key is assigned automatically from the category.',
    titlePlaceholder: 'What needs to be done?',
    create: 'Create task',
    created: (key: string) => `Created ${key}`,
  },
  history: {
    /** An empty value before or after a change. */
    none: 'none',
    empty: 'No history yet.',
    created: (title: string) => `Task created as “${title}”.`,
    noChanges: 'No field changes.',
    edited: 'edited',
  },
  comments: {
    edited: '· edited',
    edit: 'Edit comment',
    delete: 'Delete comment',
    created: 'Created',
    completed: 'Completed',
    statusTo: (status: ReactNode) => (
      <>
        Status →{' '}
        {status}
      </>
    ),
    showStatus: 'Show status changes',
    addTitle: 'Add a progress update',
    placeholder: 'What progress was made? Markdown and #KEY mentions work here.',
    add: 'Add update',
  },
  completion: {
    title: 'Completion summary',
    spent: (days: number) => plural(days, 'working day spent', 'working days spent'),
    blocked: (days: number) => plural(days, 'working day blocked', 'working days blocked'),
    note: 'Counted by date, inclusive of the first and last day, Monday to Friday. Public holidays are not considered.',
  },
  labels: {
    clearDate: 'Clear date',
    remove: (label: string) => `Remove label ${label}`,
    placeholder: 'Add label and press Enter',
    suggestions: 'Suggestions:',
  },
}
