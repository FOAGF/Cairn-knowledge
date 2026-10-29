import { plural } from '../util'

// The Categories screen and a category's own page (FR-PRJ-*). A category's name is data and is shown
// as stored, the built-in "General" too.
export const categories = {
  list: {
    description: 'Everything in Cairn belongs to a category. The built-in General category holds the rest.',
    showArchived: 'Show archived',
    newCategory: 'New category',
    loadFailed: (message: string) => `Could not load categories: ${message}`,
  },
  dialog: {
    editTitle: 'Edit category',
    newTitle: 'New category',
    description: 'Tasks and knowledge pages always belong to a category.',
    taskKey: 'Task key',
    keyTaken: 'This key is already used.',
    keyNumbering: (key: string) => `Tasks will be numbered ${key}-1, ${key}-2, …`,
    /** Follows `keyTaken` or `keyNumbering` while editing. */
    keyFixed: ' The key cannot be changed.',
    descriptionPlaceholder: 'What is this category about? e.g. “Planning and notes for the website redesign.”',
    descriptionHint: 'A short description helps the assistant understand what the category is about when it searches and answers.',
    colour: 'Colour',
    create: 'Create category',
    updated: 'Category updated',
    created: (name: string) => `Category "${name}" created`,
  },
  card: {
    builtIn: 'Built-in category, cannot be deleted',
    /** Under the number of tasks that are not closed. */
    open: 'open',
    pages: 'pages',
    blocked: 'blocked',
    progress: (done: number, total: number, groups: number) => `${done} of ${total} tasks done · ${plural(groups, '# group', '# groups')}`,
  },
  page: {
    editCategory: 'Edit category',
    keyFixed: (key: string) => `The task key ${key} cannot be changed.`,
    descriptionHint: 'Helps the assistant understand what this category is about.',
    descriptionPlaceholder: 'What is this category about?',
    noDescription: 'No description yet.',
    /** Next to the description's Edit link. */
    descriptionNote: 'Helps the assistant understand what this category is about',
    archived: 'Archived',
    notFound: 'Category not found',
    loadFailed: 'Could not load the category',
    notFoundHint: 'It may have been removed, or the link is wrong.',
    backToCategories: 'Back to Categories',
    askPlaceholder: (name: string) => `Ask about ${name}… search its knowledge or tasks.`,
    ask: 'Ask',
  },
  activity: {
    title: 'Recent activity',
    description: 'Latest changes in this category',
    empty: 'Nothing has happened here yet.',
    /** By activity kind (and the newest change of a task or page without one). */
    page_created: 'Added page',
    page_updated: 'Updated page',
    task_created: 'Added task',
    task_completed: 'Completed task',
    task_updated: 'Updated task',
    task_commented: 'Commented on',
  },
  tasks: {
    description: (open: number) => `${open} open · sorted by due date`,
    openBoard: 'Open board',
    statusCount: (status: string, n: number) => `${status}: ${n}`,
    noOpen: 'No open tasks. Nice and quiet.',
    /** The noun of "Show 4 more tasks" (FoldButton). */
    foldNoun: (n: number): string => (n === 1 ? 'task' : 'tasks'),
    recentlyCompleted: 'Recently completed',
    noneCompleted: 'Nothing completed yet.',
  },
  knowledge: {
    description: (pages: number, groups: number) => `${pages} pages in ${plural(groups, '# group', '# groups')}`,
    newGroup: 'New group',
    fromFile: 'From a file',
    empty: 'No knowledge yet. Add a page or create a group.',
    /** The noun of "Show 2 more groups" (FoldButton). */
    groupFoldNoun: (n: number): string => (n === 1 ? 'group' : 'groups'),
  },
  group: {
    notCheckedYet: 'Not checked yet',
    pageCount: (n: number) => plural(n, '# page', '# pages'),
    descriptionPlaceholder: 'What is this group about?',
    addDescription: 'Add a description',
    ungroupedNote: 'Pages that are not in any group.',
    /** The noun of "Show 2 more pages" (FoldButton). */
    pageFoldNoun: (n: number): string => (n === 1 ? 'page' : 'pages'),
    noPages: 'No pages yet.',
  },
  newGroupDialog: {
    descriptionHint: 'Helps the assistant file and find pages in this group.',
    create: 'Create group',
  },
}
