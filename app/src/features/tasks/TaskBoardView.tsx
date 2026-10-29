// Board view (FR-TASK-UI-2): one column per status, drag a card to change its status.
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from '@dnd-kit/core'
import { ChevronLeft, ChevronRight } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { StatusDropdown } from '@/components/shared/task-status'
import { PriorityIcon, CategoryDot, MoreTagsBadge, TagChip, TaskTypeIcon } from '@/components/shared/tint'
import { isClosed, statusLabel, STATUS_ORDER, STATUS_TINT, TINT_CLASS } from '@/lib/labels'
import type { Category } from '@/api/bindings'
import { useT } from '@/i18n'
import { primeTaskInLists, type Task, type TaskStatus } from '@/api/tasks'
import { cn } from '@/lib/utils'
import { DueText, useChangeTask } from './taskUi'

// Won't do tasks are not shown on the board (List view + "Show closed" still has them).
const BOARD_STATUSES = STATUS_ORDER.filter((s) => s !== 'wont_do')

function CardBody({ task, category, withStatus }: { task: Task; category?: Category; withStatus?: boolean }) {
  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-1.5">
          <TaskTypeIcon type={task.type} />
          <span className="font-mono text-xs text-muted-foreground">{task.key}</span>
        </span>
        <PriorityIcon priority={task.priority} />
      </div>
      <p className={cn('mt-1.5 text-sm leading-snug font-medium', isClosed(task.status) && 'text-muted-foreground')}>{task.title}</p>
      {task.labels.length > 0 && (
        <div className="mt-2 flex flex-wrap items-center gap-1">
          {task.labels.slice(0, 3).map((l) => (
            <TagChip key={l} tag={l} />
          ))}
          <MoreTagsBadge hidden={task.labels.slice(3)} />
        </div>
      )}
      <div className="mt-3 flex items-center justify-between gap-2 text-xs">
        <span className="flex min-w-0 items-center gap-1.5 text-muted-foreground">
          {category && <CategoryDot color={category.color} className="size-2" />}
          <span className="truncate">{category?.name}</span>
        </span>
        {task.dueDate && <DueText task={task} className="shrink-0" />}
      </div>
      {withStatus && (
        <div className="mt-2.5 border-t pt-2.5">
          <StatusDropdown task={task} />
        </div>
      )}
    </>
  )
}

function BoardCard({ task, category }: { task: Task; category?: Category }) {
  const navigate = useNavigate()
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: task.id })
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      onClick={() => navigate(`/tasks/${task.key}`)}
      className={cn(
        'cursor-grab rounded-xl border bg-card p-3.5 shadow-[0_1px_2px_rgba(16,24,40,0.04)] transition-shadow hover:shadow-md',
        isDragging && 'opacity-40',
      )}
    >
      <CardBody task={task} category={category} withStatus />
    </div>
  )
}

function Column({
  status,
  tasks,
  categories,
  collapsed,
  onToggle,
}: {
  status: TaskStatus
  tasks: Task[]
  categories: Category[]
  collapsed: boolean
  onToggle?: () => void
}) {
  const { setNodeRef, isOver } = useDroppable({ id: status })
  const t = useT()
  const header = (
    <div className={cn('flex items-center gap-2', collapsed && 'flex-col')}>
      <span className={cn('rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap', TINT_CLASS[STATUS_TINT[status]])}>
        {statusLabel(status)}
      </span>
      <span className="text-xs text-muted-foreground">{tasks.length}</span>
      {onToggle && (
        <button
          className={cn('rounded p-0.5 text-muted-foreground hover:bg-muted', !collapsed && 'ml-auto')}
          onClick={onToggle}
          aria-label={collapsed ? t.tasks.board.expand(statusLabel(status)) : t.tasks.board.collapse(statusLabel(status))}
        >
          {collapsed ? <ChevronRight className="size-4" /> : <ChevronLeft className="size-4" />}
        </button>
      )}
    </div>
  )
  return (
    <div
      ref={setNodeRef}
      className={cn(
        'flex shrink-0 flex-col rounded-2xl border bg-muted/40 p-3 transition-colors',
        collapsed ? 'w-16 shrink-0 items-center' : 'min-w-64 flex-1 basis-0',
        isOver && 'border-primary bg-accent/60',
      )}
    >
      {header}
      {!collapsed && (
        <div className="mt-3 flex min-h-24 flex-1 flex-col gap-2.5">
          {tasks.map((t) => (
            <BoardCard key={t.id} task={t} category={categories.find((p) => p.id === t.categoryId)} />
          ))}
          {tasks.length === 0 && (
            <p className="rounded-xl border border-dashed p-4 text-center text-xs text-muted-foreground">{t.tasks.board.dropHere}</p>
          )}
        </div>
      )}
      {collapsed && <p className="mt-3 text-[11px] text-muted-foreground [writing-mode:vertical-rl]">{t.tasks.board.dropTo(statusLabel(status))}</p>}
    </div>
  )
}

export function TaskBoardView({ tasks, categories, showClosed }: { tasks: Task[]; categories: Category[]; showClosed: boolean }) {
  const change = useChangeTask()
  const [active, setActive] = useState<Task | null>(null)
  const [expanded, setExpanded] = useState<Partial<Record<TaskStatus, boolean>>>({})
  // A dropped card shows in its new column at once, while Rust saves the change. The drop
  // animation flies the card to where it is when the drag ends, so without this it went back to
  // the old column first. Each entry goes when the save returns; a failed save puts the card back.
  const [moved, setMoved] = useState<Record<string, TaskStatus>>({})
  const shown = useMemo(() => tasks.map((t) => (moved[t.id] ? { ...t, status: moved[t.id] } : t)), [tasks, moved])
  // Distance constraint so a plain click still opens the task.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }))

  // Only the entry this drop made: a second drop of the same card may have replaced it.
  const settle = (id: string, status: TaskStatus) =>
    setMoved((m) => {
      if (m[id] !== status) return m
      const { [id]: _, ...rest } = m
      return rest
    })
  const onStart = (e: DragStartEvent) => setActive(shown.find((t) => t.id === e.active.id) ?? null)
  const onEnd = (e: DragEndEvent) => {
    setActive(null)
    const task = shown.find((t) => t.id === e.active.id)
    const status = e.over?.id as TaskStatus | undefined
    if (!task || !status || status === task.status) return
    setMoved((m) => ({ ...m, [task.id]: status }))
    change(
      task,
      { status },
      {
        // The lists get the saved task in the same render that drops the entry, so the card stays put.
        onSuccess: (saved) => {
          primeTaskInLists(saved)
          settle(task.id, status)
        },
        onError: () => settle(task.id, status),
      },
    )
  }

  return (
    <DndContext sensors={sensors} onDragStart={onStart} onDragEnd={onEnd} onDragCancel={() => setActive(null)}>
      <div className="flex items-stretch gap-4 overflow-x-auto pb-3">
        {BOARD_STATUSES.map((s) => {
          const closed = isClosed(s)
          // Closed columns collapse into a slim drop target unless "Show closed" is on.
          const collapsed = closed && !(showClosed || expanded[s])
          return (
            <Column
              key={s}
              status={s}
              tasks={shown.filter((t) => t.status === s)}
              categories={categories}
              collapsed={collapsed}
              onToggle={closed ? () => setExpanded((x) => ({ ...x, [s]: !(showClosed || x[s]) })) : undefined}
            />
          )
        })}
      </div>
      <DragOverlay>
        {active && (
          <div className="rotate-2 rounded-xl border bg-card p-3.5 shadow-lg">
            <CardBody task={active} category={categories.find((p) => p.id === active.categoryId)} />
          </div>
        )}
      </DragOverlay>
    </DndContext>
  )
}
