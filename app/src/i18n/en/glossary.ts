import { plural } from '../util'

// The glossary screen (UI-KN-8) and the term suggestions under a draft (FR-KN-GL-7).
export const glossary = {
  description: 'What your work terms mean. Search and the assistant use it.',
  loading: 'Loading the glossary…',
  loadFailed: 'Could not load the glossary',
  newEntry: 'New entry',
  editEntry: (term: string) => `Edit "${term}"`,
  empty: 'The glossary is empty',
  notFound: 'Entry not found',
  nothingSelected: 'Nothing selected',
  emptyHint: 'Add the abbreviations and internal names you use at work.',
  pickHint: 'Pick a term on the left, or add a new one.',
  deleteTitle: (term: string) => `Delete "${term}"?`,
  deleteDescription: 'The entry is removed from the glossary. Pages and tasks that use the term are not changed.',
  deleteConfirm: 'Delete entry',
  /** A scope: the entry applies in every category. */
  global: 'Global',
  otherNames: 'Other names',
  list: {
    entries: 'Entries',
    add: 'Add entry',
    search: 'Search term, names, description…',
    scopeFilter: 'Filter by scope',
    allScopes: 'All scopes',
    noMatch: 'No entries match.',
    overridesGlobal: 'overrides global',
    unknownCategory: 'Unknown category',
  },
  detail: {
    overrides: 'overrides the global entry in this category',
    overriddenIn: (categories: string[]) => `Overridden in ${categories.join(', ')}`,
    description: 'Description',
    noDescription: 'No description.',
    related: 'Related terms',
    noRelated: 'None. Naming another glossary term in the description links the two.',
  },
  /** Where the term is used (UI-KN-8). */
  usedIn: {
    title: (n: number) => (n > 0 ? plural(n, 'Used in # place', 'Used in # places') : 'Used in'),
    description: "Pages and tasks whose text uses this term (the category's own entries override global ones).",
    looking: 'Looking for uses…',
    /** `message` comes from the core. */
    loadFailed: (message: string) => `Could not load the uses: ${message}`,
    none: 'Not used in any page or task yet.',
    showing: (shown: number, total: number) => `Showing ${shown} of ${total}`,
    loading: 'Loading…',
    showMore: (n: number) => `Show ${n} more`,
  },
  form: {
    required: 'The term is required.',
    duplicate: (term: string) => `"${term}" already exists in this scope. Edit that entry, or pick another scope.`,
    term: 'Term',
    termPlaceholder: 'e.g. KPI',
    otherNamesPlaceholder: 'Long form, synonyms… (Enter to add)',
    description: 'Short description',
    descriptionPlaceholder: 'A few lines. Naming another glossary term here links the two entries.',
    scope: 'Scope',
  },
  /** "N possible glossary terms" (FR-KN-GL-7). */
  suggestions: {
    count: (n: number) => plural(n, '# possible glossary term', '# possible glossary terms'),
    /** How sure the model is of the meaning. */
    confidence: {
      high: 'explained in the text',
      medium: 'likely meaning',
      low: 'a guess',
      unknown: 'meaning unknown',
    },
    noMeaning: 'Cairn can’t know what this means. Add it and write the meaning.',
    from: (pages: string[]) => `From ${pages.map((p) => `“${p}”`).join(', ')}`,
    ignore: 'Ignore',
    addTitle: 'Add to the glossary',
  },
}
