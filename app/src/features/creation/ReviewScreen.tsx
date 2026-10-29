import { useSettings } from '@/api/settings'
// Review + save pipeline (§5.4), shared by every source (manual, file, task, assistant).
import { BookA, Copy, FolderTree, Layers, Loader2, Plus, Scissors, Sparkles, Square, Trash2, TriangleAlert, ScanSearch, WandSparkles } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { MarkdownField } from '@/components/editor/MarkdownField'
import { joinTitleLine, splitTitleLine } from '@/lib/titleLine'
import { PageHeader, SectionCard } from '@/components/shared/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { countWords } from '@/lib/format'
import { useAppStore } from '@/store'
import { type PageDraft, REVIEW_PATH } from '@/store/creation'
import { keepOneTopic, newStreamId } from '@/api/checks'
import { errorMessage } from '@/api/client'
import { cancelJob, clearStream, isCancelled, useLlmStream } from '@/api/llm'
import { TermSuggestions } from '@/features/glossary/TermSuggestions'
import { removeSection, splitSections } from './helpers'
import { CheckCard, CheckError, DuplicateMatches, Stepper, TagsInput, UnavailableNote, Waiting } from './parts'
import { usePipeline, type TopicResult } from './usePipeline'
import { useDuplicateMatches } from './useSimilar'
import { useGroups } from '@/api/knowledge'
import { SavedAs } from '@/components/shared/SavedAs'
import { groupOptions, groupPath } from '@/features/knowledge/groupTree'
import { useCategoryList } from '@/api/categories'
import { useT } from '@/i18n'

/** The duplicate check follows the text this long after the last edit (it uses no model). */
const LIVE_DUPLICATES_MS = 10_000

export function ReviewScreen() {
  const navigate = useNavigate()
  const draft = useAppStore((s) => s.draft)
  const setDraft = useAppStore((s) => s.setDraft)
  const queue = useAppStore((s) => s.draftQueue)
  const setQueue = useAppStore((s) => s.setDraftQueue)
  const saveDraftAsPage = useAppStore((s) => s.saveDraftAsPage)
  const setDraftPath = useAppStore((s) => s.setDraftPath)
  const categories = useCategoryList()
  const groups = useGroups()
  const settings = useSettings()
  const t = useT()

  // The "New group" dialog: null when closed, else the name it starts with.
  const [groupDialog, setGroupDialog] = useState<string | null>(null)
  const [keptSeparate, setKeptSeparate] = useState(false)
  const [diverse, setDiverse] = useState<TopicResult | null>(null)
  // FR-KN-DV-3: the topic being kept; the rewritten draft streams into the editor under `streamId`.
  const [rewriting, setRewriting] = useState<{ topic: string; streamId: string } | null>(null)
  const streamed = useLlmStream(rewriting?.streamId)
  const [resolvedTerms, setResolvedTerms] = useState<Set<string>>(new Set())
  const resolveDiverse = useRef<((a: 'changed' | void) => void) | null>(null)
  const saving = useRef(false)
  // The pending page (features/creation/pending.tsx) is kept in the data folder; its tab opens this screen.
  useEffect(() => {
    if (useAppStore.getState().draft) setDraftPath(REVIEW_PATH)
  }, [setDraftPath])

  const patch = (c: Partial<PageDraft>) => setDraft((d) => (d ? { ...d, ...c } : d))

  // Topic check can pause the pipeline until the user answers the dialog (FR-KN-DV-1).
  const pipeline = usePipeline({
    getSubject: () => {
      const d = useAppStore.getState().draft
      // A new group the user took counts as a chosen group (no suggestion needed).
      return d && { title: d.title, body: d.body, categoryId: d.categoryId, source: d.source, groupId: d.groupId ?? (d.newGroup ? 'new' : undefined), excludePageId: d.fromPageId }
    },
    onDiverse: (t) =>
      new Promise<'changed' | void>((res) => {
        resolveDiverse.current = res
        setDiverse(t)
      }),
  })
  const { steps, topic, report, suggestion, terms, duplicates, skip, unavailable, error, checked, running, rerun, refreshDuplicates, dupRefreshing, refreshTerms, termsChecking } = pipeline
  const closeDialog = (answer?: 'changed') => {
    setDiverse(null)
    resolveDiverse.current?.(answer)
    resolveDiverse.current = null
  }

  // FR-KN-DU-1: all categories, the draft's category first.
  const { matches, meaning } = useDuplicateMatches(duplicates)
  const newTerms = terms.filter((t) => !resolvedTerms.has(t.term))
  // The duplicate list follows the text: once the checks are done, LIVE_DUPLICATES_MS after the last edit.
  const live = !running && steps[2] === 'done' && !rewriting && !diverse
  useEffect(() => {
    if (!live) return
    const t = setTimeout(() => void refreshDuplicates(), LIVE_DUPLICATES_MS)
    return () => clearTimeout(t)
  }, [live, draft?.title, draft?.body, draft?.categoryId, refreshDuplicates])
  const sections = useMemo(() => splitSections(draft?.body ?? '').sections, [draft?.body])
  const categoryGroups = useMemo(() => groups.filter((g) => g.categoryId === draft?.categoryId), [groups, draft?.categoryId])

  if (!draft) return null
  // The draft keeps its whole body (the checks expect the `# title` line); the editor shows it without that line.
  const titled = splitTitleLine(draft.body, draft.title)
  const words = countWords(titled.rest)
  // Suggested tags are offered, never added by themselves: the chat card's and the check's.
  const tagIdeas = [...new Set([...(draft.suggestedTags ?? []), ...(report?.tags ?? [])])].filter((t) => !draft.tags.includes(t))

  // A new group shows in the list as "Name (new)" and is created when the page is saved.
  const groupValue = draft.newGroup ? '__new' : (draft.groupId ?? 'none')
  const pickGroup = (v: string) => {
    if (v === '__ask') setGroupDialog(draft.newGroup ?? '')
    else if (v !== '__new') patch({ groupId: v === 'none' ? undefined : v, newGroup: undefined })
  }
  const takeNewGroup = (name: string) => {
    const n = name.trim()
    if (!n) return
    // External mode names nested groups by their paths ("Parent / Child").
    const existing = categoryGroups.find((g) => groupPath(g, categoryGroups).toLowerCase() === n.toLowerCase())
    patch(existing ? { groupId: existing.id, newGroup: undefined } : { groupId: undefined, newGroup: n })
  }
  const lookForTerms = () =>
    void refreshTerms().then(
      (n) => n === 0 && toast(t.creation.review.noNewTerms),
      (e) => toast.error(errorMessage(e)),
    )

  // FR-KN-DV-3: rewrite the draft to one topic; the text streams into the editor, *Stop* keeps the draft as it was.
  async function keepOne(topicName: string) {
    const d = useAppStore.getState().draft
    if (!d || !topic) return
    const streamId = newStreamId()
    setRewriting({ topic: topicName, streamId })
    setDiverse(null)
    let changed = false
    try {
      const r = await keepOneTopic({ categoryId: d.categoryId, body: d.body, topic: topicName, topics: topic.topics, streamId })
      patch({ title: r.title, body: r.body, incomplete: r.incomplete })
      changed = true
      toast.success(t.creation.review.keptOneTopic(topicName))
    } catch (e) {
      if (!isCancelled(e)) toast.error(errorMessage(e))
    } finally {
      clearStream(streamId)
      setRewriting(null)
    }
    // Opened again from the card after the pipeline went on: run the checks on the new text.
    if (changed && !resolveDiverse.current) void rerun({ topicDone: true })
    closeDialog(changed ? 'changed' : undefined)
  }

  // FR-KN-DV-2: move a section to its own draft, with the same Source info.
  function moveToNewPage(heading: string, text: string) {
    const extra: PageDraft = { ...draft!, groupId: undefined, title: heading, body: `# ${heading}\n\n${text.replace(/^##\s+.+\n?/, '').trim()}`, tags: [...draft!.tags] }
    setQueue((q) => [...q, extra])
    patch({ body: removeSection(draft!.body, heading) })
    toast.success(t.creation.review.queued(heading))
  }

  async function save() {
    if (!draft!.title.trim()) return toast.error(t.creation.review.needTitle)
    if (saving.current) return
    saving.current = true
    try {
      // "Checked" only when every check ran (FR-KN-SV-3: skipped ones leave the page "not checked").
      const page = await saveDraftAsPage({ checkStatus: checked ? 'checked' : 'not_checked' })
      if (!page) return
      toast.success(t.creation.review.saved(page.title), checked ? undefined : { description: t.creation.review.savedUnchecked })
      setKeptSeparate(false)
      if (queue.length) {
        // Continue with the next queued draft.
        const [next, ...rest] = queue
        setDraft(next)
        setQueue(rest)
        setResolvedTerms(new Set())
        void rerun({ fresh: true })
      } else navigate(`/knowledge/${page.id}`)
    } finally {
      saving.current = false
    }
  }

  function discard() {
    setDraft(null)
    setQueue([])
    navigate('/knowledge/new')
  }

  const topicStatus = steps[0] === 'active' ? <Waiting text={t.creation.checks.checking} /> : steps[0] === 'skipped' ? <Badge variant="secondary">{skip.topic === 'rule' ? t.creation.checks.notNeeded : t.common.skipped}</Badge> : topic?.diverse ? <Badge className="bg-tint-amber text-tint-amber-fg">{t.creation.checks.severalTopics}</Badge> : steps[0] === 'done' ? <Badge className="bg-tint-green text-tint-green-fg">{t.creation.checks.oneTopic}</Badge> : null
  const stopRewriting = () => rewriting && void cancelJob(rewriting.streamId)

  return (
    <>
      <PageHeader
        icon={Sparkles}
        title={t.creation.review.title}
        description={t.creation.review.description(t.creation.review.source[draft.source], draft.fileName)}
        actions={
          <div className="flex items-center gap-3">
            {queue.length > 0 && <Badge className="bg-tint-purple text-tint-purple-fg">{t.creation.review.moreWaiting(queue.length)}</Badge>}
          </div>
        }
      />
      <div className="mb-5 space-y-3">
        <Stepper steps={steps} />
        {error && <CheckError message={error.message} onRetry={pipeline.retry} onSkip={pipeline.skipStep} />}
        {unavailable && <UnavailableNote message={unavailable} />}
      </div>

      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_23rem]">
        <SectionCard>
          <div className="space-y-4">
            <div>
              <Input
                value={draft.title}
                // The body's `# title` line follows the title field (lib/titleLine.ts).
                onChange={(e) => patch(titled.had ? { title: e.target.value, body: joinTitleLine(e.target.value, titled.rest) } : { title: e.target.value })}
                placeholder={t.creation.review.titlePlaceholder}
                className="h-11 border-0 bg-transparent px-0 text-2xl dark:bg-transparent font-semibold shadow-none focus-visible:ring-0 md:text-2xl"
              />
              <SavedAs title={draft.title} />
            </div>
            {draft.incomplete && (
              <p role="alert" className="flex items-center gap-2 rounded-xl bg-tint-amber px-3 py-2 text-sm text-tint-amber-fg">
                <TriangleAlert className="size-4 shrink-0" /> {t.creation.review.incomplete}
              </p>
            )}
            {rewriting && (
              <div role="status" className="flex items-center gap-3 rounded-xl bg-muted px-3 py-2 text-sm">
                <WandSparkles className="size-4 shrink-0 animate-pulse text-primary" />
                <span className="min-w-0 flex-1 truncate">{t.creation.review.rewriting(rewriting.topic)}</span>
                <Button size="xs" variant="outline" onClick={stopRewriting}><Square /> {t.common.stop}</Button>
              </div>
            )}
            <div className="min-h-64 rounded-xl border bg-background p-4">
              {rewriting ? (
                <pre className="min-h-60 font-sans text-sm whitespace-pre-wrap">{streamed || t.creation.review.waitingForModel}</pre>
              ) : (
                <MarkdownField value={titled.rest} onChange={(rest) => patch({ body: titled.had ? joinTitleLine(draft.title, rest) : rest })} minHeight="16rem" placeholder={t.creation.review.bodyPlaceholder} imageCategoryId={draft.categoryId} />
              )}
            </div>
            <p className="text-right text-xs text-muted-foreground">{t.creation.review.words(words)}</p>

            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-1.5">
                <Label>{t.common.category}</Label>
                <Select
                  value={draft.categoryId}
                  onValueChange={(v) => patch({ categoryId: v, groupId: undefined, newGroup: undefined })}
                >
                  <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {categories.filter((p) => p.status === 'active').map((p) => (
                      <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label>{t.common.group}</Label>
                <Select value={groupValue} onValueChange={pickGroup}>
                  <SelectTrigger id="group-field" className="w-full"><SelectValue placeholder={t.creation.review.noGroup} /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{t.creation.review.noGroup}</SelectItem>
                    {groupOptions(categoryGroups).map(({ group: g, label }) => (
                      <SelectItem key={g.id} value={g.id}>{label}</SelectItem>
                    ))}
                    {draft.newGroup && <SelectItem value="__new">{t.creation.newGroupName(draft.newGroup)}</SelectItem>}
                    <SelectItem value="__ask">{t.creation.review.newGroupItem}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5 sm:col-span-2">
                <Label>{t.common.tags}</Label>
                <TagsInput tags={draft.tags} onChange={(tags) => patch({ tags })} />
                {tagIdeas.length > 0 && (
                  <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
                    <span className="text-xs text-muted-foreground">{t.creation.review.suggestedTags}</span>
                    {tagIdeas.map((tag) => (
                      <button
                        key={tag}
                        type="button"
                        aria-label={t.creation.review.addTag(tag)}
                        onClick={() => patch({ tags: [...draft.tags, tag] })}
                        className="inline-flex items-center gap-0.5 rounded-full border border-dashed px-2 py-0.5 text-xs text-muted-foreground transition-colors hover:border-primary hover:text-foreground"
                      >
                        <Plus className="size-3" />
                        {tag}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              <div className="space-y-1.5 sm:col-span-2">
                <Label>{t.creation.review.sourceInfo}</Label>
                <Input value={draft.sourceInfo ?? ''} onChange={(e) => patch({ sourceInfo: e.target.value || undefined })} placeholder={t.creation.review.sourceInfoPlaceholder} />
              </div>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-3 border-t pt-4">
              <Button variant="ghost" className="text-muted-foreground" onClick={discard}>
                <Trash2 /> {t.creation.review.discardDraft}
              </Button>
              <div className="flex items-center gap-3">
                {!checked && !running && <span className="text-xs text-muted-foreground">{t.creation.review.willBeUnchecked}</span>}
                <Button size="lg" disabled={!!rewriting} onClick={() => void save()}>{t.creation.review.savePage}</Button>
              </div>
            </div>
          </div>
        </SectionCard>

        <div className="space-y-4">
          <CheckCard icon={ScanSearch} tint="amber" title={t.creation.review.topicCard} status={topicStatus}>
            {topic?.diverse ? (
              <>
                <p className="text-sm text-muted-foreground">
                  {t.creation.review.topicsFound(topic.topics.length)}
                </p>
                <Button size="sm" variant="outline" className="mt-2" onClick={() => setDiverse(topic)}>{t.creation.review.seeTopics}</Button>
              </>
            ) : (
              <p className="text-sm text-muted-foreground">
                {steps[0] === 'active' ? t.creation.review.topicChecking : steps[0] === 'skipped' ? (skip.topic === 'rule' ? t.creation.review.topicRule(settings.wordWarningThreshold) : skip.topic === 'unavailable' ? t.creation.checks.noModel : t.creation.checks.skipped) : steps[0] === 'pending' ? t.creation.review.waiting : t.creation.review.oneTopic}
              </p>
            )}
          </CheckCard>

          <CheckCard icon={FolderTree} tint="purple" title={t.creation.review.groupCard} status={steps[1] === 'active' ? <Waiting text={t.creation.checks.thinking} /> : steps[1] === 'skipped' ? <Badge variant="secondary">{t.common.skipped}</Badge> : null}>
            {steps[1] === 'done' && suggestion ? (
              <div className="space-y-2">
                <p className="text-sm">
                  {t.creation.checks.suggestedGroup(<span className="rounded-full bg-tint-purple px-2.5 py-0.5 text-xs font-medium text-tint-purple-fg">{suggestion.kind === 'new' ? t.creation.newGroupName(suggestion.name) : suggestion.name}</span>)}
                </p>
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" onClick={() => (suggestion.kind === 'existing' ? pickGroup(suggestion.groupId) : takeNewGroup(suggestion.name))}>{t.creation.checks.accept}</Button>
                  <Button size="sm" variant="outline" onClick={() => document.getElementById('group-field')?.click()}>{t.common.change}</Button>
                  <Button size="sm" variant="outline" onClick={() => setGroupDialog('')}>{t.creation.review.newGroup}</Button>
                </div>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{steps[1] === 'skipped' ? (skip.group === 'picked' ? t.creation.review.groupPicked : skip.group === 'unavailable' ? t.creation.review.groupNoModel : t.creation.review.groupSkipped) : steps[1] === 'done' ? t.creation.review.groupNone : t.creation.review.groupWaiting}</p>
            )}
          </CheckCard>

          <CheckCard icon={Copy} tint="blue" title={t.creation.review.duplicateCard} status={steps[2] === 'active' ? <Waiting text={t.creation.checks.searching} /> : dupRefreshing ? <Waiting text={t.creation.review.updating} /> : null}>
            {steps[2] === 'pending' ? (
              <p className="text-sm text-muted-foreground">{t.creation.review.duplicatesPending}</p>
            ) : steps[2] === 'skipped' ? (
              <p className="text-sm text-muted-foreground">{t.creation.checks.skipped}</p>
            ) : keptSeparate ? (
              <div className="text-sm text-muted-foreground">
                {t.creation.review.keptSeparate(<button className="text-primary hover:underline" onClick={() => setKeptSeparate(false)}>{t.creation.review.showMatches}</button>)}
              </div>
            ) : (
              <DuplicateMatches
                matches={matches}
                loading={steps[2] === 'active'}
                meaning={meaning}
                onMerge={(p) => navigate(`/knowledge/merge/${p.id}`)}
                footer={matches.length > 0 && (
                  <Button variant="secondary" size="sm" className="w-full" onClick={() => setKeptSeparate(true)}>{t.creation.review.keepSeparate}</Button>
                )}
              />
            )}
          </CheckCard>

          {sections.length >= 2 && (
            <CheckCard icon={Scissors} tint="green" title={t.creation.review.split}>
              <p className="mb-2 text-xs text-muted-foreground">{t.creation.review.splitHint}</p>
              <ul className="space-y-1.5">
                {sections.map((s) => (
                  <li key={s.heading} className="flex items-center justify-between gap-2 rounded-lg bg-muted/50 px-3 py-1.5">
                    <span className="truncate text-sm">{s.heading}</span>
                    <Button size="xs" variant="outline" onClick={() => moveToNewPage(s.heading, s.text)}>{t.creation.review.moveToNewPage}</Button>
                  </li>
                ))}
              </ul>
            </CheckCard>
          )}
          <TermSuggestions categoryId={draft.categoryId} terms={newTerms} onResolved={(t) => setResolvedTerms((r) => new Set(r).add(t))} />
          {/* Glossary terms are not looked for again by themselves: this asks the model once. */}
          {!running && !unavailable && (
            <Button variant="outline" size="sm" className="w-full" disabled={termsChecking} onClick={lookForTerms}>
              {termsChecking ? <Loader2 className="animate-spin" /> : <BookA />}
              {termsChecking ? t.creation.review.lookingForTerms : t.creation.review.lookForTerms}
            </Button>
          )}
          {running ? (
            steps[3] === 'pending' && <p className="text-center text-xs text-muted-foreground">{t.creation.review.running}</p>
          ) : (
            <Button variant="ghost" size="sm" className="w-full text-muted-foreground" onClick={() => void rerun()}>{t.creation.review.rerun}</Button>
          )}
        </div>
      </div>

      <DiverseDialog topic={diverse} onKeepOne={(t) => void keepOne(t)} onEdit={() => closeDialog()} onCancel={() => { closeDialog(); discard() }} />
      <NewGroupDialog key={groupDialog ?? 'closed'} initial={groupDialog} onClose={() => setGroupDialog(null)} onSave={takeNewGroup} />
    </>
  )
}

/** Names a new group for the page; it is created when the page is saved. */
function NewGroupDialog({ initial, onClose, onSave }: { initial: string | null; onClose: () => void; onSave: (name: string) => void }) {
  const [name, setName] = useState(initial ?? '')
  const t = useT()
  return (
    <Dialog open={initial !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-sm">
        <DialogHeader>
          <DialogTitle>{t.creation.review.newGroup}</DialogTitle>
          <DialogDescription>{t.creation.newGroupDialog.description}</DialogDescription>
        </DialogHeader>
        <form
          className="contents"
          onSubmit={(e) => {
            e.preventDefault()
            if (!name.trim()) return
            onSave(name)
            onClose()
          }}
        >
          <Input autoFocus aria-label={t.creation.newGroupDialog.name} placeholder={t.creation.newGroupDialog.name} value={name} onChange={(e) => setName(e.target.value)} />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>{t.common.cancel}</Button>
            <Button type="submit" disabled={!name.trim()}>{t.creation.newGroupDialog.use}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** §5.5 diverse-content dialog. */
function DiverseDialog({
  topic,
  onKeepOne,
  onEdit,
  onCancel,
}: {
  topic: TopicResult | null
  onKeepOne: (h: string) => void
  onEdit: () => void
  onCancel: () => void
}) {
  const t = useT()
  return (
    <Dialog open={!!topic} onOpenChange={(o) => !o && onEdit()}>
      <DialogContent className="sm:max-w-lg" showCloseButton={false}>
        <DialogHeader>
          <div className="mb-1 flex size-10 items-center justify-center rounded-xl bg-tint-amber text-tint-amber-fg">
            <TriangleAlert className="size-5" />
          </div>
          <DialogTitle>{t.creation.diverse.title}</DialogTitle>
          <DialogDescription>
            {t.creation.diverse.description(topic?.words, topic?.topics.length)}
          </DialogDescription>
        </DialogHeader>
        {topic && topic.topics.length > 0 && (
          <div className="space-y-1.5">
            <p className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground"><Layers className="size-3.5" /> {t.creation.diverse.detected}</p>
            {topic.topics.map((name) => (
              <button
                key={name}
                type="button"
                onClick={() => onKeepOne(name)}
                className="flex w-full items-center justify-between gap-2 rounded-xl border bg-background px-3 py-2 text-left text-sm hover:border-primary hover:bg-muted"
              >
                <span className="truncate">{name}</span>
                <span className="shrink-0 text-xs text-primary">{t.creation.diverse.keepOne}</span>
              </button>
            ))}
          </div>
        )}
        <DialogFooter className="sm:justify-between">
          <Button variant="ghost" onClick={onCancel}>{t.common.cancel}</Button>
          <Button onClick={onEdit}>{t.creation.diverse.editMyself}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
