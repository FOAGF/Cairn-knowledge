import { useSettings, useUpdateSettings } from '@/api/settings'
import { commands } from '@/api/bindings'
import { errorMessage, isChangedOnDisk } from '@/api/client'
import { useDraft } from '@/api/drafts'
import { followOwnWrites, useGroups, usePage, useUpdatePage } from '@/api/knowledge'
// A single page: header, origin, source info, body (view or edit) and sidebar (§5.1, §5.2).
import { Archive, ChevronRight, FileQuestion, FileText, Pencil, RotateCcw, ScanSearch, TriangleAlert } from 'lucide-react'
import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { MarkdownEditor } from '@/components/editor/MarkdownEditor'
import { EditPreviewToggle, type EditorMode, MarkdownHints, MarkdownPreview } from '@/components/editor/MarkdownField'
import { EmptyState } from '@/components/shared/PageHeader'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { SavedAs } from '@/components/shared/SavedAs'
import { CategoryDot, TagChip } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'
import { countWords, dateTime, timeAgo } from '@/lib/format'
import { originLabel } from '@/lib/labels'
import { joinTitleLine, splitTitleLine } from '@/lib/titleLine'
import { cn } from '@/lib/utils'
import { groupPath } from './groupTree'
import { PageActions } from './PageActions'
import { PageSidebar } from './PageSidebar'
import { SaveConflictDialog } from './SaveConflictDialog'
import { useCategoryList } from '@/api/categories'
import { useIsExternal } from '@/api/storage'
import { useTask } from '@/api/tasks'
import { cleanTitle } from '@/lib/fileNames'
import { useSettled } from '@/lib/useSettled'
import { askBeforeEditing, holdPageEdit, pendingPath, releasePageEdit } from '@/features/creation/pending'
import { useAppStore } from '@/store'

/**
 * A saved draft (FR-KN-ED-10): what the autosave wrote while editing. Anything malformed is ignored.
 * External mode adds `baseHash`, the file's hash when editing started (FR-KN-EXT-11), so the check
 * still holds after a restart.
 */
function parseDraft(content: string): { title: string; body: string; baseHash: string | null } | null {
  try {
    const d = JSON.parse(content) as { title?: unknown; body?: unknown; baseHash?: unknown }
    return typeof d.body === 'string'
      ? { title: typeof d.title === 'string' ? d.title : '', body: d.body, baseHash: typeof d.baseHash === 'string' ? d.baseHash : null }
      : null
  } catch {
    return null
  }
}

/** The whole body as typed (kept as the draft): the `# title` line follows the title field. */
function fullBody(e: { title: string; body: string; titleLine: boolean }, pageTitle: string) {
  return e.titleLine ? joinTitleLine(e.title.trim() || pageTitle, e.body) : e.body
}

export function PageView({ pageId }: { pageId: string }) {
  const { data: page, isPending } = usePage(pageId)
  const categories = useCategoryList()
  const groups = useGroups(page?.categoryId)
  const originTask = useTask(page?.originTaskId ?? undefined).data
  const settings = useSettings()
  const updateSettings = useUpdateSettings()
  const external = useIsExternal()
  const { mutate: updatePage, isPending: saving } = useUpdatePage()
  const draft = useDraft('page', pageId)
  const [editing, setEditing] = useState(false)
  // `body` is the text without its `# title` line, which is written back from `title` on save
  // (`titleLine`: the page had that line), so the title is edited only in the title field.
  const [edit, setEdit] = useState({ title: '', body: '', titleLine: false })
  // FR-KN-EXT-11 (external mode): the file's hash when editing started, sent with *Save*; a file
  // changed since then opens the conflict dialog. Null in internal mode.
  const [base, setBase] = useState<string | null>(null)
  const [conflict, setConflict] = useState(false)
  const [resolving, setResolving] = useState(false)
  // The restore prompt is hidden once the user chose (restore keeps the draft; discard deletes it).
  const [promptDone, setPromptDone] = useState(false)
  const [mode, setMode] = useState<EditorMode>('edit')
  const [editingSource, setEditingSource] = useState(false)
  const [sourceDraft, setSourceDraft] = useState('')
  const navigate = useNavigate()
  const t = useT()
  const k = t.knowledge
  // `change` is one function for good, so the title field can skip the re-render of each key in the
  // body (TitleField). It reads the newest values from here.
  const latest = useRef({ edit, page, base, draft })
  useLayoutEffect(() => {
    latest.current = { edit, page, base, draft }
  })
  // FR-KN-ED-10: every change is saved as a draft (the whole body), so closing the app does not lose it.
  const change = useCallback((next: typeof edit) => {
    const { page, base, draft } = latest.current
    if (!page) return
    latest.current.edit = next // a second change before the next render builds on this one
    setEdit(next)
    draft.update(JSON.stringify({ title: next.title, body: fullBody(next, page.title), tags: page.tags, baseHash: base ?? undefined }))
    holdPageEdit(page.id, next.title.trim() || page.title)
  }, [])
  const setTitle = useCallback((title: string) => change({ ...latest.current.edit, title }), [change])
  const setBody = useCallback((body: string) => change({ ...latest.current.edit, body }), [change])
  const toolbarTitle = useSettled(edit.title.trim(), SETTLE_MS)
  // UI-KN-6a (D-71): from its first unsaved change until *Save* or *Cancel*, the page is the pending
  // page, with a tab in the top bar; opened again, it goes straight back into editing with its
  // changes: those typed this session, else the database's.
  const held = useAppStore((s) => s.pageEdit?.pageId === pageId)
  const heldText = held ? (draft.recent ?? (draft.loaded ? (draft.pending?.content ?? null) : undefined)) : undefined
  const heldDraft = typeof heldText === 'string' ? parseDraft(heldText) : null
  const heldGone = heldText === null || (heldText !== undefined && !heldDraft)
  useEffect(() => {
    if (heldGone) releasePageEdit(pageId)
  }, [heldGone, pageId])

  if (isPending) return <div className="rounded-2xl border bg-card p-6 text-sm text-muted-foreground">{k.page.loading}</div>

  // Unknown page id.
  if (!page)
    return (
      <EmptyState icon={FileQuestion} title={k.page.notFound}>
        {k.sentences(
          k.page.deletedHint,
          <Link to="/knowledge" className="font-medium text-primary">
            {k.page.backToKnowledge}
          </Link>,
        )}
      </EmptyState>
    )

  const category = categories.find((p) => p.id === page.categoryId)
  const group = groups.find((g) => g.id === page.groupId)
  const groupName = group && groupPath(group, groups)
  const pending = !promptDone && !held && draft.pending ? { at: draft.pending.updatedAt, content: parseDraft(draft.pending.content) } : null

  // What *Save* writes. In external mode a changed title follows its file name (FR-KN-EXT-15), and the
  // `# title` line is written with that name too, so the two stay the same.
  const savedTitle = (e: typeof edit) => {
    const typed = e.title.trim() || page.title
    return external && typed !== page.title ? cleanTitle(typed) : typed
  }
  const savedBody = (e: typeof edit) => (e.titleLine ? joinTitleLine(savedTitle(e), e.body) : e.body)
  const begin = (title: string, body: string, baseHash: string | null) => {
    const { rest, had } = splitTitleLine(body, title)
    setEdit({ title, body: rest, titleLine: had })
    setBase(baseHash)
    setMode('edit')
    setEditing(true)
  }
  if (heldDraft && !editing) begin(heldDraft.title || page.title, heldDraft.body, heldDraft.baseHash ?? page.fileHash)
  // Another page waiting asks first: continue it, or discard it and edit this one.
  const startEdit = async () => {
    const choice = await askBeforeEditing(page.id)
    if (choice === 'continue') navigate(pendingPath() ?? '/knowledge')
    else if (choice === 'start') begin(page.title, page.body, page.fileHash)
  }
  const restore = async () => {
    if (!pending?.content) return
    const title = pending.content.title || page.title
    const choice = await askBeforeEditing(page.id)
    if (choice === 'continue') return navigate(pendingPath() ?? '/knowledge')
    if (choice !== 'start') return
    begin(title, pending.content.body, pending.content.baseHash ?? page.fileHash)
    holdPageEdit(page.id, title.trim() || page.title)
    setPromptDone(true)
  }
  const discardPending = () => {
    void draft.discard()
    setPromptDone(true)
  }
  const cancel = () => {
    void draft.discard()
    releasePageEdit(page.id)
    setEditing(false)
  }
  const finish = () => {
    void draft.discard()
    releasePageEdit(page.id)
    setEditing(false)
    setConflict(false)
  }
  // `baseHash`: the file as the editor knows it (external mode). Cairn's own writes since editing
  // started (a tag, the group) are followed; any other change is refused as `changed_on_disk`.
  const write = (baseHash: string | null) =>
    new Promise<void>((resolve) =>
      updatePage(
        { id: page.id, patch: { title: savedTitle(edit), body: savedBody(edit), ...(baseHash ? { baseHash } : {}) } },
        { onSuccess: finish, onError: (e) => isChangedOnDisk(e) && setConflict(true), onSettled: () => resolve() },
      ),
    )
  const save = () => void write(base && followOwnWrites(page.id, base))
  // *Keep mine*: save over the file as it is now (none when it is gone: the save writes it again).
  const keepMine = async () => {
    setResolving(true)
    try {
      const disk = await commands.getPageOnDisk(page.id)
      await write(disk?.fileHash ?? null)
    } catch (e) {
      toast.error(errorMessage(e))
    } finally {
      setResolving(false)
    }
  }
  // *Take the disk version*: the index reads the file, and the changes are dropped.
  const takeDisk = async () => {
    setResolving(true)
    try {
      await commands.reloadPageFromDisk(page.id)
      finish()
    } catch (e) {
      toast.error(errorMessage(e))
    } finally {
      setResolving(false)
    }
  }

  const shown = splitTitleLine(page.body, page.title).rest

  return (
    <div className="grid items-start gap-6 xl:grid-cols-[1fr_300px]">
      <article className={cn('min-w-0 space-y-4', editing && 'pb-20')}>
        {pending && (
          <div role="status" className="flex flex-wrap items-center gap-3 rounded-2xl border bg-tint-amber/50 p-4 text-sm">
            <RotateCcw className="size-4 shrink-0 text-tint-amber-fg" />
            <p className="min-w-0 flex-1">
              {k.sentences(
                <span className="font-medium">{k.page.restoreTitle}</span>,
                <span className="text-muted-foreground">{k.page.restoreHint(timeAgo(pending.at))}</span>,
              )}
            </p>
            {pending.content && (
              <Button size="sm" onClick={restore}>
                {t.common.restore}
              </Button>
            )}
            <Button size="sm" variant="outline" onClick={discardPending}>
              {t.common.discard}
            </Button>
          </div>
        )}
        <div className="rounded-2xl border bg-card p-6 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
          {/* Header */}
          <div className="mb-1 flex flex-wrap items-center gap-1 text-sm text-muted-foreground">
            <CategoryDot color={category?.color ?? 'slate'} className="mr-1 size-2" />
            <span>{category?.name}</span>
            <ChevronRight className="size-3.5" />
            <span>{groupName ?? k.ungrouped}</span>
            {page.archived && (
              <span className="ml-2 inline-flex items-center gap-1 rounded-full bg-tint-slate px-2 py-0.5 text-xs font-medium text-tint-slate-fg">
                <Archive className="size-3" /> {k.archived}
              </span>
            )}
          </div>
          <div className="flex flex-wrap items-start justify-between gap-3">
            {editing ? (
              <TitleField title={edit.title} onChange={setTitle} />
            ) : (
              <h1 className="text-2xl font-semibold tracking-tight">{page.title}</h1>
            )}
            {!editing && (
              <div className="flex flex-wrap items-center gap-2">
                <Button variant="outline" onClick={startEdit}>
                  <Pencil /> {t.common.edit}
                </Button>
                <PageActions page={page} />
              </div>
            )}
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            {page.tags.map((t) => (
              <TagChip key={t} tag={t} />
            ))}
            {page.checkStatus === 'not_checked' && (
              <>
                <span className="inline-flex items-center gap-1 rounded-full bg-tint-amber px-2.5 py-0.5 text-xs font-medium text-tint-amber-fg">
                  <TriangleAlert className="size-3" /> {k.notChecked}
                </span>
                <Button size="sm" variant="outline" asChild>
                  <Link to={`/knowledge/new?check=${page.id}`}>
                    <ScanSearch /> {k.page.runChecks}
                  </Link>
                </Button>
              </>
            )}
          </div>

          {/* Origin + source info (§5.1) */}
          <div className="mt-4 space-y-1.5 border-t pt-3 text-sm text-muted-foreground">
            <p className="flex flex-wrap items-center gap-1.5">
              <FileText className="size-3.5" />
              {page.origin === 'task' && originTask
                ? k.page.createdFromTask(
                    <Link to={`/tasks/${originTask.key}`} className="font-medium text-primary hover:underline">
                      {originTask.key}
                    </Link>,
                  )
                : originLabel(page.origin)}
              <span>{k.page.updated(dateTime(page.updatedAt))}</span>
              {/* External mode: the page's file, from the knowledge folder's root. */}
              {page.path && (
                <span className="min-w-0 truncate font-mono text-xs" title={page.path} data-testid="page-path">
                  · {page.path}
                </span>
              )}
            </p>
            <div className="flex items-center gap-1.5">
              <span className="shrink-0">{k.page.source}</span>
              {editingSource ? (
                <Input
                  autoFocus
                  value={sourceDraft}
                  className="h-7 flex-1"
                  placeholder={k.page.sourcePlaceholder}
                  onChange={(e) => setSourceDraft(e.target.value)}
                  onBlur={() => {
                    updatePage({ id: page.id, patch: { sourceInfo: sourceDraft.trim() } })
                    setEditingSource(false)
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.nativeEvent.isComposing) e.currentTarget.blur()
                    if (e.key === 'Escape') setEditingSource(false)
                  }}
                />
              ) : (
                <button
                  className="group inline-flex min-w-0 items-center gap-1.5 rounded px-1 text-left hover:bg-muted"
                  onClick={() => {
                    setSourceDraft(page.sourceInfo ?? '')
                    setEditingSource(true)
                  }}
                >
                  <span className={cn('truncate pr-1', page.sourceInfo ? 'text-foreground' : 'italic')}>{page.sourceInfo || k.page.addSource}</span>
                  <Pencil className="size-3 opacity-0 group-hover:opacity-100" />
                </button>
              )}
            </div>
          </div>
        </div>

        {/* Body */}
        <div className="rounded-2xl border bg-card p-6 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
          {editing ? (
            <>
              <div className="mb-4 flex flex-wrap items-center gap-2 border-b pb-3">
                <EditPreviewToggle mode={mode} onChange={setMode} />
                {/* D-62: the checks underline the source, so they belong to Edit only. */}
                {mode === 'edit' && (
                  <>
                    <Button size="sm" variant={settings.spellcheck ? 'secondary' : 'outline'} aria-pressed={settings.spellcheck} onClick={() => updateSettings({ spellcheck: !settings.spellcheck })}>
                      {k.page.spelling(settings.spellcheck)}
                    </Button>
                    <Button size="sm" variant={settings.grammar ? 'secondary' : 'outline'} aria-pressed={settings.grammar} onClick={() => updateSettings({ grammar: !settings.grammar })}>
                      {k.page.grammar(settings.grammar)}
                    </Button>
                  </>
                )}
                <div className="ml-auto">
                  <MarkdownHints pageImages />
                </div>
              </div>
              {/* The editor stays mounted while previewing, so undo history and the cursor survive. */}
              <div className={mode === 'preview' ? 'hidden' : undefined}>
                <MarkdownEditor
                  value={edit.body}
                  onChange={setBody}
                  spellcheck={settings.spellcheck}
                  glossaryCategoryId={page.categoryId}
                  imageCategoryId={page.categoryId}
                  minHeight="22rem"
                  autoFocus
                  placeholder={k.page.editorPlaceholder}
                />
                <p className="mt-3 text-xs text-muted-foreground">{k.page.imageTip}</p>
              </div>
              {mode === 'preview' && <MarkdownPreview text={edit.body} minHeight="22rem" glossaryCategoryId={page.categoryId} />}
            </>
          ) : (
            <MarkdownView glossaryCategoryId={page.categoryId} text={shown} />
          )}
          <WordCount key={page.id} text={editing ? edit.body : shown} threshold={settings.wordWarningThreshold} />
        </div>
      </article>
      <PageSidebar page={page} />
      {editing && (
        // Cancel and Save float at the bottom of the window while editing, so they never scroll away.
        <div className="pointer-events-none fixed inset-x-0 bottom-5 z-20 flex justify-center px-6">
          <div role="toolbar" aria-label={k.page.editing} className="pointer-events-auto flex max-w-full min-w-0 items-center gap-3 rounded-2xl border bg-card/95 py-2 pr-2 pl-4 shadow-lg backdrop-blur">
            <Pencil className="size-4 shrink-0 text-muted-foreground" />
            <span className="min-w-0 truncate text-sm">
              <span className="text-muted-foreground">{k.page.editing}</span> <span className="font-medium">{toolbarTitle || page.title}</span>
            </span>
            <Button variant="outline" onClick={cancel}>
              {t.common.cancel}
            </Button>
            <Button onClick={save} disabled={saving}>
              {saving ? k.page.saving : t.common.save}
            </Button>
          </div>
        </div>
      )}
      <SaveConflictDialog
        open={conflict}
        pageId={page.id}
        mine={{ title: savedTitle(edit), body: savedBody(edit) }}
        busy={resolving || saving}
        onKeepMine={() => void keepMine()}
        onTakeDisk={() => void takeDisk()}
        onClose={() => setConflict(false)}
      />
    </div>
  )
}

// While editing, a change on the page outside the editor's box makes WebKitGTK repaint the whole window
// (see `followHeight` in MarkdownEditor.tsx), too slow for a held key. So typing in the body leaves
// the title field alone, and the word count and the toolbar's title follow once typing pauses.
const SETTLE_MS = 400

/** The title while editing, with the file name it will be saved as. */
const TitleField = memo(function TitleField({ title, onChange }: { title: string; onChange: (title: string) => void }) {
  const t = useT()
  return (
    <div className="min-w-0 flex-1 space-y-1">
      <Input value={title} onChange={(e) => onChange(e.target.value)} className="h-11 text-xl font-semibold" aria-label={t.common.title} />
      <SavedAs title={title} />
    </div>
  )
})

/** FR-KN-ED-4: the word count, with a soft warning over the threshold that never blocks saving. */
function WordCount({ text, threshold }: { text: string; threshold: number }) {
  const words = countWords(useSettled(text, SETTLE_MS))
  const over = words > threshold
  const p = useT().knowledge.page
  return (
    <div className={cn('mt-4 flex items-center gap-2 border-t pt-3 text-sm', over ? 'text-tint-amber-fg' : 'text-muted-foreground')}>
      {over && <TriangleAlert className="size-4" />}
      <span>{p.words(words)}</span>
      {over && <span className="rounded-md bg-tint-amber px-2 py-0.5 text-xs">{p.manyTopics}</span>}
    </div>
  )
}
