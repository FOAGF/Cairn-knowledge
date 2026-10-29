// The one pending page: a new page in review (from a file, a task, the chat or written by hand),
// content on its way into a merge, or a saved page with unsaved changes (D-71). Only one waits at a
// time; starting another asks first. It is kept in the data folder (drafts table, `new_page` /
// `pending`; a saved page's changes are its own `page` draft) every few seconds and when the window
// closes, so it comes back after a restart, and the top bar shows it as a tab until it is saved or
// discarded on its own screen (the tab and the question: PendingPage.tsx).
import { useLocation, type NavigateFunction } from 'react-router'
import { commands } from '@/api/bindings'
import { isApiError, isTauri } from '@/api/client'
import { discardDraft, registerFlusher } from '@/api/drafts'
import { useAppStore } from '@/store'
import { type PageDraft, REVIEW_PATH } from '@/store/creation'

type AppState = ReturnType<typeof useAppStore.getState>

const KIND = 'new_page'
const REF = 'pending'
/** Written before this version: a hand-written page only (`new_page` / `manual`). */
const LEGACY_REF = 'manual'
const SAVE_EVERY_MS = 3000

interface Saved {
  draft: PageDraft
  queue: PageDraft[]
  path: string
}

export type PageEdit = NonNullable<AppState['pageEdit']>

/** A saved page's unsaved changes: which page, and the title its tab shows. */
interface SavedEdit {
  edit: PageEdit
}

let installed = false
let timer: ReturnType<typeof setTimeout> | undefined
let dirty = false

async function write() {
  clearTimeout(timer)
  timer = undefined
  if (!dirty) return
  dirty = false
  const { draft, draftQueue, draftPath, pageEdit } = useAppStore.getState()
  if (draft) await commands.saveDraft(KIND, REF, JSON.stringify({ draft, queue: draftQueue, path: draftPath } satisfies Saved))
  else if (pageEdit) await commands.saveDraft(KIND, REF, JSON.stringify({ edit: pageEdit } satisfies SavedEdit))
  else await commands.deleteDraft(KIND, REF)
}

function parse(content: string): Saved | null {
  try {
    const s = JSON.parse(content) as Partial<Saved>
    if (!s.draft || typeof s.draft.body !== 'string' || !s.draft.categoryId) return null
    return { draft: s.draft, queue: Array.isArray(s.queue) ? s.queue : [], path: typeof s.path === 'string' ? s.path : REVIEW_PATH }
  } catch {
    return null
  }
}

function parseEdit(content: string): PageEdit | null {
  try {
    const e = (JSON.parse(content) as Partial<SavedEdit>).edit
    return e && typeof e.pageId === 'string' ? { pageId: e.pageId, title: typeof e.title === 'string' ? e.title : '' } : null
  } catch {
    return null
  }
}

/** The title in a `page` draft (`{ title, body, tags }`), or ''. */
function draftTitle(content: string): string {
  try {
    const t = (JSON.parse(content) as { title?: unknown }).title
    return typeof t === 'string' ? t.trim() : ''
  } catch {
    return ''
  }
}

/** The page still exists and still has its changes. */
async function stillEdited(pageId: string): Promise<boolean> {
  try {
    await commands.getPage(pageId)
  } catch {
    return false
  }
  return !!(await commands.getDraft('page', pageId))
}

/**
 * Nothing is pending: the saved page changed most recently without *Save* or *Cancel* becomes the
 * pending page (unsaved changes from before D-71 included; any others keep their restore prompt).
 * Changes left for a page that no longer exists are removed.
 */
async function adoptPageEdit() {
  for (const d of await commands.listDrafts('page')) {
    let title: string
    try {
      title = (await commands.getPage(d.refId)).title
    } catch (e) {
      if (isApiError(e) && e.kind === 'not_found') await commands.deleteDraft('page', d.refId)
      continue
    }
    useAppStore.setState({ pageEdit: { pageId: d.refId, title: draftTitle(d.content) || title } })
    return
  }
}

/** A hand-written page autosaved by the earlier review screen. */
function parseLegacy(content: string): PageDraft | null {
  try {
    const d = JSON.parse(content) as { title?: string; body?: string; tags?: string[]; categoryId?: string; groupId?: string | null; sourceInfo?: string | null }
    if (!d.categoryId || !(d.title || d.body)) return null
    return { source: 'manual', categoryId: d.categoryId, title: d.title ?? '', body: d.body ?? '', tags: d.tags ?? [], groupId: d.groupId ?? undefined, sourceInfo: d.sourceInfo ?? undefined }
  } catch {
    return null
  }
}

/** Loads the pending page saved earlier and keeps it saved from then on. Call once from the app shell. */
export async function installPendingPage() {
  if (installed || !isTauri()) return
  installed = true
  // The storage the store used to keep it in is shared by every build: only a development build takes
  // a page from there, once.
  const fromStorage = import.meta.env.DEV ? useAppStore.getState().draft : null
  useAppStore.setState({ draft: null, draftQueue: [], pageEdit: null })
  try {
    const saved = await commands.getDraft(KIND, REF)
    const parsed = saved && parse(saved.content)
    const edit = saved && !parsed ? parseEdit(saved.content) : null
    if (parsed) useAppStore.setState({ draft: parsed.draft, draftQueue: parsed.queue, draftPath: parsed.path })
    else if (edit && (await stillEdited(edit.pageId))) useAppStore.setState({ pageEdit: edit })
    else {
      const legacy = await commands.getDraft(KIND, LEGACY_REF)
      const old = (legacy && parseLegacy(legacy.content)) ?? fromStorage
      if (old) useAppStore.setState({ draft: old, draftQueue: [], draftPath: REVIEW_PATH })
      else await adoptPageEdit()
      if (old || saved || useAppStore.getState().pageEdit) {
        dirty = true
        await write()
      }
      if (legacy) await commands.deleteDraft(KIND, LEGACY_REF)
    }
  } catch (e) {
    console.warn('could not load the pending page', e)
  }
  useAppStore.subscribe((s, prev) => {
    if (s.draft === prev.draft && s.draftQueue === prev.draftQueue && s.draftPath === prev.draftPath && s.pageEdit === prev.pageEdit) return
    dirty = true
    if (!timer) timer = setTimeout(() => void write().catch((e) => console.warn('could not save the pending page', e)), SAVE_EVERY_MS)
  })
  registerFlusher(write)
}

/** The same page started again (its chat card, or the page being merged): continue it instead of asking. */
function samePage(a: PageDraft, b: PageDraft): boolean {
  if (a.originConversation && a.originConversation.messageId === b.originConversation?.messageId) return true
  return !!a.fromPageId && a.fromPageId === b.fromPageId
}

type Choice = 'start' | 'continue' | 'cancel'

/** Where the pending page is edited (the review or merge screen, or the saved page), or null when there is none. */
export function pendingPath(s: Pick<AppState, 'draft' | 'draftPath' | 'pageEdit'> = useAppStore.getState()): string | null {
  return s.draft ? s.draftPath : s.pageEdit ? `/knowledge/${s.pageEdit.pageId}` : null
}

/** True while the window shows the pending page: its tab is highlighted, not Knowledge as well. */
export function useOnPendingPage(): boolean {
  const path = useAppStore(pendingPath)
  const savedPage = useAppStore((s) => !s.draft && !!s.pageEdit)
  const location = useLocation()
  if (!path) return false
  // A saved page keeps the tree's category filter in the query string.
  return savedPage ? location.pathname === path : location.pathname + location.search === path
}

/** The question; `start` once the user chose to discard the pending page. */
function ask(next: 'new' | 'edit'): Promise<Choice> {
  const { draft, pageEdit } = useAppStore.getState()
  return new Promise((resolve) => {
    useAppStore.setState({
      pendingAsk: {
        title: (draft ? draft.title : (pageEdit?.title ?? '')).trim(),
        edit: !draft,
        next,
        resolve: (choice) => {
          useAppStore.setState({ pendingAsk: null })
          resolve(choice === 'replace' ? 'start' : choice === 'continue' ? 'continue' : 'cancel')
        },
      },
    })
  })
}

/** Drops a saved page's unsaved changes (its `page` draft). */
export function dropPageEdit(edit: PageEdit) {
  void discardDraft('page', edit.pageId).catch((e) => console.warn('could not discard the unsaved changes', e))
}

/**
 * Before a new page replaces the pending one: `start` when nothing else is pending or the user chose to
 * discard it; `continue` when the user kept it (or `next` is the same page), then open `pendingPath()`;
 * `cancel` when the question was closed.
 */
export function askBeforeReplacing(next?: PageDraft): Promise<Choice> {
  const { draft, pageEdit } = useAppStore.getState()
  if (!draft && !pageEdit) return Promise.resolve('start')
  if (draft && next && samePage(draft, next)) return Promise.resolve('continue')
  return ask('new')
}

/**
 * Before editing a saved page (or restoring its changes): the same question when another page is
 * pending. On `start` the other one is already discarded; on `continue` open `pendingPath()`.
 */
export async function askBeforeEditing(pageId: string): Promise<Choice> {
  const { draft, pageEdit } = useAppStore.getState()
  if (!draft && (!pageEdit || pageEdit.pageId === pageId)) return 'start'
  const choice = await ask('edit')
  if (choice === 'start') {
    if (pageEdit) dropPageEdit(pageEdit)
    useAppStore.setState({ draft: null, draftQueue: [], pageEdit: null })
  }
  return choice
}

/** Makes `next` the pending page, edited at `path`. Call after `askBeforeReplacing` said `start`. */
export function startPendingPage(next: PageDraft, path = REVIEW_PATH) {
  const { pageEdit } = useAppStore.getState()
  if (pageEdit) dropPageEdit(pageEdit)
  useAppStore.setState({ draft: next, draftQueue: [], draftPath: path, pageEdit: null })
}

let retitle: ReturnType<typeof setTimeout> | undefined

/**
 * A saved page's first unsaved change makes it the pending page; later changes keep its tab's title
 * current, once typing in the title pauses: a change in the top bar makes WebKitGTK repaint the whole
 * window (see `followHeight` in MarkdownEditor.tsx), too slow for a held key.
 */
export function holdPageEdit(pageId: string, title: string) {
  const cur = useAppStore.getState().pageEdit
  clearTimeout(retitle)
  if (cur?.pageId === pageId && cur.title === title) return
  if (cur?.pageId !== pageId) return useAppStore.setState({ pageEdit: { pageId, title } })
  retitle = setTimeout(() => {
    if (useAppStore.getState().pageEdit?.pageId === pageId) useAppStore.setState({ pageEdit: { pageId, title } })
  }, 400)
}

/** After *Save* or *Cancel* (or when its changes are gone): the saved page is no longer pending. */
export function releasePageEdit(pageId: string) {
  if (useAppStore.getState().pageEdit?.pageId === pageId) useAppStore.setState({ pageEdit: null })
}

/** `askBeforeReplacing`, then start `next` and open it, or open the pending page the user kept. True when `next` was started. */
export async function openAsPending(next: PageDraft, path: string, navigate: NavigateFunction): Promise<boolean> {
  const choice = await askBeforeReplacing(next)
  if (choice === 'cancel') return false
  if (choice === 'continue') {
    navigate(pendingPath() ?? path)
    return false
  }
  startPendingPage(next, path)
  navigate(path)
  return true
}
