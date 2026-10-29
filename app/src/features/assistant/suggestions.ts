// The Assistant's starting templates (D-64). A card puts its template into the message box instead of
// sending it: the user fills in the [blanks] (the first is selected, Tab goes to the next) and sends.
// One per kind of request the assistant handles: a question about the notes, a new task, a status
// change and a list of tasks. A new task or a change is still only a proposal until confirmed (FR-LLM-AS-3).
import { BookOpen, CircleCheckBig, FilePlus2, ListChecks } from 'lucide-react'
import { getT } from '@/i18n'
import type { TintColor } from '@/lib/types'

export interface Template {
  icon: typeof BookOpen
  tint: TintColor
  /** What kind of request it is, shown above the text. */
  label: string
  /** The text put into the message box; each `[blank]` is for the user to fill in. */
  text: string
}

const KINDS = [
  { kind: 'ask', icon: BookOpen, tint: 'purple' },
  { kind: 'create', icon: FilePlus2, tint: 'green' },
  { kind: 'update', icon: CircleCheckBig, tint: 'blue' },
  { kind: 'find', icon: ListChecks, tint: 'rose' },
] as const

/** The templates in the interface language (their texts are in i18n/<lang>/assistant.tsx). */
export function templates(): Template[] {
  const t = getT().assistant.templates
  return KINDS.map(({ kind, icon, tint }) => ({ icon, tint, ...t[kind] }))
}

/** The blanks of a template, in order, e.g. `["[topic]"]`. */
export function blanksOf(text: string): string[] {
  return text.match(/\[[^\]\n]+\]/g) ?? []
}

/** The template's text and its blanks, split for display (blanks are highlighted). */
export function templateParts(text: string): { text: string; blank: boolean }[] {
  return text.split(/(\[[^\]\n]+\])/).filter(Boolean).map((t) => ({ text: t, blank: /^\[[^\]\n]+\]$/.test(t) }))
}
