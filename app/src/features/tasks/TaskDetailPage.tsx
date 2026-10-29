import { ArrowLeft, BookPlus, Pencil, SearchX, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { toast } from 'sonner'
import { useDraft } from '@/api/drafts'
import { DraftRestorePrompt, draftText } from '@/components/editor/DraftRestorePrompt'
import { MarkdownField } from '@/components/editor/MarkdownField'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { EmptyState, SectionCard } from '@/components/shared/PageHeader'
import { StatusDropdown, useChangeTask } from '@/components/shared/task-status'
import { PriorityIcon, CategoryDot, TaskTypeIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useT } from '@/i18n'
import { fullDate } from '@/lib/format'
import { priorityLabel, PRIORITY_ORDER, TYPE_ORDER } from '@/lib/labels'
import { errorMessage, isApiError } from '@/api/client'
import { primeTask, type Priority, type Task, type TaskType, useDeleteTask, useMoveTask, useTask, useTaskComments, useTaskVersions } from '@/api/tasks'
import { CompletionSummary } from './CompletionSummary'
import { TaskComments } from './TaskComments'
import { TaskHistory } from './TaskHistory'
import { LinkedPagesPanel, MentionedInPanel, TaskLinksPanel } from './TaskLinksPanel'
import { DatePicker, LabelInput } from './taskUi'
import { useCategoryList } from '@/api/categories'

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_1fr] items-start gap-3">
      <span className="pt-1.5 text-sm text-muted-foreground">{label}</span>
      <div className="min-w-0">{children}</div>
    </div>
  )
}

function TitleEditor({ task }: { task: Task }) {
  const change = useChangeTask()
  const [title, setTitle] = useState(task.title)
  const m = useT()
  useEffect(() => setTitle(task.title), [task.title])
  const commit = () => {
    const t = title.trim()
    if (!t) setTitle(task.title)
    else if (t !== task.title) change(task, { title: t })
  }
  // FR-TASK-UI-3: editable title
  return (
    <input
      value={title}
      aria-label={m.tasks.detail.titleLabel}
      onChange={(e) => setTitle(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && !e.nativeEvent.isComposing) (e.target as HTMLInputElement).blur()
        if (e.key === 'Escape') setTitle(task.title)
      }}
      className="w-full rounded-lg border border-transparent bg-transparent px-2 py-1 -mx-2 text-2xl font-semibold tracking-tight outline-none hover:border-border focus:border-ring focus:ring-3 focus:ring-ring/50"
    />
  )
}

function Description({ task }: { task: Task }) {
  const change = useChangeTask()
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(task.description)
  const saved = useDraft('task_description', task.id, editing)
  const pendingText = saved.pending ? draftText(saved.pending) : null
  const t = useT()
  const edit = (text: string) => {
    setDraft(text)
    saved.update(JSON.stringify({ text }))
  }
  return (
    <SectionCard
      title={t.common.description}
      action={
        !editing && (
          <Button variant="outline" size="sm" onClick={() => { setDraft(task.description); setEditing(true) }}>
            <Pencil /> {t.common.edit}
          </Button>
        )
      }
    >
      {editing ? (
        <div className="space-y-3">
          {saved.pending && pendingText !== null && pendingText !== task.description && (
            <DraftRestorePrompt draft={saved.pending} onRestore={() => { setDraft(pendingText); void saved.discard() }} onDiscard={() => void saved.discard()} />
          )}
          <div className="rounded-lg border px-3 py-2">
            <MarkdownField small value={draft} onChange={edit} autoFocus minHeight="8rem" placeholder={t.tasks.markdownHint} />
          </div>
          <div className="flex justify-end gap-2">
            <Button variant="outline" size="sm" onClick={() => { void saved.discard(); setEditing(false) }}>
              {t.common.cancel}
            </Button>
            <Button size="sm" onClick={() => { change(task, { description: draft }); void saved.discard(); setEditing(false) }}>
              {t.common.save}
            </Button>
          </div>
        </div>
      ) : task.description.trim() ? (
        <MarkdownView text={task.description} className="text-sm [&>:last-child]:mb-0" />
      ) : (
        <p className="text-sm text-muted-foreground">{t.tasks.detail.noDescription}</p>
      )}
    </SectionCard>
  )
}

function Sidebar({ task }: { task: Task }) {
  const categories = useCategoryList()
  const change = useChangeTask()
  const move = useMoveTask()
  const navigate = useNavigate()
  const m = useT()
  return (
    <SectionCard title={m.tasks.detail.details}>
      <div className="space-y-3">
        <Field label={m.common.type}>
          <Select value={task.type} onValueChange={(v) => change(task, { type: v as TaskType })}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {TYPE_ORDER.map((ty) => (
                <SelectItem key={ty} value={ty}>
                  <TaskTypeIcon type={ty} withLabel />
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <Field label={m.common.status}>
          <StatusDropdown task={task} className="h-8" />
        </Field>
        <Field label={m.common.priority}>
          <Select value={task.priority} onValueChange={(v) => change(task, { priority: v as Priority })}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PRIORITY_ORDER.map((p) => (
                <SelectItem key={p} value={p}>
                  <PriorityIcon priority={p} />
                  {priorityLabel(p)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <Field label={m.common.category}>
          {/* While a move runs, show the destination, so the select doesn't jump back and forth. */}
          <Select value={move.isPending ? (move.variables?.categoryId ?? task.categoryId) : task.categoryId} onValueChange={(v) => {
              if (v !== task.categoryId) move.mutate({ id: task.id, categoryId: v }, { onSuccess: (t) => navigate(`/tasks/${t.key}`, { replace: true }) })
            }}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {categories.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  <CategoryDot color={p.color} className="size-2" />
                  {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <Field label={m.common.dueDate}>
          <DatePicker value={task.dueDate ?? undefined} onChange={(d) => change(task, { dueDate: d ?? '' })} className="w-full" />
        </Field>
        <Field label={m.common.labels}>
          <LabelInput value={task.labels} onChange={(labels) => change(task, { labels })} />
        </Field>
        <div className="space-y-1 border-t pt-3 text-xs text-muted-foreground">
          <p>{m.tasks.detail.created(fullDate(task.createdAt))}</p>
          {task.completedAt && <p>{m.tasks.detail.completed(fullDate(task.completedAt))}</p>}
        </div>
      </div>
    </SectionCard>
  )
}

export default function TaskDetailPage() {
  const { taskKey } = useParams()
  const navigate = useNavigate()
  const { data: task, error, isPending } = useTask(taskKey)
  const categories = useCategoryList()
  const commentCount = useTaskComments(task?.id).length
  const versionCount = useTaskVersions(task?.id).length
  const deleteTask = useDeleteTask()
  const [confirmDelete, setConfirmDelete] = useState(false)
  const t = useT()
  const d = t.tasks.detail

  // An old key (after a move) resolves to the task; show it under its current key (FR-TASK-UI-6).
  useEffect(() => {
    if (task && taskKey && task.key !== taskKey) {
      primeTask(task) // the new key opens with this fresh copy, not an older cached one
      navigate(`/tasks/${task.key}`, { replace: true })
    }
  }, [task, taskKey, navigate])

  const back = (
    <Link to="/tasks" className="mb-4 inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
      <ArrowLeft className="size-4" /> {t.common.nav.tasks}
    </Link>
  )

  if (!task) {
    if (isPending && !error) return <div>{back}</div>
    const notFound = isApiError(error) && error.kind === 'not_found'
    return (
      <div>
        {back}
        <EmptyState icon={SearchX} title={notFound ? d.notFound(taskKey ?? '') : d.loadFailed}>
          {notFound ? d.maybeDeleted : `${errorMessage(error)} `}
          <Link to="/tasks" className="text-primary">{d.backToAll}</Link>
        </EmptyState>
      </div>
    )
  }

  const category = categories.find((p) => p.id === task.categoryId)

  return (
    <div>
      {back}
      <div className="mb-6 flex flex-wrap items-start justify-between gap-4">
        <div className="min-w-0 flex-1">
          <div className="mb-1 flex items-center gap-2.5 text-sm">
            <TaskTypeIcon type={task.type} />
            <span className="font-mono text-muted-foreground">{task.key}</span>
            <StatusDropdown task={task} />
            {category && (
              <span className="inline-flex items-center gap-1.5 text-muted-foreground">
                <CategoryDot color={category.color} className="size-2" />
                {category.name}
              </span>
            )}
          </div>
          <TitleEditor task={task} />
        </div>
        <div className="flex items-center gap-2">
          {/* FR-TASK-KN-2: available on any task */}
          <Button variant="outline" onClick={() => navigate('/knowledge/new?fromTask=' + task.key)}>
            <BookPlus /> {d.createKnowledge}
          </Button>
          <Button variant="destructive" size="icon" aria-label={d.deleteTask} onClick={() => setConfirmDelete(true)}>
            <Trash2 />
          </Button>
        </div>
      </div>

      <div className="grid gap-6 lg:grid-cols-[1fr_360px]">
        <div className="min-w-0 space-y-6">
          <Description task={task} />
          <section className="rounded-2xl border bg-card p-5 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
            <Tabs defaultValue="progress">
              <TabsList>
                <TabsTrigger value="progress">{d.updates(commentCount)}</TabsTrigger>
                <TabsTrigger value="history">{d.history(versionCount)}</TabsTrigger>
              </TabsList>
              <TabsContent value="progress" className="pt-3">
                <TaskComments task={task} />
              </TabsContent>
              <TabsContent value="history" className="pt-3">
                <TaskHistory taskId={task.id} />
              </TabsContent>
            </Tabs>
          </section>
        </div>

        <div className="min-w-0 space-y-6">
          <CompletionSummary taskId={task.id} />
          <Sidebar task={task} />
          <TaskLinksPanel task={task} />
          <LinkedPagesPanel task={task} />
          <MentionedInPanel task={task} />
        </div>
      </div>

      {/* FR-TASK-UI-3: deleting asks for confirmation */}
      <Dialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{d.deleteTitle(task.key)}</DialogTitle>
            <DialogDescription>{d.deleteDescription(task.title)}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmDelete(false)}>
              {t.common.cancel}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                deleteTask.mutate(task.id, {
                  onSuccess: () => {
                    setConfirmDelete(false)
                    toast.success(d.deleted(task.key))
                    navigate('/tasks')
                  },
                })
              }}
            >
              {d.deleteTask}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
