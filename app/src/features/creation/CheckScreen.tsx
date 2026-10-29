// "Run checks" on one existing page (FR-KN-SV-2); "Check all unchecked pages" is a list (BulkCheckScreen.tsx).
import { ChevronRight, CircleCheck, Copy, FolderTree, RefreshCw, ScanSearch, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { EmptyState, PageHeader } from '@/components/shared/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { TermSuggestions } from '@/features/glossary/TermSuggestions'
import { useCreateGroup, useGroups, usePage, type Page, useSetPageCheckStatus, useUpdatePage } from '@/api/knowledge'
import { ensureGroupPath, groupPath, isGroupPath } from '@/features/knowledge/groupTree'
import { draftSource } from './helpers'
import { useDuplicateMatches } from './useSimilar'
import { CheckCard, CheckError, DuplicateMatches, Stepper, UnavailableNote, Waiting } from './parts'
import { usePipeline } from './usePipeline'
import { BulkCheckScreen } from './BulkCheckScreen'
import { openAsPending } from './pending'
import { useT } from '@/i18n'

export function CheckScreen({ target }: { target: string }) {
  if (target === 'unchecked') return <BulkCheckScreen />
  return <CheckOne pageId={target} />
}

function CheckOne({ pageId }: { pageId: string }) {
  const { data: page, isPending } = usePage(pageId)
  const t = useT()
  if (isPending) return <p className="pt-10 text-center text-sm text-muted-foreground">{t.creation.loading}</p>
  if (!page) return <EmptyState icon={ScanSearch} title={t.creation.pageNotFound} />
  return <CheckBody page={page} />
}

function CheckBody({ page }: { page: Page }) {
  const navigate = useNavigate()
  const groups = useGroups(page.categoryId)
  const { mutate: updatePage } = useUpdatePage()
  const { mutateAsync: createGroup } = useCreateGroup()
  const { mutate: setCheckStatus } = useSetPageCheckStatus()
  const [resolvedTerms, setResolvedTerms] = useState<Set<string>>(new Set())
  const t = useT()

  // The page itself is never its own duplicate.
  const pipeline = usePipeline({
    getSubject: () => ({
      title: page.title,
      body: page.body,
      categoryId: page.categoryId,
      source: draftSource(page.origin),
      groupId: page.groupId,
      excludePageId: page.id,
    }),
  })
  const { steps, topic, suggestion, terms, duplicates, skip, unavailable, error, checked, rerun } = pipeline
  const { matches, meaning } = useDuplicateMatches(duplicates)
  const done = steps[3] === 'active'
  const found = groups.find((g) => g.id === page.groupId)
  const group = found && { name: groupPath(found, groups) }

  function markChecked() {
    setCheckStatus(
      { id: page.id, status: 'checked' },
      {
        onSuccess: () => toast.success(t.creation.markedChecked(page.title)),
      },
    )
  }
  // Merge into…: the page content becomes the draft; the merge screen decides the rest.
  function mergeInto(targetId: string) {
    void openAsPending(
      { source: draftSource(page.origin), categoryId: page.categoryId, groupId: page.groupId ?? undefined, title: page.title, body: page.body, tags: page.tags, sourceInfo: page.sourceInfo ?? undefined, originTaskId: page.originTaskId ?? undefined, fromPageId: page.id },
      `/knowledge/merge/${targetId}`,
      navigate,
    )
  }
  async function acceptGroup() {
    if (!suggestion) return
    try {
      const groupId =
        suggestion.kind === 'existing'
          ? suggestion.groupId
          : isGroupPath(suggestion.name)
            ? await ensureGroupPath(page.categoryId, suggestion.name)
            : (await createGroup({ categoryId: page.categoryId, name: suggestion.name })).id
      updatePage({ id: page.id, patch: { groupId } }, { onSuccess: () => toast.success(t.creation.check.moved(suggestion.name)) })
    } catch (e) {
      toast.error(errorMessage(e))
    }
  }

  return (
    <>
      <PageHeader
        icon={ShieldCheck}
        tint="green"
        title={t.creation.check.title(page.title)}
        description={t.creation.check.description}
        actions={done && <Button variant="outline" onClick={() => void rerun({ fresh: true })}><RefreshCw /> {t.creation.check.rerun}</Button>}
      />
      <div className="mb-5 space-y-3">
        <Stepper steps={steps} lastLabel={t.creation.check.results} />
        {error && <CheckError message={error.message} onRetry={pipeline.retry} onSkip={pipeline.skipStep} />}
        {unavailable && <UnavailableNote message={unavailable} />}
      </div>
      <div className="grid gap-4 lg:grid-cols-3">
        <CheckCard icon={ScanSearch} tint="amber" title={t.creation.steps.topic} status={steps[0] === 'active' ? <Waiting text={t.creation.checks.checking} /> : steps[0] === 'skipped' ? <Badge variant="secondary">{t.common.skipped}</Badge> : topic?.diverse ? <Badge className="bg-tint-amber text-tint-amber-fg">{t.creation.checks.severalTopics}</Badge> : steps[0] === 'done' ? <Badge className="bg-tint-green text-tint-green-fg">{t.creation.checks.oneTopic}</Badge> : null}>
          {topic?.diverse ? (
            <>
              <p className="text-sm text-muted-foreground">{t.creation.check.coversTopics}</p>
              <ul className="mt-2 list-disc space-y-0.5 pl-5 text-sm">{topic.topics.map((t) => <li key={t}>{t}</li>)}</ul>
              <Button asChild size="sm" variant="outline" className="mt-3"><Link to={`/knowledge/${page.id}`}>{t.creation.check.openToEdit}</Link></Button>
            </>
          ) : <p className="text-sm text-muted-foreground">{steps[0] === 'skipped' ? (skip.topic === 'rule' ? t.creation.check.topicRule : skip.topic === 'unavailable' ? t.creation.checks.noModel : t.creation.checks.skipped) : steps[0] === 'active' ? t.creation.check.looking : steps[0] === 'pending' ? t.creation.checks.waiting : t.creation.check.noProblem}</p>}
        </CheckCard>
        <CheckCard icon={FolderTree} tint="purple" title={t.creation.steps.group} status={steps[0] === 'active' ? <Waiting text={t.creation.checks.thinking} /> : null}>
          {steps[1] === 'done' && suggestion ? (
            <>
              <p className="text-sm">{t.creation.checks.suggestedGroup(<span className="rounded-full bg-tint-purple px-2.5 py-0.5 text-xs font-medium text-tint-purple-fg">{suggestion.kind === 'new' ? t.creation.newGroupName(suggestion.name) : suggestion.name}</span>)}</p>
              <p className="mt-1 text-xs text-muted-foreground">{t.creation.check.currentGroup(group?.name)}</p>
              <Button size="sm" className="mt-3" onClick={() => void acceptGroup()}>{t.creation.checks.accept}</Button>
            </>
          ) : <p className="text-sm text-muted-foreground">{steps[1] === 'skipped' ? (skip.group === 'picked' ? t.creation.check.groupPicked(group?.name ?? '') : skip.group === 'unavailable' ? t.creation.checks.noModel : t.creation.checks.skipped) : steps[1] === 'done' ? t.creation.check.noSuggestion : t.creation.checks.waiting}</p>}
        </CheckCard>
        <CheckCard icon={Copy} tint="blue" title={t.creation.similarPages} status={steps[2] === 'active' ? <Waiting text={t.creation.checks.searching} /> : null}>
          {steps[2] === 'pending' ? <p className="text-sm text-muted-foreground">{t.creation.checks.waiting}</p> : steps[2] === 'skipped' ? <p className="text-sm text-muted-foreground">{t.creation.checks.skipped}</p> : (
            <DuplicateMatches matches={matches} loading={steps[2] === 'active'} mergeLabel={t.creation.mergeInto} meaning={meaning} onMerge={(p) => mergeInto(p.id)} />
          )}
        </CheckCard>
      </div>
      <TermSuggestions className="mt-4" categoryId={page.categoryId} terms={terms.filter((t) => !resolvedTerms.has(t.term))} onResolved={(t) => setResolvedTerms((r) => new Set(r).add(t))} />
      <div className="mt-6 flex items-center justify-end gap-3">
        {page.checkStatus === 'checked' && <Badge className="bg-tint-green text-tint-green-fg"><CircleCheck /> {t.creation.check.alreadyChecked}</Badge>}
        <Button variant="ghost" asChild><Link to={`/knowledge/${page.id}`}>{t.creation.check.openPage} <ChevronRight /></Link></Button>
        <Button size="lg" disabled={!done || !checked} onClick={markChecked}>{t.creation.markAsChecked}</Button>
      </div>
      {done && !checked && <p className="mt-2 text-right text-xs text-muted-foreground">{t.creation.check.cantMark}</p>}
    </>
  )
}
