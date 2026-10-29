// "Check all unchecked" (FR-KN-SV-4, UI-KN-7): the checks of many pages, several at a time.
// Per page, similar pages come first (the local search model, quick). A likely duplicate holds the
// model's check back until the user asks for it; otherwise the model checks topics, group, tags and
// glossary terms. A clean page (one topic, no likely duplicate) is marked checked by itself.
// The results are kept for the session, per page version (`updatedAt`), so coming back to the screen
// doesn't call the model again.
import { create } from 'zustand'
import { commands, type CheckReport, type DuplicateReport, type Page } from '@/api/bindings'
import { checkPage, findDuplicates, pageMarkdown } from '@/api/checks'
import { errorMessage } from '@/api/client'
import { isLlmUnavailable } from '@/api/llm'
import { readSettings } from '@/api/settings'
import { countWords } from '@/lib/format'

export interface CheckRow {
  pageId: string
  /** The page version the results are for. */
  updatedAt: string
  state: 'running' | 'held' | 'done' | 'error'
  /** What runs now: similar pages, then the model. */
  step?: 'similar' | 'model'
  duplicates?: DuplicateReport
  report?: CheckReport
  /** The model's check did not run because no model is set up (FR-KN-SV-3). */
  noModel?: string
  /** The topic result doesn't count: a short manual page (FR-KN-SV-1). */
  topicNotNeeded?: boolean
  error?: string
  /** Marked as checked here: by itself (clean) or by the user. */
  checked?: 'auto' | 'user'
}

interface CheckRows {
  rows: Record<string, CheckRow>
}

export const useCheckRows = create<CheckRows>()(() => ({ rows: {} }))

function put(row: CheckRow) {
  useCheckRows.setState((s) => ({ rows: { ...s.rows, [row.pageId]: row } }))
}

export function patchRow(pageId: string, c: Partial<CheckRow>) {
  useCheckRows.setState((s) => (s.rows[pageId] ? { rows: { ...s.rows, [pageId]: { ...s.rows[pageId], ...c } } } : s))
}

/** Forgets results made for an older version of a page (it changed since). */
export function dropStale(pages: { id: string; updatedAt: string }[]) {
  const rows = { ...useCheckRows.getState().rows }
  let changed = false
  for (const p of pages) {
    const r = rows[p.id]
    if (r && r.state !== 'running' && r.updatedAt !== p.updatedAt) {
      delete rows[p.id]
      changed = true
    }
  }
  if (changed) useCheckRows.setState({ rows })
}

export const likelyDuplicate = (r: CheckRow | undefined) => !!r?.duplicates?.hits.some((h) => h.likely)
export const severalTopics = (r: CheckRow | undefined) => !!r?.report && !r.report.oneTopic && !r.topicNotNeeded
/** Every check ran, one topic and no likely duplicate. A group suggestion or glossary terms don't block it. */
export const isClean = (r: CheckRow | undefined) => r?.state === 'done' && !!r.report && !severalTopics(r) && !likelyDuplicate(r)

/** Marks a page as checked and remembers the new version, so the change doesn't count as an edit. */
export async function markChecked(pageId: string, by: 'auto' | 'user') {
  const p = await commands.setPageCheckStatus(pageId, 'checked')
  patchRow(pageId, { checked: by, updatedAt: p.updatedAt })
}

/** One page's checks. `force` runs the model's check even when a likely duplicate held it back. */
export async function runChecks(pageId: string, opts?: { force?: boolean }) {
  const before = useCheckRows.getState().rows[pageId]
  let page: Page
  try {
    page = await commands.getPage(pageId)
  } catch (e) {
    return put({ pageId, updatedAt: before?.updatedAt ?? '', state: 'error', error: errorMessage(e) })
  }
  const input = { categoryId: page.categoryId, title: page.title, body: pageMarkdown(page.title, page.body) }
  const keep = opts?.force ? before?.duplicates : undefined
  put({ pageId, updatedAt: page.updatedAt, state: 'running', step: keep ? 'model' : 'similar', duplicates: keep })
  try {
    const duplicates = keep ?? (await findDuplicates({ ...input, excludePageId: page.id }))
    if (!opts?.force && duplicates.hits.some((h) => h.likely)) return patchRow(pageId, { state: 'held', step: undefined, duplicates })
    patchRow(pageId, { step: 'model', duplicates })
    let report: CheckReport | undefined
    let noModel: string | undefined
    try {
      report = await checkPage(input)
    } catch (e) {
      if (!isLlmUnavailable(e)) throw e
      noModel = errorMessage(e)
    }
    // A page found in the knowledge folder (D-72) counts as written by hand.
    const topicNotNeeded = (page.origin === 'manual' || page.origin === 'folder') && countWords(page.body) <= readSettings().wordWarningThreshold
    patchRow(pageId, { state: 'done', step: undefined, report, noModel, topicNotNeeded })
    if (isClean(useCheckRows.getState().rows[pageId])) await markChecked(pageId, 'auto')
  } catch (e) {
    patchRow(pageId, { state: 'error', step: undefined, error: errorMessage(e) })
  }
}
