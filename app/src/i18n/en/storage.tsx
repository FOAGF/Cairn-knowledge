import type { ReactNode } from 'react'
import { plural } from '../util'

// Where knowledge pages live (spec §5.11, D-72): the first-start chooser, the switch in progress, the
// switch backups' start-up warning, and the folder scans (api/storage.ts).
export const storage = {
  /** The system folder picker's title (lib/pickFolder.ts). */
  pickFolder: {
    knowledge: 'Choose the folder for your knowledge pages',
    anotherKnowledge: 'Choose another folder for your knowledge pages',
    onlyInApp: 'Folders can only be chosen in the Cairn app.',
  },
  /** The folder can't be watched; `error` comes from Rust. */
  notWatched: {
    title: 'Changes made in other programs aren’t picked up live',
    toast: (error: string) => `${error} Rescan in Settings › General picks them up.`,
  },
  /** "Updating the index: 340 of 1,200"; `step` comes from Rust. */
  progress: (step: string, done: string, total: string) => `${step}: ${done} of ${total}`,
  /** The first step of a switch, until Rust reports its own. */
  starting: 'Starting',
  scan: {
    reading: 'Reading the knowledge folder',
    failed: (error: string) => `The knowledge folder could not be read: ${error}`,
    skipped: (n: number) => plural(n, '# file in the knowledge folder left out', '# files in the knowledge folder left out'),
    /** `list` is the first files left out (with Rust's reasons); `more` how many others. */
    skippedDescription: (list: string, more: number) => `${list}${more > 0 ? ` …and ${more} more.` : ''} Settings › General lists them.`,
  },
  /** FR-KN-EXT-7. */
  modeChooser: {
    welcome: 'Welcome to Cairn',
    intro: 'First, choose where your knowledge pages live. Tasks work the same either way.',
    aria: 'Where knowledge pages live',
    internal: 'Internal mode',
    internalBody: 'Pages are kept in Cairn’s own database, in its data folder.',
    internalTogether: 'Everything stays together: backups and exports include every page.',
    internalOtherPrograms: 'Other programs can’t open the pages directly. You can export them as Markdown files at any time.',
    external: 'External mode',
    externalBody: 'Pages are .md files in a folder you choose. Cairn keeps an index of it for search, links and the Assistant.',
    externalFolders: 'Each folder in it is a category, and the folders inside those are groups.',
    externalTools: 'Other tools (a text editor, Obsidian, git, OneDrive) can read and change the same files. Cairn picks up their changes.',
    externalBackups: 'Cairn’s backups don’t include the folder: keep it safe with your own tools.',
    noFolder: 'No folder chosen',
    change: 'Change…',
    chooseFolder: 'Choose folder…',
    sample: 'Use the sample folder (development)',
    laterTitle: 'You can change this later',
    laterBody:
      'Either way, tasks, categories, the glossary and conversations stay in Cairn’s database. Settings › General can switch between the two; every switch first backs up the knowledge pages.',
    useFolder: 'Use this folder',
    chooseFirst: 'Choose a folder first',
    keepInCairn: 'Keep pages in Cairn',
    chooseOne: 'Choose one to continue',
  },
  /** The switch in progress, then what it did (FR-KN-EXT-8). */
  switchDialog: {
    toExternal: 'Switching to external mode',
    toFolder: 'Switching to the other folder',
    toInternal: 'Switching to internal mode',
    switching: 'Switching where knowledge lives',
    failed: 'The switch didn’t finish',
    nowInternal: 'Now in internal mode',
    nowFolder: 'Now using the new folder',
    nowExternal: 'Now in external mode',
    keepOpen: 'Please keep Cairn open until it finishes. Tasks and everything else are not changed.',
    backupKept: (file: ReactNode, size: string) => (
      <>
        A backup of the knowledge pages from before the switch is kept in Switch backups: {file} ({size}).
      </>
    ),
    internalResult: (restored: number) =>
      `${restored > 0 ? `${plural(restored, '# page', '# pages')} loaded from the switch backup.` : 'Starting with no pages.'} The folder was left as it is.`,
    pagesInFolder: (n: number) => `${plural(n, '# page', '# pages')} in the folder.`,
    detailsBack: (n: number) =>
      `Cairn’s own details (checks, task links, history) came back for ${plural(n, '# page', '# pages')} from an earlier switch backup.`,
    skipped: (n: number) => `${plural(n, '# file', '# files')} left out of the index; Settings › General lists them.`,
    previousLeft: 'The previous folder was left as it is. Switching back to it later brings Cairn’s details for its pages back.',
    searchRebuilt: 'Search by meaning is rebuilt in the background. Keyword search works now.',
  },
  /** FR-KN-EXT-9: the start-up pop-up. */
  backupWarning: {
    title: (size: string) => `Switch backups take ${size}`,
    body: (warnSize: string) =>
      `Cairn keeps a backup of the knowledge pages from every switch: the newest for each folder, and every one from internal mode. Together they are now larger than ${warnSize}. Deleting the ones you no longer need frees the space.`,
    skip: 'Skip for now',
    openFolder: 'Open folder',
    openList: 'Open switch backups',
  },
}
