// The word tokenizer and the `Match` shape the duplicate lists use.
import type { PageSummary } from '@/api/bindings'

/** Lower-case Latin words (3+ chars) plus CJK character bigrams. */
export function tokenize(text: string): string[] {
  const lower = text.toLowerCase()
  const words = lower.match(/[a-z0-9]{3,}/g) ?? []
  const cjk = lower.match(/[㐀-鿿]+/g) ?? []
  const bigrams = cjk.flatMap((run) => (run.length === 1 ? [run] : [...run].slice(0, -1).map((c, i) => c + run[i + 1])))
  return [...words.filter((w) => !STOP.has(w)), ...bigrams]
}

const STOP = new Set(['the', 'and', 'for', 'with', 'this', 'that', 'from', 'are', 'was', 'what', 'how', 'why', 'when', 'into', 'all', 'can', 'not', 'any', 'have', 'has', 'you', 'your', 'about'])

export interface Match {
  page: PageSummary
  score: number
  /** Plain excerpt around the first match. */
  snippet: string
}
