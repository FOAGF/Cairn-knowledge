// New task dialog (FR-TASK-UI-3). Fields follow requirement §4.1.
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { MarkdownField } from '@/components/editor/MarkdownField'
import { CategoryDot, TaskTypeIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { priorityLabel, PRIORITY_ORDER, typeLabel, TYPE_ORDER } from '@/lib/labels'
import { cn } from '@/lib/utils'
import { DatePicker, LabelInput } from './taskUi'
import { useCategoryList } from '@/api/categories'
import { type Priority, type TaskType, useCreateTask } from '@/api/tasks'

/** Values the form starts with (the assistant's *Edit…* on a proposed task). */
export interface NewTaskInitial {
  categoryId?: string
  title?: string
  description?: string
  type?: TaskType
  priority?: Priority
  labels?: string[]
  dueDate?: string
}

export function NewTaskDialog({
  open,
  onOpenChange,
  defaultCategoryId,
  initial,
  onCreated,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  defaultCategoryId?: string
  initial?: NewTaskInitial
  /** Called after the task was created, before the app goes to it. */
  onCreated?: () => void
}) {
  const navigate = useNavigate()
  const categories = useCategoryList()
  const { mutate: createTask, isPending } = useCreateTask()
  const active = categories.filter((p) => p.status === 'active')
  const general = categories.find((p) => p.isGeneral)
  // `t` is the new task below.
  const m = useT()

  const [categoryId, setCategoryId] = useState('')
  const [title, setTitle] = useState('')
  const [description, setDescription] = useState('')
  const [type, setType] = useState<TaskType>('general')
  const [priority, setPriority] = useState<Priority>('medium')
  const [labels, setLabels] = useState<string[]>([])
  const [dueDate, setDueDate] = useState<string | undefined>()

  // Reset every time the dialog opens.
  useEffect(() => {
    if (!open) return
    setCategoryId(initial?.categoryId ?? defaultCategoryId ?? general?.id ?? categories[0]?.id ?? '')
    setTitle(initial?.title ?? '')
    setDescription(initial?.description ?? '')
    setType(initial?.type ?? 'general')
    setPriority(initial?.priority ?? 'medium')
    setLabels(initial?.labels ?? [])
    setDueDate(initial?.dueDate)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  const submit = () => {
    if (!title.trim() || !categoryId || isPending) return
    createTask(
      { categoryId, title: title.trim(), type, description, priority, labels, dueDate: dueDate ?? null },
      {
        onSuccess: (t) => {
          toast.success(m.tasks.newTask.created(t.key))
          onCreated?.()
          onOpenChange(false)
          navigate(`/tasks/${t.key}`)
        },
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{m.common.newTask}</DialogTitle>
          <DialogDescription>{m.tasks.newTask.description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label>{m.common.category}</Label>
              <Select value={categoryId} onValueChange={setCategoryId}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={m.common.category} />
                </SelectTrigger>
                <SelectContent>
                  {active.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      <CategoryDot color={p.color} className="size-2" />
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>{m.common.priority}</Label>
              <Select value={priority} onValueChange={(v) => setPriority(v as Priority)}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PRIORITY_ORDER.map((p) => (
                    <SelectItem key={p} value={p}>
                      {priorityLabel(p)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="space-y-1.5">
            <Label>{m.common.type}</Label>
            <div role="radiogroup" aria-label={m.common.type} className="inline-flex gap-1 rounded-lg border p-1">
              {TYPE_ORDER.map((ty) => (
                <button
                  key={ty}
                  type="button"
                  role="radio"
                  aria-checked={type === ty}
                  onClick={() => setType(ty)}
                  className={cn(
                    'inline-flex items-center gap-1.5 rounded-md px-3 py-1 text-sm transition-colors',
                    type === ty ? 'bg-accent font-medium text-accent-foreground' : 'text-muted-foreground hover:bg-muted',
                  )}
                >
                  <TaskTypeIcon type={ty} />
                  {typeLabel(ty)}
                </button>
              ))}
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="new-task-title">{m.common.title}</Label>
            <Input
              id="new-task-title"
              autoFocus
              value={title}
              placeholder={m.tasks.newTask.titlePlaceholder}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && !e.nativeEvent.isComposing && submit()}
            />
          </div>
          <div className="space-y-1.5">
            <Label>{m.common.description}</Label>
            <div className="rounded-lg border px-3 py-2">
              <MarkdownField small value={description} onChange={setDescription} minHeight="6rem" placeholder={m.tasks.markdownHint} />
            </div>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-1.5">
              <Label>{m.common.labels}</Label>
              <LabelInput value={labels} onChange={setLabels} />
            </div>
            <div className="space-y-1.5">
              <Label>{m.common.dueDate}</Label>
              <DatePicker value={dueDate} onChange={setDueDate} className="w-full" />
            </div>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {m.common.cancel}
          </Button>
          <Button disabled={!title.trim() || isPending} onClick={submit}>
            {m.tasks.newTask.create}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
