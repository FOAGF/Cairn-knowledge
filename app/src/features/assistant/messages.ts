// Pure helpers on stored chat messages (from Rust, api/conversations.ts).
import type { ChatMessage, DraftCard } from '@/api/conversations'

/** The latest card the user can still refine: not saved, discarded or replaced (same rule as Rust's `active_card`). */
export function activeDraftMessage(messages: ChatMessage[]): ChatMessage | undefined {
  for (let i = messages.length - 1; i >= 0; i--) {
    const d = messages[i].draft
    if (d && d.state !== 'saved' && !d.discarded && !d.superseded) return messages[i]
  }
  return undefined
}

/** The card can still be changed or sent to review. */
export const isActiveCard = (d: DraftCard) => d.state !== 'saved' && !d.discarded && !d.superseded

/**
 * An assistant message whose job is still working: a step is running, or the job has just started
 * (no steps yet, nothing to show). A job that failed or was stopped has an error; one that
 * finished has a card, an answer, a find list, a proposal or text. Jobs from before a restart are
 * settled by Rust at startup.
 */
export function isRunning(m: ChatMessage): boolean {
  if (m.role !== 'assistant' || m.error || m.draft || m.answer || m.find || m.proposal) return false
  if (m.steps.some((s) => s.state === 'active')) return true
  return m.steps.length === 0 && !m.text && !m.extra
}

/** Whether the question was sent with 🌐: the reply after it came from, or tried, a web search (FR-LLM-AS-12). */
export function askedWeb(reply: ChatMessage | undefined): boolean {
  return !!reply && (reply.answer?.basis === 'web' || reply.answer?.basis === 'no_web_search' || reply.steps.some((s) => /web/i.test(s.label)))
}

/** Turns `[n]` into a link `cite:n` for numbers that name a source; `[n](…)` is a real link and stays. */
export function markersToLinks(text: string, count: number): string {
  return text.replace(/\[(\d{1,3})\](?!\()/g, (m, n: string) => (Number(n) >= 1 && Number(n) <= count ? `[${n}](cite:${n})` : m))
}
