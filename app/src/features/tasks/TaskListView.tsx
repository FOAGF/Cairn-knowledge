// List view (FR-TASK-UI-1): rows like the Home page, with more detail. Each row has a checkbox for
// changing many tasks at once (FR-TASK-UI-7): the header selects all shown, Shift+click a range.
import { useNavigate } from 'react-router'
import { StatusDropdown } from '@/components/shared/task-status'
import { PriorityIcon, CategoryBadge, MoreTagsBadge, TagChip, TaskTypeIcon } from '@/components/shared/tint'
import { Checkbox } from '@/components/ui/checkbox'
import type { Task } from '@/api/tasks'
import type { Category } from '@/api/bindings'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { DueText } from './taskUi'

export type TaskSelection = {
  selected: Set<string>
  /** `range`: Shift was held, so everything from the last ticked row to this one follows it. */
  toggle: (id: string, range: boolean) => void
  /** Ticks every shown task, or clears them all when all are ticked. */
  toggleAll: () => void
}

export function TaskListView({ tasks, categories, selection }: { tasks: Task[]; categories: Category[]; selection: TaskSelection }) {
  const navigate = useNavigate()
  // `t` is a task below.
  const m = useT().tasks.list
  const { selected } = selection
  const ticked = tasks.filter((t) => selected.has(t.id)).length
  const all = ticked === 0 ? false : ticked === tasks.length ? true : 'indeterminate'
  return (
    <div className="rounded-2xl border bg-card p-2 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      <label className="flex w-fit cursor-pointer items-center gap-3 rounded-lg px-3 py-1.5 text-xs text-muted-foreground select-none hover:text-foreground">
        <Checkbox checked={all} onCheckedChange={selection.toggleAll} aria-label={m.selectAll} />
        {ticked > 0 ? m.selectedOf(ticked, tasks.length) : m.selectAllShown(tasks.length)}
      </label>
      <ul className="divide-y border-t">
        {tasks.map((t) => {
          const closed = t.status === 'done' || t.status === 'wont_do'
          const isTicked = selected.has(t.id)
          return (
            <li
              key={t.id}
              className={cn('flex cursor-pointer items-center gap-3 rounded-xl px-3 py-3 hover:bg-muted/50', isTicked && 'bg-tint-blue/40 hover:bg-tint-blue/60')}
              onClick={() => navigate(`/tasks/${t.key}`)}
            >
              <Checkbox
                checked={isTicked}
                aria-label={m.select(t.key)}
                // Shift+click must not select the text between the rows.
                onMouseDown={(e) => e.shiftKey && e.preventDefault()}
                onClick={(e) => {
                  e.stopPropagation()
                  e.preventDefault()
                  selection.toggle(t.id, e.shiftKey)
                }}
              />
              <TaskTypeIcon type={t.type} />
              <span className="w-20 shrink-0 font-mono text-xs text-muted-foreground">{t.key}</span>
              <span className={cn('min-w-0 flex-1 truncate text-sm font-medium', closed && 'text-muted-foreground line-through')}>
                {t.title}
              </span>
              <CategoryBadge category={categories.find((p) => p.id === t.categoryId)} className="hidden w-36 shrink-0 lg:inline-flex" />
              <LabelCell labels={t.labels} />
              <span className="hidden w-32 shrink-0 sm:flex">
                <StatusDropdown task={t} />
              </span>
              <DueText task={t} className="hidden w-28 shrink-0 text-right text-sm md:block" />
              <PriorityIcon priority={t.priority} className="w-5 shrink-0 justify-center" />
            </li>
          )
        })}
      </ul>
    </div>
  )
}

/** Up to two labels in a fixed column, never running into the status: the longest label is
 *  shortened with "…"; the other keeps its width, up to 60% of the column. More labels show as "+N". */
function LabelCell({ labels }: { labels: string[] }) {
  const shown = labels.slice(0, 2)
  const longest = shown.reduce((a, b) => (b.length > a.length ? b : a), '')
  return (
    <span className="hidden w-32 shrink-0 items-center gap-1 overflow-hidden xl:flex">
      {shown.map((l) => (
        <TagChip key={l} tag={l} className={l === longest ? 'min-w-0' : 'max-w-[60%] shrink-0'} />
      ))}
      <MoreTagsBadge hidden={labels.slice(2)} />
    </span>
  )
}
