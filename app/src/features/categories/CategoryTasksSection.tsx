import { formatDate } from '@/lib/format'
import { ChevronDown, CircleCheckBig, Columns3, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { SectionCard } from '@/components/shared/PageHeader'
import { StatusDropdown } from '@/components/shared/task-status'
import { PriorityIcon, TaskTypeIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { FoldButton, useFold } from '@/components/shared/Fold'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { useT } from '@/i18n'
import { isClosed, statusLabel } from '@/lib/labels'
import type { Category } from '@/api/categories'
import { type Task, type TaskFilter, type TaskStatus, useTaskList } from '@/api/tasks'
import { cn } from '@/lib/utils'
import { DueText } from '../tasks/taskUi'

// Literal classes per status (Tailwind needs full strings).
const BAR: Partial<Record<TaskStatus, { bar: string; dot: string }>> = {
  todo: { bar: 'bg-tint-slate-fg/60', dot: 'bg-tint-slate-fg/60' },
  in_progress: { bar: 'bg-tint-blue-fg', dot: 'bg-tint-blue-fg' },
  blocked: { bar: 'bg-tint-rose-fg', dot: 'bg-tint-rose-fg' },
  done: { bar: 'bg-tint-green-fg', dot: 'bg-tint-green-fg' },
}
const SEGMENTS: TaskStatus[] = ['todo', 'in_progress', 'blocked', 'done']

/** Open tasks shown before the list folds. */
const OPEN_LIMIT = 6

/** Tasks of one category: status overview, open list, recently completed. */
export function CategoryTasksSection({ category }: { category: Category }) {
  const navigate = useNavigate()
  const filter = useMemo<TaskFilter>(() => ({ categoryIds: [category.id], includeClosed: true }), [category.id])
  const tasks = useTaskList(filter)
  const [showDone, setShowDone] = useState(false)
  const t = useT()
  const k = t.categories.tasks

  const counts = useMemo(() => {
    const c: Record<string, number> = { todo: 0, in_progress: 0, blocked: 0, done: 0 }
    for (const t of tasks) if (t.status in c) c[t.status]++
    return c
  }, [tasks])
  const total = SEGMENTS.reduce((n, s) => n + counts[s], 0)

  const open = useMemo(
    () => tasks.filter((t) => !isClosed(t.status)).sort((a, b) => (a.dueDate ?? '9999').localeCompare(b.dueDate ?? '9999')),
    [tasks],
  )
  // Long lists fold automatically; the rest is one click away.
  const openFold = useFold(open, OPEN_LIMIT)
  const recentDone = useMemo(
    () =>
      tasks
        .filter((t) => t.status === 'done')
        .sort((a, b) => (b.completedAt ?? b.updatedAt).localeCompare(a.completedAt ?? a.updatedAt))
        .slice(0, 5),
    [tasks],
  )

  return (
    <SectionCard
      icon={CircleCheckBig}
      tint="blue"
      title={t.common.nav.tasks}
      description={k.description(open.length)}
      className="min-w-0"
      action={
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => navigate(`/tasks?category=${category.id}&view=board`)}>
            <Columns3 /> {k.openBoard}
          </Button>
          <Button size="sm" onClick={() => navigate(`/tasks?new=1&category=${category.id}`)}>
            <Plus /> {t.common.newTask}
          </Button>
        </div>
      }
    >
      {/* Status overview */}
      <div className="mb-5">
        <div className="flex h-2.5 gap-0.5 overflow-hidden rounded-full bg-muted">
          {total > 0 &&
            SEGMENTS.filter((s) => counts[s] > 0).map((s) => (
              <div key={s} className={cn('h-full', BAR[s]?.bar)} style={{ width: `${(counts[s] / total) * 100}%` }} title={k.statusCount(statusLabel(s), counts[s])} />
            ))}
        </div>
        <div className="mt-2.5 flex flex-wrap gap-x-5 gap-y-1 text-xs text-muted-foreground">
          {SEGMENTS.map((s) => (
            <Link key={s} to={`/tasks?category=${category.id}&status=${s}`} className="flex items-center gap-1.5 hover:text-foreground">
              <span className={cn('size-2 rounded-full', BAR[s]?.dot)} />
              {statusLabel(s)}
              <span className="font-semibold text-foreground">{counts[s]}</span>
            </Link>
          ))}
        </div>
      </div>

      {open.length === 0 ? (
        <p className="rounded-xl border border-dashed py-8 text-center text-sm text-muted-foreground">{k.noOpen}</p>
      ) : (
        <>
          <ul className="divide-y">
            {openFold.visible.map((t) => (
              <OpenRow key={t.id} task={t} />
            ))}
          </ul>
          <FoldButton fold={openFold} noun={k.foldNoun(openFold.hiddenCount)} className="mt-1" />
        </>
      )}

      <Collapsible open={showDone} onOpenChange={setShowDone} className="mt-4 border-t pt-3">
        <CollapsibleTrigger className="flex w-full items-center gap-2 text-sm font-medium text-muted-foreground hover:text-foreground">
          <ChevronDown className={cn('size-4 transition-transform', !showDone && '-rotate-90')} />
          {k.recentlyCompleted}
          <span className="text-xs font-normal">({recentDone.length})</span>
        </CollapsibleTrigger>
        <CollapsibleContent>
          {recentDone.length === 0 ? (
            <p className="py-3 text-sm text-muted-foreground">{k.noneCompleted}</p>
          ) : (
            <ul className="mt-2 divide-y">
              {recentDone.map((t) => (
                <li key={t.id} className="flex items-center gap-3 py-2">
                  <TaskTypeIcon type={t.type} />
                  <Link to={`/tasks/${t.key}`} className="min-w-0 truncate text-sm text-muted-foreground hover:text-primary">
                    <span className="mr-2 font-mono text-xs">{t.key}</span>
                    {t.title}
                  </Link>
                  <span className="ml-auto shrink-0 text-xs text-muted-foreground">
                    {t.completedAt ? formatDate(t.completedAt, 'date') : '—'}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </CollapsibleContent>
      </Collapsible>
    </SectionCard>
  )
}

function OpenRow({ task: t }: { task: Task }) {
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2.5">
      <TaskTypeIcon type={t.type} />
      <Link to={`/tasks/${t.key}`} className="min-w-0 flex-1 basis-40 truncate text-sm hover:text-primary">
        <span className="mr-2 font-mono text-xs text-muted-foreground">{t.key}</span>
        {t.title}
      </Link>
      <DueText task={t} className="w-28 shrink-0 text-right text-sm" />
      <PriorityIcon priority={t.priority} className="w-5 justify-center" />
      <StatusDropdown task={t} align="end" className="w-[7.5rem] justify-between" />
    </li>
  )
}
