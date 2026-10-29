// Display labels and colours for enums, shared by all features. The names are in the interface
// language (i18n/en/labels.ts), read when called.
import { getT } from '@/i18n'
import type { PageOrigin, Priority, TaskStatus, TaskType, TintColor } from './types'

export const STATUS_ORDER: TaskStatus[] = ['todo', 'in_progress', 'blocked', 'done', 'wont_do']

export const statusLabel = (s: TaskStatus): string => getT().labels.status[s]

export const STATUS_TINT: Record<TaskStatus, TintColor> = {
  todo: 'slate',
  in_progress: 'blue',
  blocked: 'rose',
  done: 'green',
  wont_do: 'slate',
}

export const isClosed = (s: TaskStatus) => s === 'done' || s === 'wont_do'

export const TYPE_ORDER: TaskType[] = ['bug', 'feature', 'general']

export const typeLabel = (t: TaskType): string => getT().labels.type[t]

export const TYPE_TINT: Record<TaskType, TintColor> = { bug: 'rose', feature: 'green', general: 'blue' }

export const PRIORITY_ORDER: Priority[] = ['high', 'medium', 'low']

export const priorityLabel = (p: Priority): string => getT().labels.priority[p]

export const PRIORITY_TINT: Record<Priority, TintColor> = { high: 'rose', medium: 'amber', low: 'slate' }

export const originLabel = (o: PageOrigin): string => getT().labels.origin[o]

/** Tailwind classes for a pastel tint: soft background + matching text colour. */
export const TINT_CLASS: Record<TintColor, string> = {
  blue: 'bg-tint-blue text-tint-blue-fg',
  green: 'bg-tint-green text-tint-green-fg',
  purple: 'bg-tint-purple text-tint-purple-fg',
  amber: 'bg-tint-amber text-tint-amber-fg',
  rose: 'bg-tint-rose text-tint-rose-fg',
  slate: 'bg-tint-slate text-tint-slate-fg',
}

/** Text colour only. */
export const TINT_TEXT: Record<TintColor, string> = {
  blue: 'text-tint-blue-fg',
  green: 'text-tint-green-fg',
  purple: 'text-tint-purple-fg',
  amber: 'text-tint-amber-fg',
  rose: 'text-tint-rose-fg',
  slate: 'text-tint-slate-fg',
}

/** Stable tint for a free-text tag/label. */
export function tintForTag(tag: string): TintColor {
  const tints: TintColor[] = ['blue', 'green', 'purple', 'amber', 'rose']
  let h = 0
  for (const ch of tag) h = (h * 31 + ch.charCodeAt(0)) >>> 0
  return tints[h % tints.length]
}

/** Very soft tinted background for large surfaces (stat cards). */
export const TINT_SOFT_BG: Record<TintColor, string> = {
  blue: 'bg-tint-blue/45 border-tint-blue',
  green: 'bg-tint-green/45 border-tint-green',
  purple: 'bg-tint-purple/45 border-tint-purple',
  amber: 'bg-tint-amber/45 border-tint-amber',
  rose: 'bg-tint-rose/45 border-tint-rose',
  slate: 'bg-tint-slate/45 border-tint-slate',
}
