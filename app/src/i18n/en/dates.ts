import { plural } from '../util'

// Words and date-fns patterns for dates (lib/format.ts uses them).
export const dates = {
  today: 'Today',
  tomorrow: 'Tomorrow',
  overdue: (days: number) => plural(days, '# day overdue', '# days overdue'),
  pattern: {
    /** "Wed, 15 Oct" */
    shortDay: 'EEE, d MMM',
    /** "Wed, 15 Oct 2026" */
    day: 'EEE, d MMM yyyy',
    /** "15 Oct 2026" */
    date: 'd MMM yyyy',
    /** "15 Oct 2026, 14:05" */
    dateTime: 'd MMM yyyy, HH:mm',
    /** "Wednesday 15 October" */
    weekdayDayMonth: 'EEEE d MMMM',
    /** "Wednesday, 15 October 2026" */
    longDate: 'EEEE, d MMMM yyyy',
  },
}
