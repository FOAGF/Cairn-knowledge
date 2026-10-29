import type { ReactNode } from 'react'
import { plural } from '../util'

/** The texts of one kind of name in the bulk bar's picker: a task's label or a page's tag. */
const namePicker = (noun: string) => ({
  placeholder: `A new or existing ${noun}`,
  toAdd: `The ${noun} to add`,
  existing: `Existing ${noun}s`,
  toRemove: `Choose the ${noun} to remove`,
  list: `${noun}s`,
  isNew: (name: string) => `"${name}" is a new ${noun}.`,
})

// Building blocks used by many screens (components/shared) and the shadcn primitives (components/ui).
export const shared = {
  bulk: {
    toolbar: 'Selection',
    selected: (n: number) => `${n} selected`,
    clear: 'Clear the selection',
    skippedTitle: (n: number) => `${plural(n, '# item', '# items')} left as they were`,
    /** `done`: the notice for the items that changed, e.g. "3 tasks set to Done"; null when none did. */
    skippedDescription: (done: string | null) => `${done ? `${done}. ` : 'Nothing was changed. '}These could not take the change:`,
    gone: 'An item that no longer exists',
    items: 'Items to change',
    working: 'Working…',
    /** The confirm button while it is locked. */
    countdown: (label: string, seconds: number) => `${label} (${seconds})`,
  },
  namePicker: { label: namePicker('label'), tag: namePicker('tag') },
  colorPicker: {
    custom: 'Custom colour',
    customHex: 'Custom colour as hex',
    hint: 'Pick a colour, or any colour with the colour wheel or a hex value.',
    /** The preset colours' names, on hover. */
    colors: { blue: 'blue', green: 'green', purple: 'purple', amber: 'amber', rose: 'rose', slate: 'slate' },
  },
  fold: {
    /** `noun`: plural, in the interface language, e.g. "tasks". */
    showMore: (n: number, noun: string) => `Show ${n} more ${noun}`,
  },
  glossaryTerm: {
    global: 'Global',
    unknownCategory: 'Unknown category',
    also: (names: string[]) => `Also: ${names.join(', ')}`,
    open: 'Open in glossary',
  },
  markdown: {
    taskNotFound: 'Task not found',
    pageNotFound: 'Page not found',
    /** A page mention with no stored title. */
    page: 'page',
    loadingImage: 'Loading image',
    /** An image that can't be shown. */
    image: (name: string) => `[image: ${name}]`,
    inFolder: (path: string) => `In the knowledge folder: ${path}`,
  },
  meaning: {
    partial: (ready: number, items: number) => `Search by meaning covers ${ready} of ${items} items (indexing…)`,
    unavailable: 'Search by meaning unavailable — keyword results only',
  },
  placeholder: 'This screen is being built.',
  savedAs: {
    rules: 'Pages are files, so their names follow Windows’ rules: < > : " / \\ | ? * become -, and dots and spaces at the start and end are dropped.',
    label: (name: ReactNode) => <>Saved as: {name}</>,
  },
  taskStatus: {
    saveKnowledge: 'Save knowledge from this task?',
    createKnowledge: 'Create knowledge',
    change: (status: string) => `Status: ${status}. Change status`,
  },
  tint: {
    remove: (tag: string) => `Remove ${tag}`,
    more: (names: string[]) => `${names.length} more: ${names.join(', ')}`,
    priority: (priority: string) => `${priority} priority`,
  },
  ui: {
    commandTitle: 'Command Palette',
    commandDescription: 'Search for a command to run...',
    /** The toasts' region and their close button, for screen readers. */
    notifications: 'Notifications',
    closeToast: 'Close toast',
  },
}
