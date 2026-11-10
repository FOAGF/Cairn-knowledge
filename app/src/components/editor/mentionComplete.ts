// Mention autocomplete (§3.3): `#` suggests tasks, `[[` suggests pages (inserted as `[Title](page:<id>)`).
// The tasks and pages opened last come first, and picking one counts as opening it (D-94).
import { type Completion, type CompletionContext, type CompletionResult, insertCompletionText, pickedCompletion } from '@codemirror/autocomplete'
import { commands, type OpenedKind } from '@/api/bindings'
import { openedFirst, recentlyOpened, recordOpened } from '@/api/opened'
import { searchOnce } from '@/api/search'
import { fetchAllTasks } from '@/api/tasks'
import { pageLink } from '@/lib/mentions'
import { rememberPageTitle } from './pageTitles'

/** With nothing typed yet, the lists start with at most this many opened before (D-94). */
const OPENED_FIRST = 10

/**
 * Tasks for `#query`: key or title contains it, key matches first (also the Assistant's box,
 * FR-LLM-AS-14). D-94: the ones opened before come first, most recent first.
 */
export async function suggestTasks(query: string): Promise<{ id: string; key: string; title: string }[]> {
  const q = query.toLowerCase()
  const [tasks, recent] = await Promise.all([fetchAllTasks().catch(() => []), recentlyOpened('task')])
  const matches = tasks
    .filter((t) => !q || t.key.toLowerCase().includes(q) || t.title.toLowerCase().includes(q))
    .sort((a, b) => Number(b.key.toLowerCase().includes(q)) - Number(a.key.toLowerCase().includes(q)))
  return openedFirst(matches, (t) => t.id, recent, q ? Infinity : OPENED_FIRST)
    .slice(0, 30)
    .map((t) => ({ id: t.id, key: t.key, title: t.title }))
}

/**
 * Pages for `[[query`: by search, or the newest ten when nothing is typed yet. D-94: the ones opened
 * before come first, most recent first.
 */
export async function suggestPages(query: string): Promise<{ id: string; title: string }[]> {
  const q = query.trim()
  const recent = await recentlyOpened('page')
  const found = q
    ? // More hits than shown, so a page opened lately can move up from further down.
      searchOnce({ text: q, kinds: ['page'], limit: 30 }).then((hits) => hits.map((h) => ({ id: h.id, title: h.title })))
    : Promise.all([
        recent.length ? commands.getPagesByIds(recent.slice(0, OPENED_FIRST)) : [],
        commands.listPages({ limit: 10 }),
      ]).then(([opened, newest]) => {
        const seen = new Set<string>()
        return [...opened.filter((p) => !p.archived), ...newest]
          .filter((p) => !seen.has(p.id) && !!seen.add(p.id))
          .map((p) => ({ id: p.id, title: p.title }))
      })
  const pages = openedFirst(await found.catch(() => []), (p) => p.id, recent).slice(0, 10)
  for (const p of pages) rememberPageTitle(p.id, p.title)
  return pages
}

/** Inserts a picked suggestion, as a plain `apply` text would, and records the item as opened. */
function pick(text: string, kind: OpenedKind, id: string): Completion['apply'] {
  return (view, completion, from, to) => {
    view.dispatch({ ...insertCompletionText(view.state, text, from, to), annotations: pickedCompletion.of(completion) })
    recordOpened(kind, id)
  }
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
    apply: pick(`#${t.key}`, 'task', t.id),
  }))
  return { from: m.from, options, filter: false }
}

async function pageSource(ctx: CompletionContext): Promise<CompletionResult | null> {
  const m = ctx.matchBefore(/\[\[[^\]\n]*/)
  if (!m) return null
  const pages = await suggestPages(m.text.slice(2))
  const options: Completion[] = pages.map((p) => ({ label: `[[${p.title}`, displayLabel: p.title, type: 'text', apply: pick(pageLink(p.title, p.id), 'page', p.id) }))
  // Swallow an existing "]]" after the cursor so we do not leave stray brackets.
  const after = ctx.state.doc.sliceString(ctx.pos, ctx.pos + 2)
  return { from: m.from, to: after === ']]' ? ctx.pos + 2 : ctx.pos, options, filter: false }
}

export const mentionSources = [taskSource, pageSource]
