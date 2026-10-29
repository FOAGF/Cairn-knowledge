// Knowledge: groups, pages, history, task ↔ page links, backlinks, images, Markdown export (spec §5).
// Rust emits DataChanged { knowledge, activity, mentions, search } after changes, which refetches every
// query under those keys, so mutations here don't invalidate anything themselves. Errors toast (client.ts).
//
// Query keys: ['knowledge', 'groups', categoryId?] · ['knowledge', 'pages', filter] · ['knowledge', 'page', id]
// · ['knowledge', 'byIds', ids] · ['knowledge', 'tags'] · ['knowledge', 'versions', pageId]
// · ['knowledge', 'taskPageLinks', { taskId, pageId }] · ['mentions', target, id]
import { convertFileSrc } from '@tauri-apps/api/core'
import { useMutation, useQuery } from '@tanstack/react-query'
import {
  type Backlink,
  type CheckStatus,
  commands,
  type Group,
  type GroupPatch,
  type KnowledgeSeed,
  type MarkdownExportMode,
  type MentionTarget,
  type NewGroup,
  type NewPage,
  type Page,
  type PageFilter,
  type PagePatch,
  type PageSummary,
  type PageVersion,
  type TaskPageLink,
} from './bindings'
import { isTauri, queryClient } from './client'

export type {
  Attachment,
  Backlink,
  CheckStatus,
  Group,
  GroupPatch,
  KnowledgeSeed,
  MarkdownExportMode,
  MentionSource,
  MentionTarget,
  NewGroup,
  NewPage,
  Page,
  PageExport,
  PageFilter,
  PageOrigin,
  PagePatch,
  PageSnapshot,
  PageSortField,
  PageSummary,
  PageVersion,
  TaskPageLink,
} from './bindings'

const EMPTY_GROUPS: Group[] = []
const EMPTY_PAGES: PageSummary[] = []
const EMPTY_VERSIONS: PageVersion[] = []
const EMPTY_LINKS: TaskPageLink[] = []
const EMPTY_BACKLINKS: Backlink[] = []
const EMPTY_TAGS: string[] = []

/** Every non-archived page of every category. */
export const ALL_PAGES: PageFilter = {}

// ---------------------------------------------------------------- groups

/** Groups of one category, or of all categories when `categoryId` is undefined. */
export function useGroups(categoryId?: string): Group[] {
  return (
    useQuery({ queryKey: ['knowledge', 'groups', categoryId ?? null], queryFn: () => commands.listGroups(categoryId ?? null) }).data ??
    EMPTY_GROUPS
  )
}

export function useCreateGroup() {
  return useMutation({ mutationFn: (input: NewGroup) => commands.createGroup(input) })
}

export function useUpdateGroup() {
  return useMutation({ mutationFn: ({ id, patch }: { id: string; patch: GroupPatch }) => commands.updateGroup(id, patch) })
}

/** Its pages become ungrouped. */
export function useDeleteGroup() {
  return useMutation({ mutationFn: (id: string) => commands.deleteGroup(id) })
}

// ---------------------------------------------------------------- pages

/** Page summaries (no body). Keep `filter` stable (a constant or useMemo). Text search: see api/search.ts. */
export function usePages(filter: PageFilter = ALL_PAGES) {
  return useQuery({ queryKey: ['knowledge', 'pages', filter], queryFn: () => commands.listPages(filter) })
}

/** Like usePages, but just the array: empty until loaded, never a new array per render. */
export function usePageList(filter: PageFilter = ALL_PAGES): PageSummary[] {
  return usePages(filter).data ?? EMPTY_PAGES
}

/** The full page, with body. */
export function usePage(id: string | undefined) {
  return useQuery({ queryKey: ['knowledge', 'page', id], queryFn: () => commands.getPage(id ?? ''), enabled: !!id })
}

/** For `[Title](page:<id>)` mention chips. Pass a sorted, de-duplicated id list for a stable key. */
export function usePagesByIds(ids: string[]) {
  return useQuery({ queryKey: ['knowledge', 'byIds', ids], queryFn: () => commands.getPagesByIds(ids), enabled: ids.length > 0 })
}

export function usePageTags(): string[] {
  return useQuery({ queryKey: ['knowledge', 'tags'], queryFn: () => commands.listPageTags() }).data ?? EMPTY_TAGS
}

export function useCreatePage() {
  return useMutation({ mutationFn: (input: NewPage) => commands.createPage(input) })
}

// FR-KN-EXT-11: the page editor sends the file's hash from when editing started, and a different
// hash at *Save* means the file changed on disk. Cairn's own writes to the page while it is being
// edited (tags, group, category, source info, archive) change that hash too, so they are noted here
// as "hash before → hash after", and the editor follows them (`followOwnWrites`). A write whose
// result has another text or title than the page had (the file had changed outside Cairn as well)
// is not noted, so that change still counts. Internal mode has no hashes, so nothing is noted.
const ownWrites = new Map<string, string>()

/** Runs `write` on page `id` and notes the hash change it made, if it changed nothing else. */
async function noted(id: string, write: () => Promise<Page>): Promise<Page> {
  const before = queryClient.getQueryData<Page>(['knowledge', 'page', id])
  const after = await write()
  if (before?.fileHash && after.fileHash && before.fileHash !== after.fileHash && before.body === after.body && before.title === after.title)
    ownWrites.set(`${id}:${before.fileHash}`, after.fileHash)
  return after
}

/** The hash a page's file has after Cairn's own writes since `hash` (itself when there were none). */
export function followOwnWrites(pageId: string, hash: string): string {
  let h = hash
  for (let i = 0; i < 1000 && ownWrites.has(`${pageId}:${h}`); i++) h = ownWrites.get(`${pageId}:${h}`)!
  return h
}

/**
 * Only the given fields change. `sourceInfo: ''` clears it; `groupId: ''` makes the page ungrouped.
 * With `baseHash` (external mode) a file changed since then fails with `changed_on_disk` (no toast).
 */
export function useUpdatePage() {
  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: PagePatch }) =>
      patch.body != null || patch.title != null ? commands.updatePage(id, patch) : noted(id, () => commands.updatePage(id, patch)),
  })
}

/** The group is cleared; refused if the target category already has a page with this title. */
export function useMovePage() {
  return useMutation({ mutationFn: ({ id, categoryId }: { id: string; categoryId: string }) => noted(id, () => commands.movePage(id, categoryId)) })
}

export function useArchivePage() {
  return useMutation({ mutationFn: ({ id, archived }: { id: string; archived: boolean }) => noted(id, () => commands.archivePage(id, archived)) })
}

/** Used by the (simulated until M5) checks. */
export function useSetPageCheckStatus() {
  return useMutation({ mutationFn: ({ id, status }: { id: string; status: CheckStatus }) => commands.setPageCheckStatus(id, status) })
}

/** Permanent: ask for confirmation first (FR-KN-ED-12). */
export function useDeletePage() {
  return useMutation({ mutationFn: (id: string) => commands.deletePage(id) })
}

// ---------------------------------------------------------------- history

/** Daily versions, oldest first. */
export function usePageVersions(pageId: string | undefined): PageVersion[] {
  return (
    useQuery({ queryKey: ['knowledge', 'versions', pageId], queryFn: () => commands.listPageVersions(pageId ?? ''), enabled: !!pageId }).data ??
    EMPTY_VERSIONS
  )
}

/** Restores a version as a new change (FR-KN-ED-9). */
export function useRestorePageVersion() {
  return useMutation({ mutationFn: ({ pageId, date }: { pageId: string; date: string }) => commands.restorePageVersion(pageId, date) })
}

// ---------------------------------------------------------------- task ↔ page links (DM-5) and backlinks

/** Links of a task, of a page, or both. */
export function useTaskPageLinks(filter: { taskId?: string; pageId?: string }): TaskPageLink[] {
  const taskId = filter.taskId ?? null
  const pageId = filter.pageId ?? null
  return (
    useQuery({
      queryKey: ['knowledge', 'taskPageLinks', { taskId, pageId }],
      queryFn: () => commands.listTaskPageLinks(taskId, pageId),
      enabled: !!(taskId || pageId),
    }).data ?? EMPTY_LINKS
  )
}

export function useLinkTaskPage() {
  return useMutation({ mutationFn: ({ taskId, pageId }: { taskId: string; pageId: string }) => commands.linkTaskPage(taskId, pageId) })
}

export function useUnlinkTaskPage() {
  return useMutation({ mutationFn: (id: string) => commands.unlinkTaskPage(id) })
}

/** "Mentioned in" for a task or page (DM-MN-4), including task comments. */
export function useBacklinks(target: MentionTarget, id: string | undefined): Backlink[] {
  return (
    useQuery({ queryKey: ['mentions', target, id], queryFn: () => commands.listBacklinks(target, id ?? ''), enabled: !!id }).data ??
    EMPTY_BACKLINKS
  )
}

// ---------------------------------------------------------------- images (FR-KN-ED-3)

let attachmentsDir: Promise<string> | undefined

/** URL for `attachment:<name>` (Tauri asset protocol). */
export async function attachmentUrl(name: string): Promise<string> {
  if (!isTauri()) return ''
  attachmentsDir ??= commands.attachmentsDir()
  return convertFileSrc(`${await attachmentsDir}/${name}`)
}

/**
 * Stores an image file (paste or drop) and returns its Markdown, e.g. `![](attachment:ab12….png)`.
 * With `pageCategoryId` the image belongs to a page of that category: in external mode it goes into
 * the category folder's `attachments` (`![](folder:Ops/attachments/….png)`, FR-KN-EXT-3); internal
 * mode stores it as before. Tasks and comments leave it out.
 */
export async function saveImage(file: Blob, alt = '', pageCategoryId?: string): Promise<string> {
  const buffer = new Uint8Array(await file.arrayBuffer())
  let binary = ''
  for (let i = 0; i < buffer.length; i += 0x8000) binary += String.fromCharCode(...buffer.subarray(i, i + 0x8000))
  if (pageCategoryId) {
    const saved = await commands.savePageImage(pageCategoryId, btoa(binary), file.type || 'image/png')
    return `![${alt}](${saved.target})`
  }
  const attachment = await commands.saveAttachment(btoa(binary), file.type || 'image/png')
  return `![${alt}](attachment:${attachment.name})`
}

// ---------------------------------------------------------------- export (TC-8a)

/** `path` is the `.md` file (folder mode) or the `.zip` file. The UI asks for the mode each time. */
export function useExportPageMarkdown() {
  return useMutation({
    mutationFn: ({ pageId, path, mode }: { pageId: string; path: string; mode: MarkdownExportMode }) =>
      commands.exportPageMarkdown(pageId, path, mode),
  })
}

// ---------------------------------------------------------------- dev seed

/** Development only: inserts the prototype's sample groups, pages, history and task links where missing. */
export async function seedDevKnowledge(seed: KnowledgeSeed) {
  await commands.devSeedKnowledge(seed)
}
