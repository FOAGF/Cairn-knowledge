// Manual merge screen (§5.6, FR-KN-DU-3..6): existing page left, new content right, editable result below.
import { Combine, EyeOff, FileX2, Loader2, Plus, Replace, Sparkles, Square, Undo2 } from 'lucide-react'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'
import { MarkdownField } from '@/components/editor/MarkdownField'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { EmptyState, PageHeader, SectionCard } from '@/components/shared/PageHeader'
import { CategoryBadge, TagChip } from '@/components/shared/tint'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { useAppStore } from '@/store'
import { joinBlocks, overlap, splitBlocks, type Block } from './helpers'
import { useCategoryList } from '@/api/categories'
import { useTask } from '@/api/tasks'
import { commands } from '@/api/bindings'
import { matchMergeBlocks, suggestMerge as suggestMergeCall } from '@/api/creation'
import { errorMessage } from '@/api/client'
import { cancelJob, clearStream, isCancelled, isLlmUnavailable, newStreamId, useLlmStream } from '@/api/llm'
import { type Page, useDeletePage, usePage, useUpdatePage } from '@/api/knowledge'
import { findDuplicates } from '@/api/checks'
import { useDuplicateMatches } from './useSimilar'
import { getT, useT } from '@/i18n'

type Decision = { kind: 'add' } | { kind: 'replace'; target: number } | { kind: 'ignore' }

const numbers = (t: string) => t.match(/\d+(?:[.,]\d+)*/g)?.join(' ') ?? ''

const oneLine = (t: string) => t.replace(/^[#>\-*+\d.\s]+/, '').replace(/\s+/g, ' ').slice(0, 60) || getT().creation.merge.emptyBlock

export default function MergePage() {
  const { targetPageId } = useParams()
  const { data: target, isPending } = usePage(targetPageId)
  const t = useT()
  if (isPending) return <p className="pt-10 text-center text-sm text-muted-foreground">{t.creation.loading}</p>
  return <MergeScreen target={target ?? null} />
}

function MergeScreen({ target }: { target: Page | null }) {
  const navigate = useNavigate()
  const draft = useAppStore((s) => s.draft)
  const setDraft = useAppStore((s) => s.setDraft)
  const originTask = useTask(draft?.originTaskId).data
  const categories = useCategoryList()
  const { mutateAsync: updatePage } = useUpdatePage()
  const { mutateAsync: deletePage } = useDeletePage()
  const saveDraftAsPage = useAppStore((s) => s.saveDraftAsPage)
  const setDraftPath = useAppStore((s) => s.setDraftPath)
  // The pending page's tab comes back here while the merge is open (features/creation/pending.tsx).
  useEffect(() => {
    if (useAppStore.getState().draft) setDraftPath(location.pathname)
  }, [setDraftPath])
  const [saving, setSaving] = useState(false)

  const existing = useMemo(() => splitBlocks(target?.body ?? ''), [target?.body])
  const incoming = useMemo(() => splitBlocks(draft?.body ?? ''), [draft?.body])
  const [decisions, setDecisions] = useState<Record<number, Decision>>({})
  const [result, setResult] = useState(target?.body ?? '')
  const [streamId, setStreamId] = useState<string | null>(null)
  const [suggested, setSuggested] = useState<{ incomplete: boolean } | null>(null)
  const suggesting = streamId !== null
  const streamed = useLlmStream(streamId)
  const t = useT()

  // FR-KN-DU-5: *Suggest merge* only for the highest-ranked match of the duplicate check (by meaning).
  const duplicateInput = draft ? { categoryId: draft.categoryId, title: draft.title, body: draft.body, excludePageId: draft.fromPageId } : null
  const { data: duplicates } = useQuery({
    queryKey: ['search', 'duplicates', duplicateInput],
    enabled: duplicateInput !== null,
    queryFn: () => findDuplicates(duplicateInput!),
  })
  const { matches } = useDuplicateMatches(duplicates ?? null)
  const topMatch = matches[0]

  // FR-KN-DU-3: where each new block starts, by meaning; by keyword overlap when the search model
  // can't be used. Decided once per page and new content; the user's choices rule after that.
  const targetId = target?.id
  const targetBody = target?.body
  const draftBody = draft?.body
  useEffect(() => {
    if (targetId === undefined || draftBody === undefined) return
    let cancelled = false
    const ex = splitBlocks(targetBody ?? '')
    const inc = splitBlocks(draftBody)
    const start = (next: Record<number, Decision>) => {
      if (cancelled) return
      setDecisions(next)
      const blocks: Block[] = ex.map((b) => ({ ...b }))
      const added: Block[] = []
      inc.forEach((b, i) => {
        const d = next[i]
        if (d?.kind === 'replace') blocks[d.target] = { ...b }
        else if (d?.kind === 'add') added.push(b)
      })
      setResult(joinBlocks([...blocks, ...added]))
    }
    const byKeywords = () => {
      const next: Record<number, Decision> = {}
      inc.forEach((b, i) => {
        // A changed number is never dropped quietly: only a block with the page's numbers can be ignored.
        const same = overlap(b.text, targetBody ?? '') >= 0.9 && ex.some((e) => numbers(e.text) === numbers(b.text) && overlap(b.text, e.text) >= 0.9)
        next[i] = same ? { kind: 'ignore' } : { kind: 'add' }
      })
      return next
    }
    matchMergeBlocks(
      ex.map((b) => b.text),
      inc.map((b) => b.text),
    )
      .then((r) => {
        if (!r.byMeaning || r.matches.length !== inc.length) return start(byKeywords())
        start(
          Object.fromEntries(
            r.matches.map((m, i): [number, Decision] => [
              i,
              m.action === 'ignore' && m.target != null
                ? { kind: 'ignore' }
                : m.action === 'replace' && m.target != null
                  ? { kind: 'replace', target: m.target }
                  : { kind: 'add' },
            ]),
          ),
        )
      })
      .catch(() => start(byKeywords()))
    return () => {
      cancelled = true
    }
  }, [targetId, targetBody, draftBody])

  if (!target || !draft)
    return (
      <div className="mx-auto max-w-xl pt-10">
        <EmptyState icon={FileX2} title={!target ? t.creation.merge.noTarget : t.creation.merge.noDraft}>
          {t.creation.merge.emptyHint}
          <div className="mt-4 flex justify-center gap-2">
            <Button asChild><Link to="/knowledge/new">{t.common.newPage}</Link></Button>
            <Button variant="outline" asChild><Link to="/knowledge">{t.common.nav.knowledge}</Link></Button>
          </div>
        </EmptyState>
      </div>
    )

  const task = originTask
  const isTop = topMatch?.page.id === target.id

  // Rebuilds the result from the per-block choices (FR-KN-DU-3). It stays editable afterwards.
  function apply(next: Record<number, Decision>) {
    setSuggested(null)
    setDecisions(next)
    const blocks: Block[] = existing.map((b) => ({ ...b }))
    const added: Block[] = []
    incoming.forEach((b, i) => {
      const d = next[i]
      if (d?.kind === 'replace') blocks[d.target] = { ...b }
      else if (d?.kind === 'add') added.push(b)
    })
    setResult(joinBlocks([...blocks, ...added]))
  }
  const choose = (i: number, d: Decision | null) => {
    const next = { ...decisions }
    if (d) next[i] = d
    else delete next[i]
    apply(next)
  }

  // FR-KN-DU-5: the model's merge streams into the result area. Nothing is saved.
  async function suggestMerge() {
    const id = newStreamId()
    setStreamId(id)
    try {
      const out = await suggestMergeCall({ pageId: target!.id, newBody: draft!.body, streamId: id })
      setResult(out.body)
      setSuggested({ incomplete: out.incomplete })
      if (out.incomplete) toast.warning(t.creation.merge.incomplete)
    } catch (e) {
      if (isCancelled(e)) return
      toast.error(isLlmUnavailable(e) ? t.creation.merge.noModel(errorMessage(e)) : errorMessage(e))
    } finally {
      clearStream(id)
      setStreamId(null)
    }
  }

  // FR-KN-DU-3 + DU-6: save, append Source info, add task link and tags.
  async function saveMerged() {
    const src = draft!.sourceInfo?.trim()
    const sourceInfo = src && !target!.sourceInfo?.includes(src) ? [target!.sourceInfo, src].filter(Boolean).join('\n') : target!.sourceInfo
    setSaving(true)
    try {
      await updatePage({ id: target!.id, patch: { body: result, sourceInfo: sourceInfo ?? '', tags: [...new Set([...target!.tags, ...draft!.tags])] } })
      // An existing link is fine; a failed one must not undo the merge.
      if (draft!.originTaskId) await commands.linkTaskPage(draft!.originTaskId, target!.id).catch(() => undefined)
      if (draft!.fromPageId && draft!.fromPageId !== target!.id) await deletePage(draft!.fromPageId)
    } catch {
      return // the error is shown as a toast
    } finally {
      setSaving(false)
    }
    setDraft(null)
    toast.success(t.creation.merge.merged(target!.title))
    navigate(`/knowledge/${target!.id}`)
  }
  async function keepBoth() {
    if (draft!.fromPageId) {
      setDraft(null)
      toast.success(t.creation.merge.keptBoth)
      return navigate(`/knowledge/${draft!.fromPageId}`)
    }
    const page = await saveDraftAsPage()
    if (page) {
      toast.success(t.creation.merge.savedSeparate(page.title))
      navigate(`/knowledge/${page.id}`)
    }
  }
  function discard() {
    setDraft(null)
    toast(t.creation.merge.discarded)
    navigate(`/knowledge/${target!.id}`)
  }

  const replacedIdx = new Set(Object.values(decisions).flatMap((d) => (d.kind === 'replace' ? [d.target] : [])))

  return (
    <>
      <PageHeader
        icon={Combine}
        tint="purple"
        title={t.creation.merge.title(target.title)}
        description={t.creation.merge.description}
        actions={
          isTop ? (
            suggesting ? (
              <Button variant="outline" onClick={() => void cancelJob(streamId)}>
                <Square /> {t.common.stop}
              </Button>
            ) : (
              <Button variant="outline" onClick={() => void suggestMerge()}>
                <Sparkles /> {t.creation.merge.suggest}
              </Button>
            )
          ) : (
            <span className="text-xs text-muted-foreground" title={t.creation.merge.bestOnlyHint}>{t.creation.merge.bestOnly}</span>
          )
        }
      />

      <div className="grid gap-5 lg:grid-cols-2">
        <SectionCard title={t.creation.merge.existing} description={target.title} action={<CategoryBadge category={categories.find((p) => p.id === target.categoryId)} />}>
          <div className="space-y-2">
            {existing.map((b, i) => (
              <div key={i} className={cn('rounded-xl border px-3 py-2', replacedIdx.has(i) && 'border-tint-amber bg-tint-amber/40 opacity-70')}>
                {replacedIdx.has(i) && <Badge className="mb-1 bg-tint-amber text-tint-amber-fg">{t.creation.merge.willBeReplaced}</Badge>}
                <MarkdownView text={b.text} className="text-sm" />
              </div>
            ))}
            {!existing.length && <p className="text-sm text-muted-foreground">{t.creation.merge.empty}</p>}
          </div>
        </SectionCard>

        <SectionCard title={t.creation.merge.newContent} description={draft.title} action={<div className="flex gap-1">{draft.tags.slice(0, 3).map((t) => <TagChip key={t} tag={t} />)}</div>}>
          <div className="space-y-2">
            {incoming.map((b, i) => {
              const d = decisions[i]
              return (
                <div key={i} className={cn('rounded-xl border px-3 py-2', d?.kind === 'add' && 'border-tint-green bg-tint-green/40', d?.kind === 'replace' && 'border-tint-amber bg-tint-amber/40', d?.kind === 'ignore' && 'opacity-50')}>
                  <MarkdownView text={b.text} className={cn('text-sm', d?.kind === 'ignore' && 'line-through')} />
                  <div className="mt-2 flex flex-wrap items-center gap-1.5">
                    <Button size="xs" variant={d?.kind === 'add' ? 'default' : 'outline'} onClick={() => choose(i, { kind: 'add' })}><Plus /> {t.common.add}</Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild><Button size="xs" variant={d?.kind === 'replace' ? 'default' : 'outline'}><Replace /> {t.creation.merge.replace}</Button></DropdownMenuTrigger>
                      <DropdownMenuContent align="start" className="max-h-72 w-72 overflow-y-auto">
                        <DropdownMenuLabel>{t.creation.merge.replaceWhich}</DropdownMenuLabel>
                        {existing.map((e, j) => <DropdownMenuItem key={j} onSelect={() => choose(i, { kind: 'replace', target: j })}><span className="truncate">{j + 1}. {oneLine(e.text)}</span></DropdownMenuItem>)}
                      </DropdownMenuContent>
                    </DropdownMenu>
                    <Button size="xs" variant={d?.kind === 'ignore' ? 'default' : 'outline'} onClick={() => choose(i, { kind: 'ignore' })}><EyeOff /> {t.creation.merge.ignore}</Button>
                    {d && <Button size="xs" variant="ghost" onClick={() => choose(i, null)}><Undo2 /> {t.creation.merge.undo}</Button>}
                    {d?.kind === 'replace' && <span className="text-xs text-muted-foreground">{t.creation.merge.replaces(d.target + 1)}</span>}
                  </div>
                </div>
              )
            })}
          </div>
        </SectionCard>
      </div>

      <SectionCard
        className="mt-5"
        title={t.creation.merge.result}
        description={t.creation.merge.resultHint}
        action={
          suggesting ? (
            <Badge className="bg-tint-purple text-tint-purple-fg"><Loader2 className="animate-spin" /> {t.creation.merge.merging}</Badge>
          ) : (
            suggested && <Badge className="bg-tint-purple text-tint-purple-fg"><Sparkles /> {t.creation.merge.suggested(suggested.incomplete)}</Badge>
          )
        }
      >
        <div className="rounded-xl border bg-background p-4">
          {suggesting ? (
            <MarkdownView text={streamed || '…'} className="min-h-[12rem] text-sm" />
          ) : (
            <MarkdownField small value={result} onChange={(v) => setResult(v)} minHeight="12rem" imageCategoryId={target.categoryId} />
          )}
        </div>
        <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
          <span>
            {t.creation.merge.willAlsoAdd({ sourceInfo: !!draft.sourceInfo, taskKey: task?.key, tags: draft.tags.length > 0, removesOriginal: !!draft.fromPageId })}
          </span>
        </div>
      </SectionCard>

      <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
        <Button variant="ghost" className="text-muted-foreground" onClick={discard}>{t.creation.merge.discard}</Button>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => void keepBoth()}>{t.creation.merge.keepBoth}</Button>
          <Button size="lg" disabled={saving || suggesting} onClick={() => void saveMerged()}>{t.creation.merge.save}</Button>
        </div>
      </div>
    </>
  )
}
