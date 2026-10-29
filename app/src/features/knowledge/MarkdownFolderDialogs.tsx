// Markdown folders (FR-KN-MD-1..4, D-68). Both dialogs first ask Rust for a plan (nothing written),
// list what will happen, and need an explicit tick before anything is replaced. Rust checks the
// plan again when it runs and refuses if the replacements are not the ones confirmed.
import { AlertTriangle, ChevronRight, FolderOpen, Search } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'
import { commands, type ExportPlan, type FolderExportInput, type FolderImportInput, type ImportPlan } from '@/api/bindings'
import { useCategoryList } from '@/api/categories'
import { errorMessage } from '@/api/client'
import { type PageFilter, useGroups, usePageList } from '@/api/knowledge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { pickFolder } from '@/lib/pickFolder'
import { cn } from '@/lib/utils'

const NO_GROUP = '__none__'
/** Every page, archived ones too (FR-KN-MD-3). */
const EVERY_PAGE: PageFilter = { includeArchived: true }
const NEW_GROUP = '__new__'

/** A plan from Rust for `input`, again whenever it changes; `null` while there is none. */
function usePlan<I, P>(input: I | null, run: (input: I) => Promise<P>, delay = 250) {
  const [plan, setPlan] = useState<{ for: string; plan: P } | null>(null)
  const [error, setError] = useState('')
  const [nonce, setNonce] = useState(0)
  const key = input === null ? '' : JSON.stringify(input)
  useEffect(() => {
    if (input === null) return
    let live = true
    const t = setTimeout(() => {
      run(input).then(
        (p) => live && (setPlan({ for: key, plan: p }), setError('')),
        (e) => live && (setPlan(null), setError(errorMessage(e))),
      )
    }, delay)
    return () => {
      live = false
      clearTimeout(t)
    }
    // `key` stands for `input`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, nonce])
  return { plan: plan?.for === key ? plan.plan : null, error: input === null ? '' : error, refresh: () => setNonce((n) => n + 1) }
}

function FolderField({ folder, onPick }: { folder: string | null; onPick: () => void }) {
  const f = useT().knowledge.folders
  return (
    <div className="flex items-center gap-2">
      <div className="min-w-0 flex-1 truncate rounded-lg border bg-muted/40 px-2.5 py-1.5 font-mono text-xs" title={folder ?? undefined}>
        {folder ?? f.noFolder}
      </div>
      <Button type="button" variant="outline" size="sm" onClick={onPick}>
        <FolderOpen /> {folder ? f.change : f.choose}
      </Button>
    </div>
  )
}

/** The warning box with the list of what is replaced, and the tick that allows it. */
function ReplaceConfirm({ title, explain, items, checked, onChecked }: { title: string; explain: string; items: string[]; checked: boolean; onChecked: (v: boolean) => void }) {
  const f = useT().knowledge.folders
  return (
    <div className="space-y-2 rounded-xl border border-amber-300 bg-tint-amber/50 p-3 text-sm dark:border-amber-700">
      <p className="flex items-center gap-2 font-medium text-tint-amber-fg">
        <AlertTriangle className="size-4 shrink-0" /> {title}
      </p>
      <p className="text-muted-foreground">{explain}</p>
      <ul className="max-h-36 space-y-0.5 overflow-y-auto rounded-lg bg-card/70 px-2.5 py-1.5 text-xs">
        {items.map((i) => (
          <li key={i} className="truncate">{i}</li>
        ))}
      </ul>
      <label className="flex items-start gap-2 pt-1">
        <Checkbox checked={checked} onCheckedChange={(v) => onChecked(v === true)} className="mt-0.5" />
        <span>{f.confirmReplace(items.length)}</span>
      </label>
    </div>
  )
}

function Fold({ title, children }: { title: string; children: React.ReactNode }) {
  const [openState, setOpen] = useState(false)
  return (
    <div className="text-sm">
      <button type="button" onClick={() => setOpen(!openState)} className="flex items-center gap-1 text-muted-foreground hover:text-foreground">
        <ChevronRight className={cn('size-3.5 transition-transform', openState && 'rotate-90')} /> {title}
      </button>
      {openState && <div className="mt-1 max-h-40 overflow-y-auto rounded-lg border px-2.5 py-1.5 text-xs">{children}</div>}
    </div>
  )
}

// ---------------------------------------------------------------- import

/** FR-KN-MD-1/2: every `.md` of a folder (subfolders too) into one category and group. */
export function ImportFolderDialog({ open: isOpen, onClose, defaultCategoryId }: { open: boolean; onClose: () => void; defaultCategoryId?: string }) {
  const categories = useCategoryList().filter((c) => c.status === 'active')
  const [folder, setFolder] = useState<string | null>(null)
  const [categoryId, setCategoryId] = useState('')
  const [groupChoice, setGroupChoice] = useState(NO_GROUP)
  const [newGroup, setNewGroup] = useState('')
  // The tick is for one list of replacements; a different list needs a new tick.
  const [understoodFor, setUnderstoodFor] = useState<string | null>(null)
  const [running, setRunning] = useState(false)
  const category = categoryId || defaultCategoryId || categories.find((c) => c.isGeneral)?.id || ''
  const groups = useGroups(category || undefined)
  const t = useT()
  const im = t.knowledge.importFolder

  // Choosing the folder is the first thing to do.
  useEffect(() => {
    if (!isOpen) return
    setFolder(null)
    setUnderstoodFor(null)
    void pickFolder(im.title).then((f) => (f ? setFolder(f) : onClose()))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen])

  const input: FolderImportInput | null = useMemo(
    () =>
      folder && category && (groupChoice !== NEW_GROUP || newGroup.trim())
        ? {
            folder,
            categoryId: category,
            groupId: groupChoice === NO_GROUP || groupChoice === NEW_GROUP ? null : groupChoice,
            newGroup: groupChoice === NEW_GROUP ? newGroup.trim() : null,
          }
        : null,
    [folder, category, groupChoice, newGroup],
  )
  const { plan, error, refresh } = usePlan<FolderImportInput, ImportPlan>(isOpen ? input : null, (i) => commands.planMarkdownImport(i))
  const replaced = plan?.files.filter((f) => f.action === 'replace') ?? []
  const created = plan?.files.filter((f) => f.action === 'create') ?? []
  const skipped = plan?.files.filter((f) => f.action === 'skip') ?? []
  const images = plan?.files.reduce((n, f) => n + f.images, 0) ?? 0
  const missing = plan?.files.reduce((n, f) => n + f.missingImages, 0) ?? 0
  const replaceKey = `${folder}|${replaced.map((f) => f.pageId).join(',')}`
  const understood = understoodFor === replaceKey

  const run = async () => {
    if (!input || !plan) return
    setRunning(true)
    try {
      const r = await commands.importMarkdownFolder(input, replaced.map((f) => f.pageId ?? ''))
      toast.success(im.imported(r.created, r.replaced, r.skipped), {
        description: r.backupPath ? im.backupMade(r.backupPath) : undefined,
      })
      onClose()
    } catch (e) {
      toast.error(errorMessage(e))
      refresh()
    } finally {
      setRunning(false)
    }
  }

  return (
    <Dialog open={isOpen && folder !== null} onOpenChange={(o) => !o && !running && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{im.title}</DialogTitle>
          <DialogDescription>{im.description}</DialogDescription>
        </DialogHeader>
        {/* min-w-0: a grid item, so a long folder path can't widen the dialog. */}
        <div className="min-w-0 space-y-4">
          <FolderField folder={folder} onPick={() => void pickFolder(im.title).then((f) => f && setFolder(f))} />
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label>{t.common.category}</Label>
              <Select
                value={category}
                onValueChange={(v) => {
                  setCategoryId(v)
                  setGroupChoice(NO_GROUP)
                }}
              >
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t.common.category} />
                </SelectTrigger>
                <SelectContent>
                  {categories.map((c) => (
                    <SelectItem key={c.id} value={c.id}>{c.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>{t.common.group}</Label>
              <Select value={groupChoice} onValueChange={setGroupChoice}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_GROUP}>{im.noGroup}</SelectItem>
                  {groups.map((g) => (
                    <SelectItem key={g.id} value={g.id}>{g.name}</SelectItem>
                  ))}
                  <SelectItem value={NEW_GROUP}>{im.newGroupItem}</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
          {groupChoice === NEW_GROUP && <Input autoFocus value={newGroup} onChange={(e) => setNewGroup(e.target.value)} placeholder={im.newGroupPlaceholder} />}

          {error && <p className="text-sm text-destructive">{error}</p>}
          {plan && (
            <div className="space-y-3">
              <p className="text-sm">
                {im.summary(
                  <span className="font-medium">{im.newPages(created.length)}</span>,
                  replaced.length > 0 && <span className="font-medium">{im.replaced(replaced.length)}</span>,
                  skipped.length > 0 && im.skipped(skipped.length),
                )}
                {images > 0 && im.imagesCopied(images)}
                {missing > 0 && im.missingImages(missing)}
              </p>
              {created.length > 0 && (
                <Fold title={im.newPagesList(created.length)}>
                  {created.map((f) => (
                    <div key={f.path} className="truncate">{f.title} <span className="text-muted-foreground">· {f.path}</span></div>
                  ))}
                </Fold>
              )}
              {skipped.length > 0 && (
                <Fold title={im.skippedList(skipped.length)}>
                  {skipped.map((f) => (
                    <div key={f.path} className="truncate">{f.path}: <span className="text-muted-foreground">{f.reason}</span></div>
                  ))}
                </Fold>
              )}
              {replaced.length > 0 && (
                <ReplaceConfirm
                  title={im.replaceTitle(replaced.length)}
                  explain={im.replaceExplain}
                  items={replaced.map((f) => `${f.title}${f.pageArchived ? im.archivedSuffix : ''}  ←  ${f.path}`)}
                  checked={understood}
                  onChecked={(v) => setUnderstoodFor(v ? replaceKey : null)}
                />
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={running}>
            {t.common.cancel}
          </Button>
          <Button onClick={() => void run()} disabled={running || !plan || created.length + replaced.length === 0 || (replaced.length > 0 && !understood)}>
            {running ? im.importing : replaced.length ? im.importAndReplace(replaced.length) : im.import}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// ---------------------------------------------------------------- export

const NONE = '__none__'

/** All, some or none of `ids` chosen, for a tri-state box. */
function stateOf(ids: string[], chosen: Set<string>): boolean | 'indeterminate' {
  const n = ids.filter((id) => chosen.has(id)).length
  return n === 0 ? false : n === ids.length ? true : 'indeterminate'
}

function TreeRow({
  depth,
  expandable,
  expanded,
  onExpand,
  checked,
  disabled,
  onCheck,
  children,
  count,
}: {
  depth: number
  expandable?: boolean
  expanded?: boolean
  onExpand?: () => void
  checked: boolean | 'indeterminate'
  disabled?: boolean
  onCheck: (on: boolean) => void
  children: React.ReactNode
  count?: string
}) {
  const f = useT().knowledge.folders
  return (
    <div className="flex items-center gap-1.5 rounded-lg py-1 pr-2 hover:bg-muted" style={{ paddingLeft: `${0.25 + depth * 1.25}rem` }}>
      {expandable ? (
        <button type="button" onClick={onExpand} aria-label={expanded ? f.collapse : f.expand} className="text-muted-foreground">
          <ChevronRight className={cn('size-4 transition-transform', expanded && 'rotate-90')} />
        </button>
      ) : (
        <span className="w-4 shrink-0" />
      )}
      <label className={cn('flex min-w-0 flex-1 items-center gap-2 text-sm', disabled && 'opacity-50')}>
        <Checkbox checked={checked} disabled={disabled} onCheckedChange={(v) => onCheck(v === true)} />
        <span className="min-w-0 truncate">{children}</span>
      </label>
      {count && <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{count}</span>}
    </div>
  )
}

/** FR-KN-MD-3/4: any pages to a folder, picked one by one or by whole groups and categories. The
 *  folder layout follows the groups and categories of the chosen pages. */
export function ExportFolderDialog({ open: isOpen, onClose: close }: { open: boolean; onClose: () => void }) {
  const onClose = () => {
    setUnderstoodFor(null)
    close()
  }
  const categories = useCategoryList()
  const allGroups = useGroups()
  const allPages = usePageList(EVERY_PAGE)
  const [chosen, setChosen] = useState<Set<string>>(new Set())
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [query, setQuery] = useState('')
  const [folder, setFolder] = useState<string | null>(null)
  // The tick is for one list of replacements; a different list needs a new tick.
  const [understoodFor, setUnderstoodFor] = useState<string | null>(null)
  const [running, setRunning] = useState(false)
  const t = useT()
  const ex = t.knowledge.exportFolder

  // The tree: category → its pages in no group, and its groups → pages; filtered by title.
  const tree = useMemo(() => {
    const q = query.trim().toLowerCase()
    const shown = allPages
      .filter((p) => !q || p.title.toLowerCase().includes(q))
      .sort((a, b) => a.title.localeCompare(b.title, undefined, { sensitivity: 'base' }))
    return categories
      .map((c) => {
        const pages = shown.filter((p) => p.categoryId === c.id)
        const buckets = [
          ...allGroups
            .filter((g) => g.categoryId === c.id)
            .map((g) => ({ key: `${c.id}:${g.id}`, name: g.name, pages: pages.filter((p) => p.groupId === g.id) })),
          { key: `${c.id}:${NONE}`, name: ex.noGroupBucket, pages: pages.filter((p) => !p.groupId) },
        ].filter((b) => b.pages.length > 0 || (!q && !b.key.endsWith(NONE)))
        return { category: c, pages, buckets }
      })
      .filter((c) => c.pages.length > 0 || !q)
  }, [categories, allGroups, allPages, query, ex.noGroupBucket])
  const filtering = query.trim().length > 0
  const isOpenNode = (key: string) => filtering || expanded.has(key)
  const toggleNode = (key: string) =>
    setExpanded((e) => {
      const next = new Set(e)
      if (next.has(key)) next.delete(key)
      else next.add(key)
      return next
    })
  const choose = (ids: string[], on: boolean) =>
    setChosen((c) => {
      const next = new Set(c)
      for (const id of ids) {
        if (on) next.add(id)
        else next.delete(id)
      }
      return next
    })

  const pageIds = useMemo(() => [...chosen].filter((id) => allPages.some((p) => p.id === id)).sort(), [chosen, allPages])
  const input: FolderExportInput | null = folder && pageIds.length ? { folder, pageIds } : null
  const { plan, error, refresh } = usePlan<FolderExportInput, ExportPlan>(isOpen ? input : null, (i) => commands.planMarkdownExport(i))
  const replaced = plan?.files.filter((f) => f.exists) ?? []
  const replaceKey = `${folder}|${replaced.map((f) => f.path).join('|')}`
  const understood = understoodFor === replaceKey

  const run = async () => {
    if (!input || !plan) return
    setRunning(true)
    try {
      const r = await commands.exportMarkdownFolder(input, replaced.map((f) => f.path))
      toast.success(ex.exported(r.written, r.replaced), { description: folder ?? undefined })
      onClose()
    } catch (e) {
      toast.error(errorMessage(e))
      refresh()
    } finally {
      setRunning(false)
    }
  }

  return (
    <Dialog open={isOpen} onOpenChange={(o) => !o && !running && onClose()}>
      <DialogContent className="sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>{ex.title}</DialogTitle>
          <DialogDescription>{ex.description}</DialogDescription>
        </DialogHeader>
        {/* min-w-0: grid items, so a long folder path can't widen the dialog. */}
        <div className="grid min-w-0 gap-5 md:grid-cols-[1.1fr_1fr]">
          <div className="flex min-w-0 flex-col gap-2">
            <div className="flex items-center gap-2">
              <div className="relative flex-1">
                <Search className="absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
                <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={ex.filterPlaceholder} className="pl-8" />
              </div>
              <span className="shrink-0 text-sm text-muted-foreground tabular-nums">{ex.chosen(pageIds.length)}</span>
              <Button type="button" variant="ghost" size="sm" disabled={!pageIds.length} onClick={() => setChosen(new Set())}>
                {ex.clear}
              </Button>
            </div>
            <div className="h-[52vh] min-h-64 overflow-y-auto rounded-xl border p-1.5">
              {tree.map(({ category, pages, buckets }) => {
                const ids = pages.map((p) => p.id)
                return (
                  <div key={category.id}>
                    <TreeRow
                      depth={0}
                      expandable
                      expanded={isOpenNode(category.id)}
                      onExpand={() => toggleNode(category.id)}
                      checked={stateOf(ids, chosen)}
                      disabled={ids.length === 0}
                      onCheck={(on) => choose(ids, on)}
                      count={ids.length ? `${ids.filter((id) => chosen.has(id)).length}/${ids.length}` : ex.noPages}
                    >
                      <span className="font-medium">{category.name}</span>
                      {category.status === 'archived' && <span className="ml-1.5 text-xs text-muted-foreground">{ex.archived}</span>}
                    </TreeRow>
                    {isOpenNode(category.id) &&
                      buckets.map((b) => {
                        const bIds = b.pages.map((p) => p.id)
                        return (
                          <div key={b.key}>
                            <TreeRow
                              depth={1}
                              expandable={bIds.length > 0}
                              expanded={isOpenNode(b.key)}
                              onExpand={() => toggleNode(b.key)}
                              checked={stateOf(bIds, chosen)}
                              disabled={bIds.length === 0}
                              onCheck={(on) => choose(bIds, on)}
                              count={bIds.length ? `${bIds.filter((id) => chosen.has(id)).length}/${bIds.length}` : ex.noPages}
                            >
                              <span className={cn(b.key.endsWith(NONE) && 'text-muted-foreground italic')}>{b.name}</span>
                            </TreeRow>
                            {isOpenNode(b.key) &&
                              b.pages.map((p) => (
                                <TreeRow key={p.id} depth={2} checked={chosen.has(p.id)} onCheck={(on) => choose([p.id], on)}>
                                  {p.title}
                                  {p.archived && <span className="ml-1.5 text-xs text-muted-foreground">{ex.archived}</span>}
                                </TreeRow>
                              ))}
                          </div>
                        )
                      })}
                  </div>
                )
              })}
              {tree.length === 0 && <p className="px-3 py-6 text-center text-sm text-muted-foreground">{ex.noPagesMatch}</p>}
            </div>
          </div>

          <div className="flex min-w-0 flex-col gap-3">
            <div className="space-y-1.5">
              <Label>{ex.folder}</Label>
              <FolderField folder={folder} onPick={() => void pickFolder(ex.title).then((f) => f && setFolder(f))} />
            </div>
            {error && <p className="text-sm text-destructive">{error}</p>}
            {!pageIds.length && <p className="text-sm text-muted-foreground">{ex.choosePages}</p>}
            {pageIds.length > 0 && !folder && <p className="text-sm text-muted-foreground">{ex.chooseFolder}</p>}
            {plan && (
              <>
                <p className="text-sm">
                  {ex.willBeWritten(
                    <span className="font-medium">{ex.files(plan.files.length)}</span>,
                    replaced.length > 0 && <span className="font-medium">{ex.replacing(replaced.length)}</span>,
                  )}
                </p>
                <div className="min-h-24 flex-1 overflow-y-auto rounded-xl border px-2.5 py-1.5 font-mono text-xs leading-6 md:max-h-[32vh]">
                  {plan.files.map((f) => (
                    <div key={f.path} className="truncate" title={f.path}>
                      {f.path} {f.exists && <span className="font-sans text-tint-amber-fg">{ex.replaces}</span>}
                    </div>
                  ))}
                </div>
                {replaced.length > 0 && (
                  <ReplaceConfirm
                    title={ex.replaceTitle(replaced.length)}
                    explain={ex.replaceExplain}
                    items={replaced.map((f) => f.path)}
                    checked={understood}
                    onChecked={(v) => setUnderstoodFor(v ? replaceKey : null)}
                  />
                )}
              </>
            )}
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={running}>
            {t.common.cancel}
          </Button>
          <Button onClick={() => void run()} disabled={running || !plan || plan.files.length === 0 || (replaced.length > 0 && !understood)}>
            {running ? ex.exporting : replaced.length ? ex.exportAndReplace(replaced.length) : ex.exportPages(pageIds.length)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
