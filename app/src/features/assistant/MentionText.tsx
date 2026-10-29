// A sent message's references as chips (FR-LLM-AS-14, D-81), the same chips pages show.
import type { ReactNode } from 'react'
import { PageMention, TaskMention } from '@/components/shared/MarkdownView'
import { PAGE_LINK_RE, TASK_MENTION_RE } from '@/lib/mentions'

/** A sent message's text with its references as chips (FR-LLM-AS-14); the rest stays plain text. */
export function MentionText({ text }: { text: string }) {
  const re = new RegExp(`${PAGE_LINK_RE.source}|${TASK_MENTION_RE.source}`, 'g')
  const parts: ReactNode[] = []
  let last = 0
  for (const m of text.matchAll(re)) {
    const at = m.index
    if (m[2] !== undefined) {
      parts.push(text.slice(last, at), <PageMention key={at} id={m[2]} fallback={m[1].replace(/\\(.)/g, '$1')} />)
      last = at + m[0].length
    } else {
      const start = at + m[3].length
      parts.push(text.slice(last, start), <TaskMention key={at} taskKey={m[4]} />)
      last = start + 1 + m[4].length
    }
  }
  parts.push(text.slice(last))
  return <>{parts}</>
}
