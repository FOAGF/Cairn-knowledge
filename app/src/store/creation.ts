// OWNER: Knowledge creation flows (upload → draft → checks → merge).
// Holds the in-progress draft so the flow can span several screens.
import type { SetStateAction } from 'react'
import { toast } from 'sonner'
import { type CheckStatus, commands, type Page } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { fetchMessage, updateMessage } from '@/api/conversations'
import { ensureGroupPath, isGroupPath } from '@/features/knowledge/groupTree'
import { readSettings } from '@/api/settings'
import { cleanTitle } from '@/lib/fileNames'
import { joinTitleLine, splitTitleLine } from '@/lib/titleLine'
import type { SliceCreator } from './types'

export interface PageDraft {
  source: 'manual' | 'file' | 'task'
  categoryId: string
  groupId?: string
  title: string
  body: string
  tags: string[]
  sourceInfo?: string
  originTaskId?: string
  fileName?: string
  instruction?: string
  /** Set when the draft is an existing page being merged elsewhere (FR-KN-SV-2). */
  fromPageId?: string
  /** Assistant chat card this draft came from; marked "saved" after saving. */
  originConversation?: { conversationId: string; messageId: string }
  /** Glossary terms already shown for this draft (the chat card's), so the review step doesn't repeat them (FR-KN-GL-7). */
  shownTerms?: string[]
  /** The model stopped at its token limit: the draft may be cut off. */
  incomplete?: boolean
  /** A group that does not exist yet, created when the page is saved; shown as "Name (new)". */
  newGroup?: string
  /** Tags suggested before the review (the chat card's): offered to add, never added by themselves. */
  suggestedTags?: string[]
}

/** The screen of the pending page: the review, or the merge screen it went on to. */
export const REVIEW_PATH = '/knowledge/new?draft=1'

export interface CreationSlice {
  draft: PageDraft | null
  setDraft: (d: SetStateAction<PageDraft | null>) => void
  /** Extra drafts waiting after a split (FR-KN-DV-2). */
  draftQueue: PageDraft[]
  setDraftQueue: (q: SetStateAction<PageDraft[]>) => void
  /** Where the pending page is being edited (`REVIEW_PATH` or a merge screen); the top bar's tab opens it. */
  draftPath: string
  setDraftPath: (path: string) => void
  /**
   * The pending page's other kind (UI-KN-6a): a saved page with unsaved changes, held from its first
   * change until *Save* or *Cancel*. The changes themselves are its `page` draft (FR-KN-ED-10). Only one
   * of `draft` and `pageEdit` is set at a time (`features/creation/pending.tsx`).
   */
  pageEdit: { pageId: string; title: string } | null
  /** The question shown before the pending page is replaced (`features/creation/pending.tsx`). */
  pendingAsk: {
    title: string
    /** The pending page is a saved page's unsaved changes, not a new page. */
    edit: boolean
    /** What the user is starting: a new page, or editing a saved page. */
    next: 'new' | 'edit'
    resolve: (choice: 'continue' | 'replace' | null) => void
  } | null
  /** Saves the current draft as a new page (Rust) and clears it. Resolves to null when nothing was saved (an error toast is shown). */
  saveDraftAsPage: (opts?: { checkStatus?: CheckStatus }) => Promise<Page | null>
}

export const createCreationSlice: SliceCreator<CreationSlice> = (set, get) => ({
  draft: null,
  setDraft: (d) => set((s) => ({ draft: typeof d === 'function' ? d(s.draft) : d })),
  draftQueue: [],
  setDraftQueue: (q) => set((s) => ({ draftQueue: typeof q === 'function' ? q(s.draftQueue) : q })),
  draftPath: REVIEW_PATH,
  setDraftPath: (draftPath) => set({ draftPath }),
  pageEdit: null,
  pendingAsk: null,
  saveDraftAsPage: async (opts) => {
    const d = get().draft
    if (!d) return null
    let page: Page
    try {
      const name = d.newGroup?.trim()
      // External mode: "Parent / Child" makes the missing groups inside each other (D-72).
      const groupId = name ? (isGroupPath(name) ? await ensureGroupPath(d.categoryId, name) : (await commands.createGroup({ categoryId: d.categoryId, name })).id) : d.groupId
      // External mode: the title follows its file name (FR-KN-EXT-15), and so does the `# title` line.
      const external = readSettings().knowledgeMode === 'external'
      const title = external ? cleanTitle(d.title) : d.title.trim()
      const titled = splitTitleLine(d.body, d.title)
      page = await commands.createPage({
        categoryId: d.categoryId,
        groupId: groupId ?? null,
        title,
        body: external && titled.had ? joinTitleLine(title, titled.rest) : d.body,
        tags: d.tags,
        sourceInfo: d.sourceInfo ?? null,
        origin: d.source,
        originTaskId: d.originTaskId ?? null,
        // The save pipeline decides the status (FR-KN-SV-3).
        checkStatus: opts?.checkStatus ?? null,
      })
    } catch (e) {
      toast.error(errorMessage(e))
      return null
    }
    // A failed link must not lose the saved page; the user can link it from the page later.
    if (d.originTaskId) await commands.linkTaskPage(d.originTaskId, page.id).catch(() => undefined)
    if (d.originConversation) {
      const { conversationId, messageId } = d.originConversation
      // The card lives in Rust now; marking it saved also releases the file's text (FR-KN-UP-6).
      // The page is saved, so a failure here is only logged.
      try {
        const msg = await fetchMessage(conversationId, messageId)
        if (msg?.draft) await updateMessage(messageId, { draft: { ...msg.draft, state: 'saved', savedPageId: page.id } })
      } catch (e) {
        console.warn('could not mark the chat card as saved', e)
      }
    }
    set({ draft: null })
    return page
  },
})
