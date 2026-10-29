// The Markdown editor: Edit | Preview, the hints, the draft prompt and the CodeMirror extensions.
export const editor = {
  mode: {
    /** aria-label of the Edit | Preview switch. */
    label: 'Editor mode',
    preview: 'Preview',
  },
  hints: {
    button: 'Hints',
    title: 'Markdown in Cairn',
    pageLink: "Link a page: type [[ and pick one. It shows as a chip with the page's current title, so renaming the page never breaks the link.",
    taskMention: 'Mention a task by its key: type # and pick one. The mention keeps working after the task moves to another category.',
    images: 'Images',
    imagesText: 'Paste or drop a screenshot to add it. The source shows ![](attachment:…); Preview shows the image.',
    /** External mode: a page's images are files in its category folder (FR-KN-EXT-3). */
    folderImagesText:
      'Paste or drop a screenshot to add it. It is saved in the category folder’s attachments folder; the source shows ![](folder:…), and Preview shows the image.',
    glossaryTerms: 'Glossary terms',
    glossaryText: 'Write them as usual. Known work terms get a dotted underline with their meaning on hover.',
    latex:
      'A LaTeX formula: $…$ inside a line, or $$ on lines of their own for a centred one; \\(…\\) and \\[…\\] work too. Preview shows it. Amounts such as $5 stay plain text.',
    /** The footer; the code samples are written in the interface language. */
    rest: (
      <>
        Everything else is standard Markdown: <code># Heading</code> (with a space), <code>**bold**</code>, <code>*italic*</code>, lists,{' '}
        <code>- [ ]</code> checklists, tables, <code>`code`</code> and <code>&gt; quotes</code>.
      </>
    ),
  },
  previewEmpty: 'Nothing to preview yet.',
  /** DraftRestorePrompt (FR-KN-ED-10); `ago` is "3 hours ago". */
  restoreDraft: (ago: string) => `Restore unsaved changes from ${ago}?`,
  image: {
    /** Stands in the text while a pasted image is saved. */
    saving: (n: number) => `[Saving image… #${n}]`,
    saveFailed: (message: string) => `Could not save the image: ${message}`,
    dropOnlyImages: 'Only PNG or JPEG images can be dropped into the text.',
  },
  glossary: {
    /** The tooltip's scope for a term of the global glossary. */
    global: 'Global',
    otherNames: (names: string[]) => `Also: ${names.join(', ')}`,
  },
  lint: {
    addToDictionary: 'Add to dictionary',
    /** A suggestion that deletes the text. */
    remove: '(remove)',
    spelling: 'Spelling',
    grammar: 'Grammar',
  },
  /** A page chip whose stored title is empty. */
  pageChip: 'page',
}
