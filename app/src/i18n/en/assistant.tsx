import type { ReactNode } from 'react'
import { plural } from '../util'

/** Draws a link around its text; a message decides where the link goes in its sentence. */
type LinkPart = (text: string) => ReactNode

// The Assistant (features/assistant): the conversations, the message box, the answers and the cards.
export const assistant = {
  changeCategory: 'Change category',
  /**
   * The starting templates (D-64, UI-AS-4): the user fills in each [blank] and sends the text as their
   * own message. The blanks are found by their half-width square brackets.
   */
  templates: {
    ask: { label: 'Ask your notes', text: 'What do my notes say about [topic]?' },
    create: { label: 'Create a task', text: 'Create a task to [what needs doing] by [due date]' },
    update: { label: 'Update a task', text: 'Mark [task key, e.g. GEN-12] as [done / in progress / blocked]' },
    find: { label: 'Find tasks', text: 'Which of my tasks are [overdue / blocked / due this week]?' },
  },
  page: {
    dropUnreadable: 'Cairn couldn’t read the dropped item. Use the attach button instead.',
    dropToAttach: 'Drop a file to attach it',
    dropOrBrowse: 'Drop a file here or browse',
    noSizeLimit: '.md  .txt  .pdf  .docx  .png  .jpg  .jpeg. There is no size limit.',
    fileHint:
      'Add an instruction below if you only need part of it, for example “only the summary and the main steps”. I draft one page and you review it before anything is saved.',
    emptyTitle: 'What would you like to know?',
    emptyText: 'Ask about your tasks and knowledge, or have me create something for you.',
    templatesHint: 'Pick a template, fill in the highlighted blanks (Tab goes to the next one), then send.',
    /** `when`: "in 3 days"; null when the time is not known. */
    archived: (when: string | null) => `Archived: read-only, and deleted ${when ?? 'soon'}.`,
  },
  conversations: {
    new: 'New conversation',
    fromFile: 'Create knowledge from a file',
    search: 'Search conversations',
    noneFound: 'No conversations found',
    noneActive: 'No active conversations',
    archivedSection: (n: number) => `Archived (${n})`,
    /** `when`: "in 3 days". */
    deletedIn: (when: string) => `Deleted ${when}`,
    menu: 'Conversation menu',
    deleteNow: 'Delete now…',
    archived: 'Conversation archived',
    archivedDescription: 'It is deleted for good in 3 days. Restore it before then to keep it.',
    undo: 'Undo',
    deleteTitle: 'Delete this conversation now?',
    deleteDescription: (title: string) => `"${title}" and its messages are deleted for good. This cannot be undone.`,
    deleteConfirm: 'Delete conversation',
    renameTitle: 'Rename conversation',
  },
  composer: {
    fillIn: (blank: string) => `Fill in ${blank} first`,
    fillInHint: 'Type over the highlighted blank; Tab goes to the next one.',
    placeholderWithFile: 'Optional: tell me which part to use…  (Enter to send)',
    placeholder: 'Ask about your tasks and knowledge…  (Enter to send, Shift+Enter for a new line, # for a task, [[ for a page)',
    attach: 'Attach a file',
    attachHint: 'Attach a file to ask about it or create a page from it (.md .txt .pdf .docx .png .jpg)',
    model: 'Model',
    /** `level`: the provider's own name for it, e.g. "high". */
    effortDefault: (level: string) => `Effort: default (${level})`,
    effortNotSet: 'Effort: not set',
    effort: (level: string) => `Effort: ${level}`,
    effortHint: 'Reasoning effort is mapped to each provider\'s own parameter. "Not set" sends nothing and uses the model\'s default.',
    allCategories: 'All categories',
    categories: (n: number) => plural(n, '# category', '# categories'),
    scopeHint: 'Limit the search to these categories. None selected means everything.',
    remove: (name: string) => `Remove ${name}`,
    web: 'Web search for the next message',
    webUnsupported:
      'Web search is not supported by the chosen model. To turn it on for a model, tick Web search in Settings › AI providers.',
    webHint: 'Search the web for the next message only. Your notes are not included.',
    webUnavailable: 'Not available with a file, or while a draft is being refined.',
    send: 'Send',
    webOn: 'Web search is on for this message. Only what you type is sent: your notes, glossary and earlier messages are not included.',
    historyLeft: (percent: number) => `History: ${percent}% left`,
    widen: 'Widen for this conversation',
    oldestDropped: 'The oldest messages are no longer sent to this model.',
    noProvider: (link: LinkPart) => (
      <>
        All AI providers are disabled —{' '}
        {link('enable one in Settings')}
      </>
    ),
    fallback: (provider: string, model: string) => `${provider} is disabled, so this conversation uses ${model} instead.`,
    anotherModel: 'another model',
  },
  message: {
    setUpModel: (link: LinkPart) => <>{link('Open Settings')} to set a model up.</>,
    retry: 'Retry',
    useAsIs: 'Use the extracted text as is',
    /** After the model's name: the effort level the reply sent. */
    effortNote: (level: string, isDefault: boolean) => ` · ${level} effort${isDefault ? ' (default)' : ''}`,
    thoughtFor: (tokens: string) => ` · thought for ${tokens} tokens`,
    webOnly: 'Web search, only this message was sent',
    sending: (percent: number) => `Sending the file… ${percent}%`,
  },
  answer: {
    /** The label over an answer, by what it rests on; the other answers have none. */
    basis: {
      not_in_notes: 'Not from your notes',
      general: 'General knowledge',
      web: 'From the web',
      no_web_search: 'No web search was made — from the model’s own knowledge',
    },
    sources: 'Sources',
    moreSearched: (n: number) => `${n} more searched`,
    outsideScope: (categories: string[]) =>
      `Your question names ${categories.join(', ')}, which is outside the categories this conversation searches.`,
    searchAll: 'Search all categories',
  },
  find: {
    showMore: (left: number) => `Show more (${left} left)`,
  },
  /** The `#` and `[[` suggestions in the composer (FR-LLM-AS-14). */
  references: {
    tasks: 'Tasks',
    pages: 'Pages',
    noTasks: 'No matching tasks',
    noPages: 'No matching pages',
  },
  proposal: {
    /** Before a linked task's name. */
    links: { relates: 'Related to', parent: 'Sub-task of', page: 'Linked page' },
    createTask: 'Create task',
    updateTask: (key: string) => `Update ${key}`,
    comment: (key: string) => `Comment on ${key}`,
    changePage: (title: string) => `Change page “${title}”`,
    due: 'Due',
    linksField: 'Links',
    addLinks: 'Add links',
    cleared: 'Cleared',
    hideChanges: 'Hide the changes',
    showChanges: 'Show the changes',
    failed: 'This change could not be made.',
    rejectedRow: 'Rejected, nothing was changed.',
    done: 'Done',
    apply: (change: string) => `Apply: ${change}`,
    edit: 'Edit…',
    /** Ends editing a change in the card; *Confirm* then saves it. */
    editDone: 'Done',
    pageChanged: 'The page changed since this was proposed. Ask again.',
    error: 'Something went wrong.',
    title: (n: number) => plural(n, 'Proposed change', '# proposed changes'),
    notSaved: 'Not saved yet. Untick what you don’t want.',
    rejected: 'Rejected',
    confirmed: 'Confirmed',
    /** `ticked`: shown when more than one change could be ticked. */
    confirm: (ticked: number | null) => (ticked === null ? 'Confirm' : `Confirm (${ticked})`),
    reject: 'Reject',
  },
  draft: {
    title: (asIs: boolean) => `Draft page${asIs ? ' · as it is, without the AI' : ''}`,
    untitled: 'Untitled',
    earlier: 'Earlier version',
    discarded: 'Discarded',
    incomplete: 'The model stopped before the end, so this draft may be cut off. Check the end, or ask me to finish it.',
    topics: (topics: string[]) => `This draft covers several topics: ${topics.join(', ')}. A page works best with one topic.`,
    /** A button, and the message it sends to the model when pressed. */
    keepOnly: (topic: string) => `Keep only: ${topic}`,
    newGroup: '(new)',
    noSuggestion: 'No suggestion',
    suggestedTags: 'Suggested tags',
    sourceInfo: 'Source info',
    showFull: 'Show full draft',
    words: (n: number) => `${n} words`,
    notSaved: 'Not saved yet. Tell me what to change.',
    inReview: 'Sent to the review screen, not saved yet.',
    reviewAgain: 'Review again',
    savedAs: (title: ReactNode) => (
      <>
        Saved as{' '}
        {title}
      </>
    ),
    replaced: 'Replaced by the revised draft below.',
    discardedNote: 'Discarded, nothing was saved.',
  },
  file: {
    remove: 'Remove file',
    large: 'Large file — may be slow or costly',
    /** FR-KN-UP-1. */
    unsupported: (name: string) => `“${name}” can’t be used. Cairn accepts .md, .txt, .pdf, .docx, .png, .jpg and .jpeg files.`,
    readFailed: 'The file could not be read.',
  },
  diff: {
    none: 'No change was made.',
    onlyChanges: 'Show only the changes',
    wholePage: 'Show the whole page',
  },
}
