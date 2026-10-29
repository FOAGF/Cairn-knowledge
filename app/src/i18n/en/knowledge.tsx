import type { ReactNode } from 'react'
import { plural } from '../util'

// Knowledge (spec §5): the tree with *Select* and the bulk bar, the page view and its sidebar, history,
// the Markdown folder dialogs and *Suggest groups*.
export const knowledge = {
  ungrouped: 'Ungrouped',
  notChecked: 'Not checked',
  archived: 'Archived',
  newGroup: 'New group',
  deleteGroup: 'Delete group',
  /** Two sentences side by side (a space between them in English). */
  sentences: (first: ReactNode, second: ReactNode) => (
    <>
      {first} {second}
    </>
  ),
  screen: {
    description: 'Pages, grouped by category. Everything you want to remember.',
    checkAllUnchecked: (n: number) => `Check all unchecked (${n})`,
    importFromFolder: 'Import from a folder…',
    exportToFolder: 'Export to a folder…',
    createFromFile: 'Create from file',
    newGroupIn: (group: string) => `New group in “${group}”`,
    editGroup: 'Edit group',
    /** Stands for a category that isn't loaded, in "Directly in …". */
    theCategory: 'the category',
    deleteGroupTitle: (group: string) => `Delete group "${group}"?`,
    deleteGroupExternal: 'The pages and groups inside are not deleted. They move up a level, and the folder is removed when nothing else is left in it.',
    deleteGroupInternal: 'The pages inside are not deleted. They become ungrouped.',
  },
  overview: {
    noPages: 'No pages found',
    noPagesHint: 'Try clearing the search or filters.',
    notCheckedYet: (n: number) => plural(n, '# page not checked yet.', '# pages not checked yet.'),
    runChecksHint: 'Run the duplicate and diverse-content checks to keep the library tidy.',
    checkNow: 'Check now',
    counts: (pages: number, groups: number) => `${plural(pages, '# page', '# pages')} · ${plural(groups, '# group', '# groups')}`,
    ungroupedCount: (n: number) => `Ungrouped ${n}`,
    /** `ago` is "3 hours ago". */
    updated: (ago: string) => `Updated ${ago}`,
  },
  tree: {
    searchPlaceholder: 'Search title, text, tags…',
    filterByCategory: 'Filter by category',
    allCategories: 'All categories',
    filterByTag: 'Filter by tag',
    allTags: 'All tags',
    notCheckedOnly: 'Not checked only',
    showArchived: 'Show archived',
    selectHint: 'Tick pages to change many at once',
    select: 'Select',
    stopSelecting: 'Stop selecting',
    selectPagesOf: (name: string) => `Select the pages of ${name}`,
    selectUngroupedOf: (category: string) => `Select the ungrouped pages of ${category}`,
    newGroupIn: (category: string) => `New group in ${category}`,
    suggestGroupsHint: 'Suggest groups for the ungrouped pages',
    suggestGroupsIn: (category: string) => `Suggest groups in ${category}`,
    noPagesMatch: 'No pages match.',
    actionsFor: (group: string) => `Actions for ${group}`,
    editNameDescription: 'Edit name & description',
    newGroupInside: 'New group inside',
    moveTo: 'Move to…',
    emptyGroup: 'Empty group',
  },
  groupDialog: {
    externalHint: 'A group is a folder in the category’s folder, and can hold groups of its own.',
    internalHint: 'Groups are one level deep, like folders inside a category.',
    namePlaceholder: 'Group name',
    descriptionPlaceholder: 'Short description (optional), e.g. “How-to guides and checklists”',
    descriptionHint: 'Helps the assistant understand what belongs in this group when searching and suggesting groups.',
    moveTitle: (group: string) => `Move “${group}”`,
    moveDescription: 'Its folder moves, with everything in it.',
    moveInto: 'Move into',
    directlyIn: (category: string) => `Directly in ${category}`,
    /** A group path with no name in it (ensureGroupPath). */
    needsName: 'A group needs a name.',
  },
  bulk: {
    clashTitle: 'Nothing can be moved: some files would get the same name in one folder',
    clashHint: 'Rename these pages, or leave some out, then try again.',
    clashWithFile: (title: string) => `${title}, and a file already there`,
    planFailed: (message: string) => `The file names could not be checked: ${message}`,
    move: 'Move…',
    archiveTitle: (n: number) => plural(n, 'Archive # page?', 'Archive # pages?'),
    archiveDescription: 'Archived pages are hidden from the tree, search and the Assistant until restored. "Show archived" shows them.',
    archived: (n: number) => plural(n, '# page archived', '# pages archived'),
    restoreTitle: (n: number) => plural(n, 'Restore # page from the archive?', 'Restore # pages from the archive?'),
    restoreDescription: 'They show again in the tree, search and the Assistant.',
    restored: (n: number) => plural(n, '# page restored', '# pages restored'),
    moveTitle: (n: number) => plural(n, 'Move # page', 'Move # pages'),
    moveExternal:
      'Pages from another category move into it, and their files with them. If two files would get the same name in one folder, nothing is moved.',
    moveInternal:
      'Pages from another category move into it. A page whose title that category already has stays where it is, and is named afterwards.',
    targetCategory: 'Target category',
    chooseCategory: 'Choose a category',
    targetGroup: 'Target group',
    noGroup: 'No group',
    /** `target` is "Category › Group". */
    moved: (n: number, target: string) => `${plural(n, '# page', '# pages')} moved to ${target}`,
    addTagTitle: (n: number) => plural(n, 'Add a tag to # page', 'Add a tag to # pages'),
    addTagDescription: 'Type a tag, or pick one of the existing tags. Pages that already have it keep it once.',
    addTag: 'Add tag',
    tagAdded: (tag: string, n: number) => `Tag "${tag}" added to ${plural(n, '# page', '# pages')}`,
    removeTagTitle: (n: number) => plural(n, 'Remove a tag from # page', 'Remove a tag from # pages'),
    removeTagDescription: 'Pages without the tag stay as they are.',
    removeTag: 'Remove tag',
    tagRemoved: (tag: string, n: number) => `Tag "${tag}" removed from ${plural(n, '# page', '# pages')}`,
    markCheckedTitle: (n: number) => plural(n, 'Mark # page as checked?', 'Mark # pages as checked?'),
    markCheckedDescription: 'They leave "Check all unchecked" without a check by the model.',
    /** A page's check status in the confirmation's list. */
    notChecked: 'not checked',
    checked: 'checked',
    markChecked: 'Mark as checked',
    markedChecked: (n: number) => plural(n, '# page marked as checked', '# pages marked as checked'),
    deleteTitle: (n: number) => plural(n, 'Delete # page?', 'Delete # pages?'),
    deleteExternal: 'Their files go to the system trash. In Cairn their history and links are deleted for good. Archive is the reversible choice.',
    deleteInternal:
      'They are deleted for good, with their history, links and the images no other page uses. This cannot be undone; a backup can bring them back. Archive is the reversible choice.',
    deleteConfirm: (n: number) => plural(n, 'Delete # page', 'Delete # pages'),
    deleted: (n: number) => plural(n, '# page deleted', '# pages deleted'),
  },
  page: {
    loading: 'Loading…',
    notFound: 'Page not found',
    deletedHint: 'It may have been deleted.',
    backToKnowledge: 'Back to Knowledge',
    restoreTitle: 'Restore unsaved changes?',
    /** `ago` is "3 hours ago". */
    restoreHint: (ago: string) => `Edits from ${ago} were kept when you left this page.`,
    runChecks: 'Run checks',
    /** `task` is the task's key, a link. */
    createdFromTask: (task: ReactNode) => <>Created from task{' '}{task}</>,
    updated: (when: string) => `· Updated ${when}`,
    source: 'Source:',
    sourcePlaceholder: 'Where does this knowledge come from?',
    addSource: 'Add source info',
    spelling: (on: boolean): string => (on ? 'Spelling on' : 'Spelling off'),
    grammar: (on: boolean): string => (on ? 'Grammar on' : 'Grammar off'),
    editorPlaceholder: 'Write Markdown. Type # to mention a task, [[ to link a page.',
    imageTip: 'Tip: paste or drop a screenshot to add an image.',
    /** The floating toolbar while editing, before the page's title. */
    editing: 'Editing',
    saving: 'Saving…',
    words: (n: number) => `${n.toLocaleString()} words`,
    manyTopics: 'This page may cover more than one topic.',
  },
  sidebar: {
    properties: 'Properties',
    newGroupItem: 'New group…',
    linkedTasks: 'Linked tasks',
    findTask: 'Find a task…',
    noTasksFound: 'No tasks found.',
    noLinkedTasks: 'No tasks linked yet.',
    unlink: (key: string) => `Unlink ${key}`,
    mentionedIn: 'Mentioned in',
    mentionedInDescription: 'Pages, tasks and comments that link to this page',
    noMentions: 'Nothing mentions this page yet.',
    /** Marks a mention that is in a task's comment. */
    comment: 'comment',
    history: 'History',
    historyDescription: 'One version per day',
    noVersions: 'No versions yet.',
    restored: (date: string) => `Restored the version from ${date}`,
  },
  actions: {
    exportMd: 'Export as .md',
    findSimilar: 'Find similar pages',
    unarchive: 'Unarchive',
    restored: 'Page restored from the archive',
    archived: 'Page archived',
    deletePage: 'Delete page',
    deleteTitle: 'Delete this page?',
    deleteExternal: (title: string) => `"${title}" and its history will be removed from Cairn. Its file goes to the system trash.`,
    deleteInternal: (title: string) => `"${title}" and its history will be removed. This cannot be undone.`,
    deleted: 'Page deleted',
  },
  history: {
    versionFrom: (date: string) => `Version from ${date}`,
    diffHint: 'Compared with the current page: red lines were removed, green lines were added since then.',
    versionHint: 'How the page looked on that day.',
    changes: 'Changes',
    thatVersion: 'That version',
    restoreVersion: 'Restore this version',
    noDifferences: 'No differences from the current page.',
  },
  exportPage: {
    onlyInApp: 'Export is only available in the Cairn app.',
    /** The file type in the save dialog. */
    zipArchive: 'Zip archive',
    exported: 'Exported as Markdown',
    /** After the file's path in the toast. */
    images: (n: number) => plural(n, ' · # image', ' · # images'),
    title: 'Export as Markdown',
    description: (title: string) => `How should “${title}” be saved?`,
    folder: 'Folder',
    folderHint: 'A .md file, with its images in a “_files” folder next to it.',
    zip: 'Zip',
    zipHint: 'One .zip file with the page and its images.',
    chooseLocation: 'Choose location…',
  },
  conflict: {
    title: 'This page’s file changed while you were editing',
    description: 'Another program changed or replaced the file after you started editing. Which version should stay?',
    keepMine: 'Keep mine',
    keepMineExplain: (keepMine: ReactNode) => <>{keepMine} saves your version over the file.</>,
    takeDisk: 'Take the disk version',
    takeDiskExplain: (takeDisk: ReactNode) => <>{takeDisk} drops your changes and shows the file as it is now.</>,
    showDifferences: 'Show differences',
    fileGone: 'The file is no longer there: it was moved or deleted outside Cairn. Keep mine writes your version again.',
    diffHint: 'Red lines are only in the file on disk, green lines only in your version.',
    titles: (disk: ReactNode, mine: ReactNode) => (
      <>
        Title on disk: {disk} · yours: {mine}
      </>
    ),
    sameText: 'The text is the same in both; only the file’s other details (tags, source) may differ.',
    backToEditing: 'Back to editing',
    saving: 'Saving…',
  },
  suggest: {
    title: (category?: string) => (category ? `Suggest groups in ${category}` : 'Suggest groups'),
    description: 'Suggestions for pages that are in no group. Nothing changes until you approve it.',
    reading: 'Reading the ungrouped pages…',
    /** After the core's message when no AI model is set up. */
    unavailable: (message: string) => `${message} You can create groups by hand with the New group button and move pages into them.`,
    openSettings: 'Open Settings',
    noSuggestions: 'No suggestions: there are no ungrouped pages, or no group fits them.',
    group: (name: ReactNode) => <>Group: {name}</>,
    newGroup: (name: ReactNode, description: string) => (
      <>
        New group: {name}
        {description && ` — ${description}`}
      </>
    ),
    noGroupSkip: 'No group (skip)',
    movedTo: (group: string) => `Moved to ${group}`,
    approve: 'Approve',
    skip: 'Skip',
    allHandled: 'All suggestions handled.',
    done: 'Done',
    /** A proposed group that doesn't exist yet, in the *Change* list. */
    newChoice: (name: string) => `${name} (new)`,
  },
  tagInput: {
    placeholder: 'Add tag…',
    more: 'Add…',
    remove: (tag: string) => `Remove tag ${tag}`,
  },
  folders: {
    noFolder: 'No folder chosen',
    change: 'Change…',
    choose: 'Choose folder…',
    /** The tick under the list of what is replaced. */
    confirmReplace: (n: number) => (n === 1 ? 'I have read the list and want it replaced.' : `I have read the list and want these ${n} replaced.`),
    collapse: 'Collapse',
    expand: 'Expand',
  },
  importFolder: {
    title: 'Import Markdown from a folder',
    description: 'Every .md file in the folder and its subfolders becomes a page in one category and group. The file name is the title.',
    noGroup: 'No group',
    newGroupItem: 'New group…',
    newGroupPlaceholder: 'Name of the new group',
    /** The plan: "3 new pages, 1 replaced, 2 skipped." The parts may be left out (false). */
    summary: (created: ReactNode, replaced: ReactNode, skipped: ReactNode) => (
      <>
        {created}
        {replaced && <>, {replaced}</>}
        {skipped && <>, {skipped}</>}.
      </>
    ),
    newPages: (n: number) => plural(n, '# new page', '# new pages'),
    replaced: (n: number) => `${n} replaced`,
    skipped: (n: number) => `${n} skipped`,
    imagesCopied: (n: number) => plural(n, ' # image is copied in.', ' # images are copied in.'),
    missingImages: (n: number) =>
      plural(
        n,
        " # image link can't be used (no such file, or not PNG, JPEG, GIF or WebP) and stay as written.",
        " # image links can't be used (no such file, or not PNG, JPEG, GIF or WebP) and stay as written.",
      ),
    newPagesList: (n: number) => `New pages (${n})`,
    skippedList: (n: number) => `Skipped files (${n})`,
    replaceTitle: (n: number) => plural(n, '# page in this category will be replaced', '# pages in this category will be replaced'),
    replaceExplain:
      "Each page's text is replaced by the file's text; its title and group stay, and tags from the file are added. A backup of all your data is made first. A version from an earlier day stays in the page's history, but today's version is overwritten.",
    archivedSuffix: ' (archived)',
    importing: 'Importing…',
    importAndReplace: (n: number) => `Import and replace ${n}`,
    import: 'Import',
    imported: (created: number, replaced: number, skipped: number) =>
      `Imported: ${[`${created} new`, replaced ? `${replaced} replaced` : '', skipped ? `${skipped} skipped` : ''].filter(Boolean).join(', ')}`,
    backupMade: (path: string) => `A backup of all data was made first: ${path}`,
  },
  exportFolder: {
    title: 'Export Markdown to a folder',
    description:
      'Choose pages, or whole groups and categories (archived pages included). Pages from one group go straight into the folder; pages from several groups get a folder per group, and from several categories a folder per category.',
    noGroupBucket: 'Pages in no group',
    filterPlaceholder: 'Filter pages by title',
    chosen: (n: number) => `${n} chosen`,
    clear: 'Clear',
    /** The count of a category or group without pages. */
    noPages: 'no pages',
    archived: 'archived',
    noPagesMatch: 'No pages match',
    folder: 'Folder',
    choosePages: 'Choose the pages to export on the left.',
    chooseFolder: 'Choose the folder to export to.',
    /** "3 files will be written, 1 of them replacing files already there." `replacing` may be false. */
    willBeWritten: (files: ReactNode, replacing: ReactNode) => (
      <>
        {files}{' '}will be written{replacing && <>, {replacing}</>}.
      </>
    ),
    files: (n: number) => plural(n, '# file', '# files'),
    replacing: (n: number) => `${n} of them replacing files already there`,
    replaces: '· replaces',
    replaceTitle: (n: number) => plural(n, '# file in the folder will be replaced', '# files in the folder will be replaced'),
    replaceExplain: 'These files are already in the folder and are overwritten, with their images. Other files in the folder are left alone.',
    exporting: 'Exporting…',
    exportAndReplace: (n: number) => `Export and replace ${n}`,
    exportPages: (n: number) => (n === 0 ? 'Export pages' : plural(n, 'Export # page', 'Export # pages')),
    exported: (written: number, replaced: number) => `Exported ${plural(written, '# page', '# pages')}${replaced ? `, ${replaced} replaced` : ''}`,
  },
}
