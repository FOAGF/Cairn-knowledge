// Working-day calculations for the completion summary (docs/requirements.md §4.4).
// Rules: date-based, inclusive, Monday–Friday, no public holidays.
// A working day is "blocked" if the task's status at the end of that day was
// Blocked; the status carries forward over days without a history version.

import type { DateStr, TaskHistoryVersion, TaskStatus } from './types'

function parseDate(d: DateStr): Date {
  const [y, m, day] = d.slice(0, 10).split('-').map(Number)
  return new Date(Date.UTC(y, m - 1, day))
}

function toDateStr(d: Date): DateStr {
  return d.toISOString().slice(0, 10)
}

function isWorkingDay(d: Date): boolean {
  const wd = d.getUTCDay()
  return wd !== 0 && wd !== 6
}

/** Calls `fn` for every calendar day from `start` to `end`, inclusive. */
function eachDay(start: DateStr, end: DateStr, fn: (d: Date) => void) {
  const cur = parseDate(start)
  const last = parseDate(end)
  while (cur <= last) {
    fn(cur)
    cur.setUTCDate(cur.getUTCDate() + 1)
  }
}

/** Working days from `start` to `end`, both inclusive. */
export function workingDaysBetween(start: DateStr, end: DateStr): number {
  let n = 0
  eachDay(start, end, (d) => {
    if (isWorkingDay(d)) n++
  })
  return n
}

/**
 * Working days on which the task ended the day as Blocked.
 * `history` may be in any order; only versions inside the range matter,
 * plus the last version before `start` (it sets the initial status).
 */
export function blockedWorkingDays(
  start: DateStr,
  end: DateStr,
  history: TaskHistoryVersion[],
  initialStatus: TaskStatus = 'todo',
): number {
  const byDate = new Map<DateStr, TaskStatus>()
  for (const v of history) byDate.set(v.date.slice(0, 10), v.snapshot.status)

  let status = initialStatus
  const before = [...byDate.keys()].filter((d) => d < start.slice(0, 10)).sort()
  if (before.length) status = byDate.get(before[before.length - 1])!

  let n = 0
  eachDay(start, end, (d) => {
    const s = byDate.get(toDateStr(d))
    if (s) status = s
    if (status === 'blocked' && isWorkingDay(d)) n++
  })
  return n
}
