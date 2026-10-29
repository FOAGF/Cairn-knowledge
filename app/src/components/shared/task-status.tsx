import { Check, ChevronDown } from 'lucide-react'
import { useCallback } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { type Task, type TaskPatch, type TaskStatus, useUpdateTask } from '@/api/tasks'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { getT, useLang, useT } from '@/i18n'
import { statusLabel, STATUS_ORDER, STATUS_TINT, TINT_CLASS } from '@/lib/labels'
import { cn } from '@/lib/utils'

/** The fields these helpers need, so callers may pass a full Task or a slimmer object. */
type TaskRef = Pick<Task, 'id' | 'key' | 'title' | 'status'>

/** Changes accepted by `useChangeTask`: `dueDate` may be undefined / null / '' to clear it. */
export type TaskChanges = Omit<TaskPatch, 'dueDate'> & { dueDate?: string | null }

/**
 * Updates a task through Rust. When it becomes Done, offers to save knowledge from it
 * (FR-TASK-KN-3). Use this everywhere a task's status can change. `then` hears how the save
 * went (a failure is already shown as a toast).
 */
export function useChangeTask() {
  const { mutate } = useUpdateTask()
  const navigate = useNavigate()
  return useCallback(
    (task: TaskRef, changes: TaskChanges, then?: { onSuccess?: (saved: Task) => void; onError?: () => void }) => {
      // `dueDate: ''` clears the date in Rust; `null` would leave it unchanged.
      const patch: TaskPatch = { ...changes }
      if ('dueDate' in changes) patch.dueDate = changes.dueDate ?? ''
      mutate(
        { id: task.id, patch },
        {
          onError: () => then?.onError?.(),
          onSuccess: (saved) => {
            then?.onSuccess?.(saved)
            if (changes.status === 'done' && task.status !== 'done') {
              const t = getT().shared.taskStatus
              toast(t.saveKnowledge, {
                description: `${task.key} · ${task.title}`,
                duration: 8000,
                action: { label: t.createKnowledge, onClick: () => navigate('/knowledge/new?fromTask=' + task.key) },
              })
            }
          },
        },
      )
    },
    [mutate, navigate],
  )
}

/** A status as a small coloured label: the dropdown's menu, the bulk bar and its confirmation. */
export function StatusPill({ status, className }: { status: TaskStatus; className?: string }) {
  useLang()
  return (
    <span className={cn('inline-flex rounded-md px-2 py-0.5 text-[11px] font-semibold tracking-wide whitespace-nowrap uppercase', TINT_CLASS[STATUS_TINT[status]], className)}>
      {statusLabel(status)}
    </span>
  )
}

/**
 * Status button: a coloured pill that opens a menu of statuses.
 * Clicks don't bubble, so it can sit inside clickable rows and cards.
 */
export function StatusDropdown({ task, className, align = 'start' }: { task: TaskRef; className?: string; align?: 'start' | 'end' }) {
  const change = useChangeTask()
  const pick = (status: TaskStatus) => status !== task.status && change(task, { status })
  const t = useT()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          onClick={(e) => e.stopPropagation()}
          onPointerDown={(e) => e.stopPropagation()}
          className={cn(
            'inline-flex h-6 items-center gap-1 rounded-md pr-1.5 pl-2 text-[11px] font-semibold tracking-wide whitespace-nowrap uppercase transition-opacity hover:opacity-80',
            TINT_CLASS[STATUS_TINT[task.status]],
            className,
          )}
          aria-label={t.shared.taskStatus.change(statusLabel(task.status))}
        >
          {statusLabel(task.status)}
          <ChevronDown className="size-3.5" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align={align} onClick={(e) => e.stopPropagation()} className="min-w-44">
        {STATUS_ORDER.map((s) => (
          <DropdownMenuItem key={s} onSelect={() => pick(s)} className="justify-between">
            <StatusPill status={s} />
            {s === task.status && <Check className="size-4 text-primary" />}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
