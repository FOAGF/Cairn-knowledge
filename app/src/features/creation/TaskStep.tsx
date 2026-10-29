import { useModelList } from '@/api/providers'
import { useSettings } from '@/api/settings'
// Create knowledge from a task (§4.6, FR-TASK-KN-2..4).
import { format, parseISO } from 'date-fns'
import { formatDate } from '@/lib/format'
import { ClipboardList, FilePlus2, FileText, Loader2, Search, Square } from 'lucide-react'
import { toast } from 'sonner'
import { useMemo, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { EmptyState, PageHeader, SectionCard } from '@/components/shared/PageHeader'
import { CategoryBadge, StatusBadge } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { type PageDraft, REVIEW_PATH } from '@/store/creation'
import { PagePickerDialog } from './parts'
import { askBeforeReplacing, pendingPath, startPendingPage } from './pending'
import { useCategoryList } from '@/api/categories'
import { draftFromTask } from '@/api/creation'
import { errorMessage } from '@/api/client'
import { cancelJob, clearStream, isCancelled, isLlmUnavailable, newStreamId, useLlmStream } from '@/api/llm'
import { ALL_TASKS, useTask, useTaskComments, useTaskList } from '@/api/tasks'
import { useT } from '@/i18n'

/** Task picker shown for "From a task". */
export function TaskPicker() {
  const navigate = useNavigate()
  const tasks = useTaskList(ALL_TASKS)
  const categories = useCategoryList()
  const [q, setQ] = useState('')
  const t = useT()
  const list = useMemo(() => tasks.filter((t) => `${t.key} ${t.title}`.toLowerCase().includes(q.toLowerCase())), [tasks, q])
  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader icon={ClipboardList} tint="green" title={t.creation.task.pickerTitle} description={t.creation.task.pickerDescription} />
      <SectionCard>
        <div className="relative mb-3">
          <Search className="absolute top-2.5 left-3 size-4 text-muted-foreground" />
          <Input className="pl-9" placeholder={t.creation.task.search} value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <div className="max-h-[28rem] space-y-1 overflow-y-auto">
          {list.map((t) => (
            <button key={t.id} type="button" onClick={() => navigate(`/knowledge/new?fromTask=${t.key}`)} className="flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left hover:bg-muted">
              <span className="w-16 shrink-0 text-xs font-medium text-muted-foreground">{t.key}</span>
              <span className="min-w-0 flex-1 truncate text-sm font-medium">{t.title}</span>
              <CategoryBadge category={categories.find((p) => p.id === t.categoryId)} className="hidden sm:inline-flex" />
              <StatusBadge status={t.status} />
            </button>
          ))}
          {!list.length && <p className="py-6 text-center text-sm text-muted-foreground">{t.creation.task.noTasks}</p>}
        </div>
        <div className="mt-4 flex justify-end"><Button variant="ghost" onClick={() => navigate('/knowledge/new')}>{t.common.back}</Button></div>
      </SectionCard>
    </div>
  )
}

export function TaskStep({ taskKey }: { taskKey: string }) {
  const navigate = useNavigate()
  const [, setParams] = useSearchParams()
  const { data: task, isPending } = useTask(taskKey)
  const allComments = useTaskComments(task?.id)
  const categories = useCategoryList()
  const models = useModelList()
  const settings = useSettings()
  const comments = useMemo(() => [...allComments].sort((a, b) => a.createdAt.localeCompare(b.createdAt)), [allComments])

  const [useDesc, setUseDesc] = useState(true)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [target, setTarget] = useState<'new' | 'existing'>('new')
  const [chosenCategoryId, setCategoryId] = useState('')
  const categoryId = chosenCategoryId || task?.categoryId || ''
  const [pickerOpen, setPickerOpen] = useState(false)
  const [pickAsIs, setPickAsIs] = useState(false)
  const [streamId, setStreamId] = useState<string | null>(null)
  const busy = streamId !== null
  const streamed = useLlmStream(streamId)
  // No default model, or the model said so: only the plain copy is offered (FR-TASK-KN-2).
  const [noModel, setNoModel] = useState(false)
  const t = useT()

  if (isPending) return null
  if (!task) return <EmptyState icon={ClipboardList} title={t.creation.task.notFound(taskKey)}><Button variant="link" onClick={() => setParams({})}>{t.creation.task.backToChooser}</Button></EmptyState>

  // The default model's name; the messages say "the default model" without one.
  const modelName = models.find((m) => m.id === settings.defaultModelId)?.label
  const nothing = !useDesc && picked.size === 0
  const llmOff = noModel || !settings.defaultModelId

  // The task's labels; the check in the review step suggests tags when there are none (FR-KN-SV-1).
  const tagsFor = () => task!.labels.slice(0, 4)
  const draftBase = { source: 'task' as const, sourceInfo: `From task ${task!.key}: ${task!.title}`, originTaskId: task!.id } // i18n-ignore: the page's source info is data

  // "Use the text as is": the selected content is copied into the draft; the task keeps it.
  function plainDraft(): PageDraft {
    const parts: string[] = []
    if (useDesc && task!.description.trim()) parts.push(task!.description.trim())
    // The page's text is data, so it stays English (NFR-I18N-2).
    for (const c of comments.filter((c) => picked.has(c.id))) parts.push(`**Comment, ${format(parseISO(c.createdAt), 'd MMM yyyy')}:** ${c.text}`)
    const body = `# ${task!.title}\n\n${parts.join('\n\n')}`
    return { ...draftBase, categoryId, title: task!.title, body, tags: tagsFor() }
  }

  // The default: the model writes the page from the chosen description and comments, streamed.
  async function llmDraft(): Promise<PageDraft | null> {
    const id = newStreamId()
    setStreamId(id)
    try {
      const out = await draftFromTask({ taskId: task!.id, includeDescription: useDesc, commentIds: [...picked], streamId: id })
      return { ...draftBase, categoryId, title: out.title, body: out.body, tags: tagsFor(), incomplete: out.incomplete || undefined }
    } catch (e) {
      if (isCancelled(e)) return null
      if (isLlmUnavailable(e)) {
        setNoModel(true)
        toast.error(t.creation.task.noModel(errorMessage(e)))
      } else toast.error(errorMessage(e))
      return null
    } finally {
      clearStream(id)
      setStreamId(null)
    }
  }

  async function go(page?: { id: string }, asIs = false) {
    // Asked before drafting, so the model isn't called for a page that won't be kept.
    const choice = await askBeforeReplacing()
    if (choice === 'continue') return navigate(pendingPath() ?? REVIEW_PATH)
    if (choice === 'cancel') return
    const draft = asIs ? plainDraft() : await llmDraft()
    if (!draft) return
    const path = page ? `/knowledge/merge/${page.id}` : REVIEW_PATH
    startPendingPage({ ...draft, categoryId: page ? task!.categoryId : categoryId }, path)
    navigate(path)
  }

  return (
    <div className="mx-auto max-w-3xl">
      <PageHeader icon={ClipboardList} tint="green" title={t.creation.task.title} description={t.creation.task.description(task.key, task.title)} />
      <div className="space-y-5">
        <SectionCard title={t.creation.task.include} description={t.creation.task.includeHint}>
          <div className="space-y-3">
            <label className={cn('flex cursor-pointer gap-3 rounded-xl border p-3', useDesc && 'border-primary/40 bg-tint-blue/30')}>
              <Checkbox checked={useDesc} onCheckedChange={(v) => setUseDesc(!!v)} className="mt-1" />
              <div className="min-w-0 flex-1"><p className="mb-1 text-sm font-medium">{t.common.description}</p>
                {task.description.trim() ? <MarkdownView text={task.description} className="text-sm" /> : <p className="text-sm text-muted-foreground">{t.creation.task.noDescription}</p>}</div>
            </label>
            {comments.map((c) => (
              <label key={c.id} className={cn('flex cursor-pointer gap-3 rounded-xl border p-3', picked.has(c.id) && 'border-primary/40 bg-tint-blue/30')}>
                <Checkbox className="mt-1" checked={picked.has(c.id)} onCheckedChange={(v) => setPicked((s) => { const n = new Set(s); if (v) n.add(c.id); else n.delete(c.id); return n })} />
                <div className="min-w-0 flex-1"><p className="mb-1 text-xs text-muted-foreground">{t.creation.task.comment(formatDate(c.createdAt, 'date'))}</p><MarkdownView text={c.text} className="text-sm" /></div>
              </label>
            ))}
            {!comments.length && <p className="text-sm text-muted-foreground">{t.creation.task.noComments}</p>}
          </div>
        </SectionCard>

        <SectionCard title={t.creation.task.where}>
          <RadioGroup value={target} onValueChange={(v) => setTarget(v as 'new' | 'existing')} className="gap-3">
            <label className={cn('flex cursor-pointer gap-3 rounded-xl border p-3', target === 'new' && 'border-primary/40 bg-tint-blue/30')}>
              <RadioGroupItem value="new" className="mt-1" />
              <div className="flex-1"><p className="flex items-center gap-2 text-sm font-medium"><FilePlus2 className="size-4" /> {t.creation.task.newPage}</p>
                <div className="mt-2 flex items-center gap-2"><Label className="text-xs text-muted-foreground">{t.common.category}</Label>
                  <Select value={categoryId} onValueChange={setCategoryId}>
                    <SelectTrigger className="w-56" size="sm"><SelectValue /></SelectTrigger>
                    <SelectContent>{categories.filter((p) => p.status === 'active').map((p) => <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>)}</SelectContent>
                  </Select></div></div>
            </label>
            <label className={cn('flex cursor-pointer gap-3 rounded-xl border p-3', target === 'existing' && 'border-primary/40 bg-tint-blue/30')}>
              <RadioGroupItem value="existing" className="mt-1" />
              <div><p className="flex items-center gap-2 text-sm font-medium"><FileText className="size-4" /> {t.creation.task.existing}</p>
                <p className="text-xs text-muted-foreground">{t.creation.task.existingHint}</p></div>
            </label>
          </RadioGroup>
        </SectionCard>

        {busy && (
          <SectionCard title={t.creation.task.drafting} description={t.creation.task.writing(modelName)}>
            <MarkdownView text={streamed || '…'} className="text-sm" />
          </SectionCard>
        )}

        <div className="flex flex-wrap items-center justify-end gap-3">
          {llmOff && !busy && <span className="mr-auto text-xs text-muted-foreground">{t.creation.task.noAi}</span>}
          {busy && <span className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" /> {t.creation.task.draftingWith(modelName)}</span>}
          {busy ? <Button variant="outline" onClick={() => void cancelJob(streamId)}><Square /> {t.common.stop}</Button> : <Button variant="ghost" onClick={() => setParams({})}>{t.common.back}</Button>}
          {target === 'new' ? (
            <>
              {!llmOff && <Button variant="outline" disabled={nothing || busy} onClick={() => void go(undefined, true)}>{t.creation.task.asIs}</Button>}
              <Button disabled={nothing || busy} onClick={() => void go(undefined, llmOff)}>{llmOff ? t.creation.task.createDraftAsIs : t.creation.task.createDraft}</Button>
            </>
          ) : (
            <>
              {!llmOff && <Button variant="outline" disabled={nothing || busy} onClick={() => { setPickAsIs(true); setPickerOpen(true) }}>{t.creation.task.asIsPick}</Button>}
              <Button disabled={nothing || busy} onClick={() => { setPickAsIs(false); setPickerOpen(true) }}>{t.creation.task.choosePage}</Button>
            </>
          )}
        </div>
      </div>
      <PagePickerDialog open={pickerOpen} onClose={() => setPickerOpen(false)} onPick={(p) => { setPickerOpen(false); void go(p, llmOff || pickAsIs) }} />
    </div>
  )
}
