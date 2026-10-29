import type { ReactNode } from 'react'
import { plural } from '../util'

// Home (FR-HOME-1..6): the greeting, the counts, *Needs attention*, Today, the calendar and the
// first-start card.
export const home = {
  greeting: {
    morning: 'Good morning',
    afternoon: 'Good afternoon',
    evening: 'Good evening',
  },
  /** The greeting with the user's name as typed. */
  greetingNamed: (greeting: string, name: string) => `${greeting}, ${name}`,
  loading: 'Loading…',
  /** The counts under the greeting; `num` is the number, drawn in bold (FR-HOME-2). */
  counts: {
    active: (num: ReactNode) => <>{num} active</>,
    blocked: (num: ReactNode) => <>{num} blocked</>,
    dueThisWeek: (num: ReactNode) => <>{num} due this week</>,
    overdue: (num: ReactNode) => <>{num} overdue</>,
    doneThisWeek: (num: ReactNode) => <>{num} done this week</>,
    pages: (num: ReactNode, n: number) => <>{num} {n === 1 ? 'page' : 'pages'}</>,
    categories: (num: ReactNode, n: number) => <>{num} {n === 1 ? 'category' : 'categories'}</>,
  },
  /** FR-HOME-3. */
  attention: {
    title: 'Needs attention',
    notChecked: (n: number) => plural(n, '# page not checked', '# pages not checked'),
    /** `error` comes from the core. */
    backupFailed: (error: string) => `Last backup failed: ${error}`,
    checkFailed: 'The database check before the last backup failed',
    noModel: 'No AI model set up',
  },
  /** FR-HOME-4. */
  today: {
    title: 'Today',
    description: (date: string, n: number) => `${date} · ${n === 0 ? 'nothing needs you' : `${n} task${n === 1 ? ' needs' : 's need'} you`}`,
    empty: 'Nothing overdue, due today, in progress or blocked.',
    allTasks: 'All tasks',
    group: {
      overdue: 'Overdue',
      due_today: 'Due today',
      in_progress: 'In progress',
      blocked: 'Blocked',
    },
    showMore: (n: number) => `Show ${n} more`,
    allInTasks: (total: number) => `All ${total} in Tasks`,
  },
  /** A day picked in the calendar (FR-HOME-5). */
  day: {
    due: (n: number) => `${n} due`,
    back: 'Back to today',
    empty: 'Nothing due that day.',
  },
  calendar: {
    title: 'Calendar',
    hint: 'Click a day to see what’s due then.',
    tasksDue: (n: number) => plural(n, '# task due', '# tasks due'),
  },
  /** FR-DATA-8: a copy with no data yet. */
  firstStart: {
    title: 'Welcome to Cairn',
    description: 'This copy has no data yet. How do you want to begin?',
    fresh: 'Start fresh',
    import: 'Import from a backup or export…',
    keysTitle: 'API keys are entered again',
    keysText: 'Backups and exports never contain API keys. After an import, enter your keys again in Settings › AI providers.',
  },
}
