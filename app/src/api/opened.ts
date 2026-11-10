// What was opened last (D-94): the `#` and `[[` suggestion lists show the most recently opened
// tasks and pages first. Opening a task or page, or picking it from a list, records it.
import { commands, type OpenedKind } from './bindings'

/** Records a task or page as opened; a failure only costs its place in the suggestion lists. */
export function recordOpened(kind: OpenedKind, id: string) {
  commands.recordOpened(kind, id).catch(() => {})
}

/** The ids of the most recently opened tasks or pages, most recent first. */
export function recentlyOpened(kind: OpenedKind, limit = 100): Promise<string[]> {
  return commands.recentlyOpened(kind, limit).catch(() => [])
}

/**
 * `items` with the ones opened before first, most recent first (at most `head` of them), then the
 * rest in their own order.
 */
export function openedFirst<T>(items: T[], idOf: (item: T) => string, recent: string[], head = Infinity): T[] {
  const rank = new Map(recent.map((id, i) => [id, i]))
  const opened = items
    .filter((item) => rank.has(idOf(item)))
    .sort((a, b) => rank.get(idOf(a))! - rank.get(idOf(b))!)
    .slice(0, head)
  const first = new Set(opened)
  return [...opened, ...items.filter((item) => !first.has(item))]
}
