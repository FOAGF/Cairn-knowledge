import { differenceInCalendarDays, format, formatDistanceToNowStrict, parseISO, type Locale } from 'date-fns'
import { zhCN } from 'date-fns/locale/zh-CN'
import { getLang, getT } from '@/i18n'

// Dates in the interface language (NFR-I18N-2, D-79): the words and patterns are in
// i18n/<lang>/dates.ts, read when called. A component showing only a date calls `useLang()`.

/** date-fns's locale for the interface language; English uses date-fns's default. */
export function dateLocale(): Locale | undefined {
  return getLang() === 'zh-CN' ? zhCN : undefined
}

type Pattern = keyof ReturnType<typeof getT>['dates']['pattern']

/** A date (or ISO text) in one of the interface's patterns. */
export function formatDate(date: Date | string, pattern: Pattern): string {
  return format(typeof date === 'string' ? parseISO(date) : date, getT().dates.pattern[pattern], { locale: dateLocale() })
}

/** "2 hours ago", "3 days ago". */
export function timeAgo(iso: string): string {
  return formatDistanceToNowStrict(parseISO(iso), { addSuffix: true, locale: dateLocale() })
}

/** "in 3 days", "in 5 hours": when something in the future happens (rounded up). */
export function timeUntil(iso: string): string {
  return formatDistanceToNowStrict(parseISO(iso), { addSuffix: true, roundingMethod: 'ceil', locale: dateLocale() })
}

/** Due-date label like the design: "Today", "Tomorrow", "Wed, 15 Oct", or "3 days overdue". */
export function dueLabel(date: string): { text: string; tone: 'overdue' | 'today' | 'soon' | 'normal' } {
  const t = getT().dates
  const diff = differenceInCalendarDays(parseISO(date), new Date())
  if (diff < 0) return { text: t.overdue(-diff), tone: 'overdue' }
  if (diff === 0) return { text: t.today, tone: 'today' }
  if (diff === 1) return { text: t.tomorrow, tone: 'soon' }
  return { text: formatDate(date, 'shortDay'), tone: diff <= 3 ? 'soon' : 'normal' }
}

/** A closed task's due-date label, fixed when it closed: "3 days overdue" if it closed after its due date, else the date. */
export function closedDueLabel(date: string, completedAt: string | null): string {
  const late = completedAt ? differenceInCalendarDays(parseISO(completedAt), parseISO(date)) : 0
  return late > 0 ? getT().dates.overdue(late) : formatDate(date, 'shortDay')
}

/** "13 Oct 2026". */
export function fullDate(iso: string): string {
  return formatDate(iso, 'date')
}

/** "13 Oct 2026, 14:05". */
export function dateTime(iso: string): string {
  return formatDate(iso, 'dateTime')
}

/** Word count where each CJK character counts as one word (FR-KN-ED-4). */
export function countWords(text: string): number {
  const cjk = text.match(/[㐀-鿿豈-﫿]/g)?.length ?? 0
  const latin = text.replace(/[㐀-鿿豈-﫿]/g, ' ').match(/[A-Za-z0-9]+(?:['’-][A-Za-z0-9]+)*/g)?.length ?? 0
  return cjk + latin
}

/** As `fileSize`, in GB from 1 GB (1024³ bytes) on: the switch backups (FR-KN-EXT-9). */
export function largeFileSize(bytes: number) {
  return bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : fileSize(bytes)
}

/** Human-readable size, e.g. `1.4 MB` (exports, backups, logs). */
export function fileSize(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}
