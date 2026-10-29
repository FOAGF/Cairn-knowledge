// "Check all unchecked" as one list (FR-KN-SV-4, UI-KN-7). `bulkChecks.ts` runs the checks, several
// pages at a time; this screen shows a row per page, the actions on many pages at once, and the
// details of one page when its row is opened.
import { useQuery } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, CircleCheck, Pause, Play, RefreshCw, ShieldCheck } from 'lucide-react'
import { Fragment, useEffect, useMemo, useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { commands, type GroupPick, type PageSummary } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { type PageFilter, useGroups, usePages } from '@/api/knowledge'
import { useCategoryList } from '@/api/categories'
import { rankedByMeaning } from '@/api/search'
import { useSettings } from '@/api/settings'
import { EmptyState, PageHeader } from '@/components/shared/PageHeader'
import { CategoryDot } from '@/components/shared/tint'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { type SourcedTerm, TermSuggestions } from '@/features/glossary/TermSuggestions'
import { cn } from '@/lib/utils'
import { type CheckRow, dropStale, markChecked, patchRow, runChecks, severalTopics, useCheckRows } from './bulkChecks'
import { DuplicateMatches, UnavailableNote, Waiting } from './parts'
import { openAsPending } from './pending'
import { draftSource } from './helpers'
import { ensureGroupPath, groupPath, isGroupPath } from '@/features/knowledge/groupTree'
import { useDuplicateMatches } from './useSimilar'
import { getT, useT } from '@/i18n'

const NOT_CHECKED: PageFilter = { checkStatus: 'not_checked' }

/** The suggestion changes something: a new group, or another existing one. */
const moves = (g: GroupPick | null | undefined, page: PageSummary): g is GroupPick => !!g && (g.kind === 'new' || g.groupId !== page.groupId)

/** Puts the pages into their suggested groups; a new group is created once per category and name. */
async function acceptGroups(items: { page: PageSummary; group: GroupPick }[]) {
  const made = new Map<string, string>()
  let moved = 0
  for (const { page, group } of items) {
    try {
      const key = `${page.categoryId}\n${group.name.toLowerCase()}`
      let groupId = group.kind === 'existing' ? group.groupId : made.get(key)
      if (!groupId) {
        groupId = isGroupPath(group.name) ? await ensureGroupPath(page.categoryId, group.name) : (await commands.createGroup({ categoryId: page.categoryId, name: group.name })).id
        made.set(key, groupId)
      }
      const saved = await commands.updatePage(page.id, { groupId })
      patchRow(page.id, { updatedAt: saved.updatedAt })
      moved++
    } catch (e) {
      toast.error(getT().creation.bulk.groupFailed(page.title, errorMessage(e)))
    }
  }
  if (moved) toast.success(getT().creation.bulk.moved(moved))
}

export function BulkCheckScreen() {
  const unchecked = usePages(NOT_CHECKED)
  const settings = useSettings()
  const categories = useCategoryList()
  const groups = useGroups()
  // The list is fixed when the screen opens, so pages marked checked stay in it.
  const [queue, setQueue] = useState<PageSummary[] | null>(null)
  useEffect(() => {
    if (queue !== null || !unchecked.data) return
    dropStale(unchecked.data) // results for an older version of a page are checked again
    setQueue(unchecked.data)
  }, [queue, unchecked.data])
  const ids = useMemo(() => queue?.map((p) => p.id) ?? [], [queue])
  // The pages as they are now (group, check status); a merged or deleted page drops out.
  const live = useQuery({ queryKey: ['knowledge', 'pages', 'by-ids', ids], enabled: ids.length > 0, queryFn: () => commands.getPagesByIds(ids) }).data
  const pages = useMemo(() => (queue ?? []).flatMap((q) => live?.find((p) => p.id === q.id) ?? (live ? [] : [q])), [queue, live])
  const rows = useCheckRows((s) => s.rows)

  const [paused, setPaused] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [open, setOpen] = useState<string | null>(null)
  const [resolvedTerms, setResolvedTerms] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const t = useT()

  // Pages start while there is room, `checksAtOnce` at a time, in list order. Leaving the screen or
  // *Pause* stops new ones; the running ones finish and are kept.
  const running = useRef(new Set<string>())
  const [tick, setTick] = useState(0)
  useEffect(() => {
    if (paused || !queue) return
    const free = settings.checksAtOnce - running.current.size
    if (free <= 0) return
    const known = useCheckRows.getState().rows
    for (const p of queue.filter((q) => !known[q.id] && !running.current.has(q.id)).slice(0, free)) {
      running.current.add(p.id)
      void runChecks(p.id).finally(() => {
        running.current.delete(p.id)
        setTick((t) => t + 1)
      })
    }
  }, [paused, queue, settings.checksAtOnce, tick])

  const terms = useMemo(() => {
    const byTerm = new Map<string, SourcedTerm>()
    for (const p of pages) {
      for (const t of rows[p.id]?.report?.terms ?? []) {
        const key = t.term.toLowerCase()
        const from = { title: p.title, categoryId: p.categoryId }
        const seen = byTerm.get(key)
        if (seen) seen.from = [...(seen.from ?? []), from]
        else byTerm.set(key, { ...t, from: [from] })
      }
    }
    return [...byTerm.values()].filter((t) => !resolvedTerms.has(t.term))
  }, [pages, rows, resolvedTerms])

  if (queue === null) return <p className="pt-10 text-center text-sm text-muted-foreground">{t.creation.loading}</p>
  if (!queue.length)
    return (
      <div className="mx-auto max-w-xl pt-10">
        <EmptyState icon={CircleCheck} title={t.creation.bulk.emptyTitle}>
          {t.creation.bulk.emptyText}
          <div className="mt-4"><Button asChild><Link to="/knowledge">{t.creation.bulk.backToKnowledge}</Link></Button></div>
        </EmptyState>
      </div>
    )

  const isChecked = (p: PageSummary) => p.checkStatus === 'checked' || !!rows[p.id]?.checked
  const finished = pages.filter((p) => isChecked(p) || (rows[p.id] && rows[p.id].state !== 'running')).length
  const count = (f: (r: CheckRow | undefined, p: PageSummary) => boolean) => pages.filter((p) => f(rows[p.id], p)).length
  const auto = count((r) => r?.checked === 'auto')
  const held = count((r, p) => !isChecked(p) && r?.state === 'held')
  const several = count((r, p) => !isChecked(p) && severalTopics(r))
  const failed = count((r, p) => !isChecked(p) && r?.state === 'error')
  const open_ = pages.filter((p) => !isChecked(p))
  const chosen = pages.filter((p) => selected.has(p.id))
  const groupMoves = chosen.flatMap((p) => {
    const g = rows[p.id]?.report?.group
    return moves(g, p) ? [{ page: p, group: g }] : []
  })
  const toggle = (id: string) => setSelected((s) => {
    const n = new Set(s)
    if (n.has(id)) n.delete(id)
    else n.add(id)
    return n
  })
  const allOpenSelected = open_.length > 0 && open_.every((p) => selected.has(p.id))

  async function markSelected() {
    setBusy(true)
    const todo = chosen.filter((p) => !isChecked(p))
    const results = await Promise.allSettled(todo.map((p) => markChecked(p.id, 'user')))
    setBusy(false)
    setSelected(new Set())
    const done = results.filter((r) => r.status === 'fulfilled').length
    if (done) toast.success(t.creation.bulk.marked(done))
    if (done < results.length) toast.error(t.creation.bulk.markFailed(results.length - done))
  }
  async function acceptSelectedGroups() {
    setBusy(true)
    await acceptGroups(groupMoves)
    setBusy(false)
  }

  return (
    <>
      <PageHeader
        icon={ShieldCheck}
        tint="green"
        title={t.creation.bulk.title}
        description={t.creation.bulk.description(pages.length, settings.checksAtOnce)}
        actions={
          <>
            {finished < pages.length && (
              <Button variant="outline" onClick={() => setPaused(!paused)}>
                {paused ? <><Play /> {t.creation.bulk.resume}</> : <><Pause /> {t.creation.bulk.pause}</>}
              </Button>
            )}
            <Button variant="outline" asChild><Link to="/knowledge">{t.creation.bulk.backToKnowledge}</Link></Button>
          </>
        }
      />

      <div className="mb-4 space-y-2">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
          <span className="font-medium">{t.creation.bulk.progress(finished, pages.length, paused && finished < pages.length)}</span>
          {auto > 0 && <span className="text-tint-green-fg">{t.creation.bulk.auto(auto)}</span>}
          {several > 0 && <span className="text-tint-amber-fg">{t.creation.bulk.several(several)}</span>}
          {held > 0 && <span className="text-tint-amber-fg">{t.creation.bulk.held(held)}</span>}
          {failed > 0 && <span className="text-destructive">{t.creation.bulk.failed(failed)}</span>}
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-muted">
          <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${(finished / Math.max(1, pages.length)) * 100}%` }} />
        </div>
      </div>

      {terms.length > 0 && (
        <TermSuggestions className="mb-4" categoryId={pages[0]?.categoryId ?? ''} terms={terms} onResolved={(t) => setResolvedTerms((r) => new Set(r).add(t))} />
      )}

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <span className="text-sm text-muted-foreground">{chosen.length ? t.creation.bulk.selected(chosen.length) : t.creation.bulk.selectHint}</span>
        <Button size="sm" disabled={busy || !chosen.some((p) => !isChecked(p))} onClick={() => void markSelected()}>
          <CircleCheck /> {t.creation.markAsChecked}
        </Button>
        <Button size="sm" variant="outline" disabled={busy || !groupMoves.length} onClick={() => void acceptSelectedGroups()}>
          {t.creation.bulk.acceptGroups(groupMoves.length)}
        </Button>
      </div>

      <div className="overflow-hidden rounded-2xl border bg-card">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-10">
                <Checkbox
                  aria-label={t.creation.bulk.selectAll}
                  checked={allOpenSelected}
                  onCheckedChange={() => setSelected(allOpenSelected ? new Set() : new Set(open_.map((p) => p.id)))}
                />
              </TableHead>
              <TableHead>{t.creation.bulk.page}</TableHead>
              <TableHead>{t.creation.bulk.topic}</TableHead>
              <TableHead>{t.common.group}</TableHead>
              <TableHead>{t.creation.similarPages}</TableHead>
              <TableHead>{t.common.status}</TableHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {pages.map((p) => {
              const r = rows[p.id]
              const category = categories.find((x) => x.id === p.categoryId)
              const group = groups.find((g) => g.id === p.groupId)
              const top = r?.duplicates?.hits[0]
              const suggested = r?.report?.group
              const expanded = open === p.id
              return (
                <Fragment key={p.id}>
                  <TableRow className={cn('cursor-pointer', expanded && 'bg-muted/40')} onClick={() => setOpen(expanded ? null : p.id)}>
                    <TableCell onClick={(e) => e.stopPropagation()}>
                      <Checkbox aria-label={t.creation.bulk.select(p.title)} checked={selected.has(p.id)} onCheckedChange={() => toggle(p.id)} />
                    </TableCell>
                    <TableCell className="max-w-72">
                      <p className="truncate font-medium">{p.title}</p>
                      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <CategoryDot color={category?.color ?? 'slate'} className="size-2" />
                        <span className="truncate">{category?.name}</span>
                      </p>
                    </TableCell>
                    <TableCell className="text-sm">
                      <TopicCell row={r} />
                    </TableCell>
                    <TableCell className="max-w-56 text-sm">
                      <span className="truncate">{group ? groupPath(group, groups) : <span className="text-muted-foreground">{t.common.none}</span>}</span>
                      {moves(suggested, p) && (
                        <span className="block truncate text-xs text-tint-purple-fg">
                          → {suggested.kind === 'new' ? t.creation.newGroupName(suggested.name) : suggested.name}
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="max-w-64 text-sm">
                      {top ? (
                        <span className={cn('block truncate', top.likely && 'font-medium text-tint-amber-fg')}>
                          {top.title}
                          {top.score !== null && (!r?.duplicates || rankedByMeaning(r.duplicates.meaning)) && ` ${Math.round(top.score * 100)}%`}
                        </span>
                      ) : r?.duplicates ? (
                        <span className="text-muted-foreground">{t.common.none}</span>
                      ) : null}
                    </TableCell>
                    <TableCell>
                      <StatusCell row={r} checked={isChecked(p)} />
                    </TableCell>
                    <TableCell>{expanded ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}</TableCell>
                  </TableRow>
                  {expanded && (
                    <TableRow className="bg-muted/20 hover:bg-muted/20">
                      <TableCell colSpan={7} className="whitespace-normal">
                        <RowDetails page={p} row={r} checked={isChecked(p)} />
                      </TableCell>
                    </TableRow>
                  )}
                </Fragment>
              )
            })}
          </TableBody>
        </Table>
      </div>
    </>
  )
}

function TopicCell({ row }: { row: CheckRow | undefined }) {
  const t = useT()
  if (!row) return null
  if (row.state === 'running') return row.step === 'model' ? <Waiting text={t.creation.checks.checking} /> : null
  if (row.state === 'held') return <span className="text-muted-foreground">{t.creation.bulk.waits}</span>
  if (row.noModel) return <span className="text-muted-foreground">{t.common.skipped}</span>
  if (!row.report) return null
  if (row.topicNotNeeded) return <span className="text-muted-foreground">{t.creation.checks.notNeeded}</span>
  return row.report.oneTopic ? <span>{t.creation.bulk.one}</span> : <span className="font-medium text-tint-amber-fg">{t.creation.bulk.severalN(row.report.topics.length)}</span>
}

function StatusCell({ row, checked }: { row: CheckRow | undefined; checked: boolean }) {
  const t = useT()
  if (checked)
    return <Badge className="bg-tint-green text-tint-green-fg">{row?.checked === 'auto' ? t.creation.bulk.checkedAuto : t.creation.bulk.checked}</Badge>
  if (!row) return <span className="text-xs text-muted-foreground">{t.creation.bulk.waiting}</span>
  if (row.state === 'running') return <Waiting text={row.step === 'model' ? t.creation.bulk.modelCheck : t.creation.bulk.similar} />
  if (row.state === 'held') return <Badge className="bg-tint-amber text-tint-amber-fg">{t.creation.bulk.likelyDuplicate}</Badge>
  if (row.state === 'error') return <Badge variant="destructive">{t.common.failed}</Badge>
  if (row.noModel) return <Badge variant="secondary">{t.creation.bulk.noModel}</Badge>
  if (severalTopics(row)) return <Badge className="bg-tint-amber text-tint-amber-fg">{t.creation.checks.severalTopics}</Badge>
  return <Badge className="bg-tint-amber text-tint-amber-fg">{t.creation.bulk.needsLook}</Badge>
}

/** One page opened in the list: what the checks found, and what can be done about it. */
function RowDetails({ page, row, checked }: { page: PageSummary; row: CheckRow | undefined; checked: boolean }) {
  const navigate = useNavigate()
  const { matches, meaning } = useDuplicateMatches(row?.duplicates ?? null)
  const suggestion = row?.report?.group
  const t = useT()
  // Merge into…: the page's content becomes the pending page; the merge screen decides the rest.
  const mergeInto = async (targetId: string) => {
    try {
      const full = await commands.getPage(page.id)
      await openAsPending(
        { source: draftSource(full.origin), categoryId: full.categoryId, groupId: full.groupId ?? undefined, title: full.title, body: full.body, tags: full.tags, sourceInfo: full.sourceInfo ?? undefined, originTaskId: full.originTaskId ?? undefined, fromPageId: full.id },
        `/knowledge/merge/${targetId}`,
        navigate,
      )
    } catch (e) {
      toast.error(errorMessage(e))
    }
  }
  const mark = () => void markChecked(page.id, 'user').then(() => toast.success(t.creation.markedChecked(page.title)), (e) => toast.error(errorMessage(e)))

  return (
    <div className="grid gap-4 py-2 lg:grid-cols-[1fr_1.2fr]">
      <div className="space-y-3 text-sm">
        {row?.state === 'held' && (
          <p className="rounded-xl bg-tint-amber/60 px-3 py-2 text-tint-amber-fg">
            {t.creation.bulk.heldText}
          </p>
        )}
        {row?.state === 'error' && <p className="rounded-xl bg-destructive/10 px-3 py-2 text-destructive">{row.error}</p>}
        {row?.noModel && <UnavailableNote message={row.noModel} />}
        {severalTopics(row) && (
          <div>
            <p className="font-medium">{t.creation.bulk.topics}</p>
            <ul className="mt-1 list-disc pl-5 text-muted-foreground">{row!.report!.topics.map((t) => <li key={t}>{t}</li>)}</ul>
          </div>
        )}
        {row?.report && (
          <p>
            <span className="text-muted-foreground">{t.creation.bulk.suggestedGroup}</span>
            {suggestion ? (
              <>
                <span className="rounded-full bg-tint-purple px-2.5 py-0.5 text-xs font-medium text-tint-purple-fg">{suggestion.kind === 'new' ? t.creation.newGroupName(suggestion.name) : suggestion.name}</span>
                {moves(suggestion, page) && (
                  <Button size="xs" variant="outline" className="ml-2" onClick={() => void acceptGroups([{ page, group: suggestion }])}>{t.creation.checks.accept}</Button>
                )}
              </>
            ) : (
              <span>{t.creation.bulk.none}</span>
            )}
          </p>
        )}
        <div className="flex flex-wrap gap-2 pt-1">
          <Button size="sm" variant="outline" asChild><Link to={`/knowledge/${page.id}`}>{t.creation.bulk.openPage}</Link></Button>
          {!checked && row && row.state !== 'running' && <Button size="sm" onClick={mark}><CircleCheck /> {t.creation.markAsChecked}</Button>}
          {row?.state === 'held' && <Button size="sm" variant="outline" onClick={() => void runChecks(page.id, { force: true })}>{t.creation.bulk.forceCheck}</Button>}
          {row && (row.state === 'done' || row.state === 'error') && !checked && (
            <Button size="sm" variant="ghost" onClick={() => void runChecks(page.id)}><RefreshCw /> {t.creation.bulk.checkAgain}</Button>
          )}
        </div>
      </div>
      <div>
        <p className="mb-2 text-sm font-medium">{t.creation.similarPages}</p>
        {row?.duplicates ? (
          <DuplicateMatches matches={matches} meaning={meaning} mergeLabel={t.creation.mergeInto} onMerge={(t) => void mergeInto(t.id)} />
        ) : (
          <p className="text-sm text-muted-foreground">{row?.state === 'running' ? t.creation.bulk.looking : t.creation.bulk.notChecked}</p>
        )}
      </div>
    </div>
  )
}
