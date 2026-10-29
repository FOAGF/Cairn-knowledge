// Progress timeline (FR-TASK-CM-1): comments as progress updates on a vertical timeline,
// with status changes (from task history), Created and Completed events.
import { CheckCircle2, Pencil, Plus, Trash2 } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useDraft } from '@/api/drafts'
import { DraftRestorePrompt, draftText } from '@/components/editor/DraftRestorePrompt'
import { MarkdownField } from '@/components/editor/MarkdownField'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useT } from '@/i18n'
import { dateTime, fullDate, timeAgo } from '@/lib/format'
import { statusLabel, STATUS_TINT, TINT_CLASS, TINT_TEXT } from '@/lib/labels'
import { type Task, type TaskComment, type TaskStatus, useAddTaskComment, useDeleteTaskComment, useTaskComments, useTaskVersions, useUpdateTaskComment } from '@/api/tasks'
import { cn } from '@/lib/utils'

function CommentItem({ c }: { c: TaskComment }) {
  const updateComment = useUpdateTaskComment()
  const deleteComment = useDeleteTaskComment()
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(c.text)
  const saved = useDraft('comment', c.id, editing)
  const pendingText = saved.pending ? draftText(saved.pending) : null
  const t = useT()
  const edit = (text: string) => {
    setDraft(text)
    saved.update(JSON.stringify({ text }))
  }

  return (
    <div className="group rounded-xl border bg-background p-4">
      <div className="mb-2 flex items-center gap-2 text-xs text-muted-foreground">
        <span className="font-medium text-foreground">{dateTime(c.createdAt)}</span>
        <span>· {timeAgo(c.createdAt)}</span>
        {c.updatedAt && <span title={dateTime(c.updatedAt)}>{t.tasks.comments.edited}</span>}
        {!editing && (
          <span className="ml-auto flex gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <Button variant="ghost" size="icon-xs" aria-label={t.tasks.comments.edit} onClick={() => { setDraft(c.text); setEditing(true) }}>
              <Pencil />
            </Button>
            <Button variant="ghost" size="icon-xs" aria-label={t.tasks.comments.delete} onClick={() => deleteComment.mutate(c.id)}>
              <Trash2 />
            </Button>
          </span>
        )}
      </div>
      {editing ? (
        <div className="space-y-2">
          {saved.pending && pendingText !== null && pendingText !== c.text && (
            <DraftRestorePrompt draft={saved.pending} onRestore={() => { setDraft(pendingText); void saved.discard() }} onDiscard={() => void saved.discard()} />
          )}
          <div className="rounded-lg border px-3 py-2">
            <MarkdownField small value={draft} onChange={edit} autoFocus minHeight="4rem" />
          </div>
          <div className="flex justify-end gap-2">
            <Button variant="outline" size="sm" onClick={() => { void saved.discard(); setEditing(false) }}>
              {t.common.cancel}
            </Button>
            <Button
              size="sm"
              disabled={!draft.trim()}
              onClick={() => {
                updateComment.mutate({ id: c.id, text: draft.trim() }, { onSuccess: () => { void saved.discard(); setEditing(false) } })
              }}
            >
              {t.common.save}
            </Button>
          </div>
        </div>
      ) : (
        <MarkdownView text={c.text} className="text-sm [&>:last-child]:mb-0" />
      )}
    </div>
  )
}

type TimelineItem =
  | { kind: 'comment'; at: string; c: TaskComment }
  | { kind: 'created' | 'completed'; at: string }
  | { kind: 'status'; at: string; from: TaskStatus; to: TaskStatus }

function Node({ children, className }: { children?: React.ReactNode; className?: string }) {
  return <span className={cn('absolute top-1.5 -left-8 flex size-4 items-center justify-center rounded-full ring-4 ring-card', className)}>{children}</span>
}

function EventRow({ item }: { item: Exclude<TimelineItem, { kind: 'comment' }> }) {
  const t = useT()
  let label: React.ReactNode
  let dot = ''
  if (item.kind === 'created') label = t.tasks.comments.created
  else if (item.kind === 'completed') {
    dot = 'bg-tint-green text-tint-green-fg'
    label = <span className={cn('font-medium', TINT_TEXT.green)}>{t.tasks.comments.completed}</span>
  } else if (item.kind === 'status') {
    label = t.tasks.comments.statusTo(
      <span className={cn('rounded-full px-2 py-px text-xs font-medium', TINT_CLASS[STATUS_TINT[item.to]])}>{statusLabel(item.to)}</span>,
    )
  }
  return (
    <li className="relative flex flex-wrap items-center gap-x-2 py-0.5 text-xs text-muted-foreground">
      <Node className={item.kind === 'completed' ? dot : 'bg-card'}>
        {item.kind === 'completed' ? <CheckCircle2 className="size-4" /> : <span className={cn('size-2 rounded-full', 'bg-muted-foreground/50')} />}
      </Node>
      <span className="flex items-center gap-1.5">{label}</span>
      <span>· {item.kind === 'status' && item.at.length === 10 ? fullDate(item.at) : dateTime(item.at)}</span>
    </li>
  )
}

export function TaskComments({ task }: { task: Task }) {
  const taskId = task.id
  const all = useTaskComments(taskId)
  const versions = useTaskVersions(taskId) // oldest first
  const addComment = useAddTaskComment()
  const [text, setText] = useState('')
  const newDraft = useDraft('new_comment', taskId)
  const pendingText = newDraft.pending ? draftText(newDraft.pending) : null
  const editNew = (value: string) => {
    setText(value)
    newDraft.update(JSON.stringify({ text: value }))
  }
  const [showStatus, setShowStatus] = useState(true)
  const t = useT()

  const items = useMemo(() => {
    const out: TimelineItem[] = []
    out.push({ kind: 'created', at: task.createdAt })
    for (const c of all) out.push({ kind: 'comment', at: c.createdAt, c })
    if (task.completedAt && task.status === 'done') out.push({ kind: 'completed', at: task.completedAt })
    if (showStatus) {
      versions.forEach((v, i) => {
        // "→ Done" is already shown by the Completed event.
        const doneShown = v.snapshot.status === 'done' && task.status === 'done' && task.completedAt
        if (i > 0 && versions[i - 1].snapshot.status !== v.snapshot.status && !doneShown)
          out.push({ kind: 'status', at: v.date, from: versions[i - 1].snapshot.status, to: v.snapshot.status })
      })
    }
    // A date-only status event sorts at the start of its day.
    return out.sort((a, b) => a.at.localeCompare(b.at))
  }, [all, versions, task.createdAt, task.completedAt, task.status, showStatus])

  return (
    <div>
      <label className="mb-3 flex w-fit items-center gap-2 text-xs text-muted-foreground">
        <Switch size="sm" checked={showStatus} onCheckedChange={setShowStatus} />
        {t.tasks.comments.showStatus}
      </label>
      <ol className="relative ml-2 space-y-3 pl-8">
        <span aria-hidden className="absolute top-3 bottom-3 left-[7px] w-px bg-border" />
        {items.map((item, i) =>
          item.kind === 'comment' ? (
            <li key={item.c.id} className="relative">
              <Node className="bg-primary" />
              <CommentItem c={item.c} />
            </li>
          ) : (
            <EventRow key={item.kind + i} item={item} />
          ),
        )}
        <li className="relative">
          <Node className="border-2 border-dashed border-primary bg-card">
            <Plus className="size-2.5 text-primary" />
          </Node>
          <p className="mb-2 text-sm font-medium">{t.tasks.comments.addTitle}</p>
          <div className="space-y-2 rounded-xl border p-3">
            {newDraft.pending && pendingText?.trim() && pendingText !== text && (
              <DraftRestorePrompt draft={newDraft.pending} onRestore={() => { setText(pendingText); void newDraft.discard() }} onDiscard={() => void newDraft.discard()} />
            )}
            <MarkdownField small value={text} onChange={editNew} minHeight="3.5rem" placeholder={t.tasks.comments.placeholder} />
            <div className="flex justify-end">
              <Button
                size="sm"
                disabled={!text.trim()}
                onClick={() => {
                  addComment.mutate({ taskId, text: text.trim() }, { onSuccess: () => { setText(''); void newDraft.discard() } })
                }}
              >
                {t.tasks.comments.add}
              </Button>
            </div>
          </div>
        </li>
      </ol>
    </div>
  )
}
