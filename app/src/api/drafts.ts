// Unsaved editor content (FR-KN-ED-10): saved to the database every few seconds while editing,
// and once more when the app is closed normally, so it can be restored the next time.
//
// Usage in an editor component:
//   const draft = useDraft('page', page.id)            // or 'new_page', 'task_description', 'comment', 'new_comment'
//   draft.pending  → a saved draft from earlier (show "Restore unsaved changes from <time>?"), or null
//   draft.recent   → the newest content typed this session, even before it reaches the database
//   draft.update(JSON.stringify({ title, body, tags }))   // call on every change while editing
//   draft.discard()                                       // after Save or Cancel, or when declining a restore
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useQuery } from '@tanstack/react-query'
import { useCallback, useEffect, useRef } from 'react'
import { commands, type Draft } from './bindings'
import { isTauri, queryClient } from './client'

export type DraftKind = 'page' | 'new_page' | 'task_description' | 'comment' | 'new_comment'
export type { Draft } from './bindings'

/** Autosave interval while editing. */
const SAVE_EVERY_MS = 3000
/** Longest the window waits for drafts to be saved when it is closed. */
const CLOSE_WAIT_MS = 2000

/** Pending writes of every mounted editor, flushed when the window closes. */
const flushers = new Set<() => Promise<void>>()

/**
 * The newest content of each draft typed this session (`kind/refId`), written or not: an editor opened
 * again right after leaving (the pending page's tab, UI-KN-6a) gets it before the database has it.
 */
const recent = new Map<string, string>()

/** Forgets a draft: the database row, this session's copy and the cached query. */
export async function discardDraft(kind: DraftKind, refId: string) {
  recent.delete(`${kind}/${refId}`)
  await commands.deleteDraft(kind, refId)
  queryClient.setQueryData<Draft | null>(['drafts', kind, refId], null)
}

/**
 * Starts a draft from outside its editor, e.g. a page change proposed in the Assistant opened for
 * editing (FR-LLM-AS-3): the editor opened next finds it as its unsaved changes.
 */
export async function writeDraft(kind: DraftKind, refId: string, content: string) {
  recent.set(`${kind}/${refId}`, content)
  await commands.saveDraft(kind, refId, content)
  void queryClient.invalidateQueries({ queryKey: ['drafts', kind, refId] })
}

/** Adds a save that must run when the window closes (outside an editor component). Returns its removal. */
export function registerFlusher(flush: () => Promise<void>): () => void {
  flushers.add(flush)
  return () => flushers.delete(flush)
}

/** Saves every editor's unsaved draft now. Called on window close; safe to call any time. */
export async function flushAllDrafts() {
  await Promise.allSettled([...flushers].map((flush) => flush()))
}

let closeHookInstalled = false

/** Installs the "save drafts, then close" hook once. Call from the app shell. */
export function installDraftCloseHook() {
  if (closeHookInstalled || !isTauri()) return
  closeHookInstalled = true
  // Registering this hands closing to the UI: Tauri waits for the handler, then the API calls
  // `destroy()` (needs the `core:window:allow-destroy` permission in src-tauri/capabilities).
  void getCurrentWindow().onCloseRequested(async () => {
    // FR-KN-ED-10: a normal exit keeps unsaved edits as a temporary version. Saving may take at
    // most CLOSE_WAIT_MS, so a slow or failing save never keeps the window open.
    await Promise.race([flushAllDrafts(), new Promise((resolve) => setTimeout(resolve, CLOSE_WAIT_MS))])
  })
}

/**
 * Draft state for one editor. `kind` + `refId` identify the item (for new items, choose a stable id,
 * e.g. the task id for `new_comment`). Pass `enabled = false` while not editing.
 */
export function useDraft(kind: DraftKind, refId: string | undefined, enabled = true) {
  const key = ['drafts', kind, refId] as const
  const query = useQuery({
    queryKey: key,
    queryFn: () => commands.getDraft(kind, refId ?? ''),
    enabled: enabled && !!refId && isTauri(),
    staleTime: 0,
  })
  const pending = query.data ?? null

  const latest = useRef<string | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  const flush = useCallback(async () => {
    clearTimeout(timer.current)
    timer.current = undefined
    const content = latest.current
    if (content === null || !refId) return
    latest.current = null
    await commands.saveDraft(kind, refId, content)
  }, [kind, refId])

  useEffect(() => {
    flushers.add(flush)
    return () => {
      flushers.delete(flush)
      void flush() // leaving the editor without Save/Cancel keeps the draft
    }
  }, [flush])

  /** Remember the current content; it is written within a few seconds. */
  const update = useCallback(
    (content: string) => {
      latest.current = content
      if (refId) recent.set(`${kind}/${refId}`, content)
      if (!timer.current) timer.current = setTimeout(() => void flush(), SAVE_EVERY_MS)
    },
    [flush, kind, refId],
  )

  /** Forget the draft (after Save or Cancel, or when the user declines to restore it). */
  const discard = useCallback(async () => {
    clearTimeout(timer.current)
    timer.current = undefined
    latest.current = null
    if (!refId) return
    await discardDraft(kind, refId)
  }, [kind, refId])

  return {
    pending: pending as Draft | null,
    /** `pending` was read from the database since this editor opened. */
    loaded: query.isFetchedAfterMount,
    recent: refId ? (recent.get(`${kind}/${refId}`) ?? null) : null,
    update,
    flush,
    discard,
  }
}
