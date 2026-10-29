// Mention autocomplete (§3.3): `#` suggests tasks, `[[` suggests pages (inserted as `[Title](page:<id>)`).
import type { Completion, CompletionContext, CompletionResult } from '@codemirror/autocomplete'
import { commands } from '@/api/bindings'
import { searchOnce } from '@/api/search'
import { fetchAllTasks } from '@/api/tasks'
import { pageLink } from '@/lib/mentions'
import { rememberPageTitle } from './pageTitles'

/** Tasks for `#query`: key or title contains it, key matches first (also the Assistant's box, FR-LLM-AS-14). */
export async function suggestTasks(query: string): Promise<{ key: string; title: string }[]> {
  const q = query.toLowerCase()
  const tasks = await fetchAllTasks().catch(() => [])
  return tasks
    .filter((t) => !q || t.key.toLowerCase().includes(q) || t.title.toLowerCase().includes(q))
    .sort((a, b) => Number(b.key.toLowerCase().includes(q)) - Number(a.key.toLowerCase().includes(q)))
    .slice(0, 30)
    .map((t) => ({ key: t.key, title: t.title }))
}

/** Pages for `[[query`: by search, or the newest ten when nothing is typed yet. */
export async function suggestPages(query: string): Promise<{ id: string; title: string }[]> {
  const q = query.trim()
  const pages = await (q
    ? searchOnce({ text: q, kinds: ['page'], limit: 10 }).then((hits) => hits.map((h) => ({ id: h.id, title: h.title })))
    : commands.listPages({ limit: 10 }).then((list) => list.map((p) => ({ id: p.id, title: p.title })))
  ).catch(() => [])
  for (const p of pages) rememberPageTitle(p.id, p.title)
  return pages
}

async function taskSource(ctx: CompletionContext): Promise<CompletionResult | null> {
  const m = ctx.matchBefore(/#[A-Za-z0-9-]*/)
  if (!m) return null
  const line = ctx.state.doc.lineAt(m.from)
  // "# Heading" at the start of a line is Markdown, not a mention.
  if (m.from === line.from) return null
  const before = ctx.state.doc.sliceString(Math.max(line.from, m.from - 1), m.from)
  if (/[\w&#]/.test(before)) return null
  const options: Completion[] = (await suggestTasks(m.text.slice(1))).map((t) => ({
    label: `#${t.key}`,
    detail: ` ${t.title}`,
    type: 'keyword',
    apply: `#${t.key}`,
  }))
  return { from: m.from, options, filter: false }
}

async function pageSource(ctx: CompletionContext): Promise<CompletionResult | null> {
  const m = ctx.matchBefore(/\[\[[^\]\n]*/)
  if (!m) return null
  const pages = await suggestPages(m.text.slice(2))
  const options: Completion[] = pages.map((p) => ({ label: `[[${p.title}`, displayLabel: p.title, type: 'text', apply: pageLink(p.title, p.id) }))
  // Swallow an existing "]]" after the cursor so we do not leave stray brackets.
  const after = ctx.state.doc.sliceString(ctx.pos, ctx.pos + 2)
  return { from: m.from, to: after === ']]' ? ctx.pos + 2 : ctx.pos, options, filter: false }
}

export const mentionSources = [taskSource, pageSource]
