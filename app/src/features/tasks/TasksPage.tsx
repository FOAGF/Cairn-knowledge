import { CircleCheckBig, KanbanSquare, List, Plus, Search, SearchX, X } from 'lucide-react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { EmptyState, PageHeader } from '@/components/shared/PageHeader'
import { CategoryDot, TaskTypeIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { useT } from '@/i18n'
import { priorityLabel, PRIORITY_ORDER, statusLabel, STATUS_ORDER, typeLabel, TYPE_ORDER } from '@/lib/labels'
import { NewTaskDialog } from './NewTaskDialog'
import { TaskBoardView } from './TaskBoardView'
import { TaskBulkBar } from './TaskBulkBar'
import { TaskListView, type TaskSelection } from './TaskListView'
import { useAllLabels } from './taskUi'
import { commands } from '@/api/bindings'
import { cn } from '@/lib/utils'
import { useCategoryList } from '@/api/categories'
import type { DueFilter, Priority, Task, TaskFilter, TaskSortField, TaskStatus, TaskType } from '@/api/tasks'

const ALL = 'all'
const NO_TASKS: Task[] = []
type SortKey = 'due' | 'priority' | 'created' | 'updated' | 'key'
const SORT_KEYS: SortKey[] = ['due', 'priority', 'created', 'updated', 'key']

/** URL value of the Due filter -> Rust's DueFilter. */
const DUE_FILTER: Record<string, DueFilter> = { overdue: 'overdue', today: 'today', week: 'this_week', none: 'none' }
/** Newest first for the date sorts; the other sorts have one natural order in Rust. */
const DESCENDING: SortKey[] = ['created', 'updated']

function FilterSelect({
  value,
  onChange,
  label,
  options,
}: {
  value: string
  onChange: (v: string) => void
  label: string
  options: { value: string; label: string; dot?: React.ReactNode }[]
}) {
  const t = useT()
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className="min-w-32 bg-card" aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL}>{t.tasks.page.all(label)}</SelectItem>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            {o.dot}
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

export default function TasksPage() {
  const categories = useCategoryList()
  const labels = useAllLabels()
  const [params, setParams] = useSearchParams()
  const t = useT()
  const tp = t.tasks.page

  const view = params.get('view') === 'board' ? 'board' : 'list'
  const category = params.get('category') ?? ALL
  const status = params.get('status') ?? ALL
  const type = params.get('type') ?? ALL
  const priority = params.get('priority') ?? ALL
  const label = params.get('label') ?? ALL
  const due = params.get('due') ?? ALL
  const q = params.get('q') ?? ''
  const showClosed = params.get('closed') === '1'
  const sort = (params.get('sort') as SortKey) || 'due'
  const newOpen = params.get('new') === '1'

  const set = (key: string, value: string | null) =>
    setParams(
      (p) => {
        const next = new URLSearchParams(p)
        if (value === null || value === ALL || value === '') next.delete(key)
        else next.set(key, value)
        return next
      },
      { replace: true },
    )

  // The URL filters become one TaskFilter; Rust filters and sorts. Memoized so the query key is stable.
  // The board keeps closed tasks so its Done column can still receive drops.
  const filter = useMemo<TaskFilter>(
    () => ({
      categoryIds: category !== ALL ? [category] : null,
      statuses: status === 'active' ? ['in_progress', 'blocked'] : status !== ALL ? [status as TaskStatus] : null,
      includeClosed: view === 'board' || showClosed,
      types: type !== ALL ? [type as TaskType] : null,
      priorities: priority !== ALL ? [priority as Priority] : null,
      labels: label !== ALL ? [label] : null,
      due: DUE_FILTER[due] ?? null,
      text: q.trim() || null,
      sort: sort as TaskSortField,
      descending: DESCENDING.includes(sort),
    }),
    [category, status, type, priority, label, due, q, view, showClosed, sort],
  )
  // Same query as useTasks(filter), but keeps the previous result while a new filter loads (no flicker while typing).
  const visible = useQuery({
    queryKey: ['tasks', 'list', filter],
    queryFn: () => commands.listTasks(filter),
    placeholderData: keepPreviousData,
  }).data ?? NO_TASKS

  const filtersActive = [category, status, type, priority, label, due].some((v) => v !== ALL) || q !== ''
  const clearFilters = () =>
    setParams(
      (p) => {
        const next = new URLSearchParams()
        if (p.get('view')) next.set('view', p.get('view')!)
        return next
      },
      { replace: true },
    )

  const [manualOpen, setManualOpen] = useState(false)

  // FR-TASK-UI-7: the ticked tasks. A task that leaves the list (a filter, closing, deleting) leaves
  // the selection too, so no hidden task is ever changed. `anchor` is the last ticked row, the
  // start of a Shift+click range.
  const [selected, setSelected] = useState<Set<string>>(() => new Set())
  const [anchor, setAnchor] = useState<string | null>(null)
  const [listed, setListed] = useState(visible)
  if (listed !== visible) {
    setListed(visible)
    const shown = new Set(visible.map((t) => t.id))
    if ([...selected].some((id) => !shown.has(id))) setSelected(new Set([...selected].filter((id) => shown.has(id))))
  }
  const ticked = useMemo(() => visible.filter((t) => selected.has(t.id)), [visible, selected])
  const selection: TaskSelection = {
    selected,
    toggle: (id, range) => {
      const from = range && anchor ? visible.findIndex((t) => t.id === anchor) : -1
      const to = visible.findIndex((t) => t.id === id)
      const ids = from >= 0 && to >= 0 ? visible.slice(Math.min(from, to), Math.max(from, to) + 1).map((t) => t.id) : [id]
      // From the latest selection, so quick clicks in a row all count.
      setSelected((s) => {
        const on = !s.has(id)
        const next = new Set(s)
        for (const i of ids) {
          if (on) next.add(i)
          else next.delete(i)
        }
        return next
      })
      setAnchor(id)
    },
    toggleAll: () => setSelected(ticked.length === visible.length ? new Set() : new Set(visible.map((t) => t.id))),
  }
  const clearSelection = () => {
    setSelected(new Set())
    setAnchor(null)
  }

  return (
    // Room under the list for the selection bar.
    <div className={cn(view === 'list' && ticked.length > 0 && 'pb-20')}>
      <PageHeader
        icon={CircleCheckBig}
        tint="green"
        title={t.common.nav.tasks}
        description={tp.count(visible.length, filtersActive)}
        actions={
          <>
            <ToggleGroup
              type="single"
              variant="outline"
              value={view}
              onValueChange={(v) => v && set('view', v === 'board' ? 'board' : null)}
              aria-label={tp.view}
            >
              <ToggleGroupItem value="list" aria-label={tp.listView}>
                <List /> {tp.list}
              </ToggleGroupItem>
              <ToggleGroupItem value="board" aria-label={tp.boardView}>
                <KanbanSquare /> {tp.board}
              </ToggleGroupItem>
            </ToggleGroup>
            <Button onClick={() => setManualOpen(true)}>
              <Plus /> {t.common.newTask}
            </Button>
          </>
        }
      />

      {/* Filters: category, status, priority, label, due, search (FR-TASK-UI-1/2) */}
      <div className="mb-5 flex flex-wrap items-center gap-2">
        <div className="relative w-full sm:w-64">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input className="h-8 bg-card pl-8" placeholder={tp.search} value={q} onChange={(e) => set('q', e.target.value)} />
        </div>
        <FilterSelect
          value={category}
          onChange={(v) => set('category', v)}
          label={t.common.category}
          options={categories.map((p) => ({ value: p.id, label: p.name, dot: <CategoryDot color={p.color} className="size-2" /> }))}
        />
        <FilterSelect value={status} onChange={(v) => set('status', v)} label={t.common.status} options={[{ value: 'active', label: tp.active }, ...STATUS_ORDER.map((s) => ({ value: s, label: statusLabel(s) }))]} />
        <FilterSelect value={type} onChange={(v) => set('type', v)} label={t.common.type} options={TYPE_ORDER.map((ty) => ({ value: ty, label: typeLabel(ty), dot: <TaskTypeIcon type={ty} className="mr-1.5" /> }))} />
        <FilterSelect value={priority} onChange={(v) => set('priority', v)} label={t.common.priority} options={PRIORITY_ORDER.map((p) => ({ value: p, label: priorityLabel(p) }))} />
        <FilterSelect value={label} onChange={(v) => set('label', v)} label={tp.label} options={labels.map((l) => ({ value: l, label: l }))} />
        <FilterSelect value={due} onChange={(v) => set('due', v)} label={tp.due} options={Object.entries(tp.dueOptions).map(([value, l]) => ({ value, label: l }))} />
        {filtersActive && (
          <Button variant="ghost" size="sm" onClick={clearFilters}>
            <X /> {tp.clear}
          </Button>
        )}
        <div className="ml-auto flex items-center gap-3">
          <Select value={sort} onValueChange={(v) => set('sort', v === 'due' ? null : v)}>
            <SelectTrigger className="bg-card" aria-label={tp.sortBy}>
              <span className="text-muted-foreground">{tp.sort}</span>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SORT_KEYS.map((k) => (
                <SelectItem key={k} value={k}>
                  {tp.sortOptions[k]}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {/* FR-TASK-ST-3: closed tasks hidden by default */}
          <label className="flex items-center gap-2 text-sm text-muted-foreground">
            <Switch checked={showClosed} onCheckedChange={(c) => set('closed', c ? '1' : null)} />
            {tp.showClosed}
          </label>
        </div>
      </div>

      {view === 'board' ? (
        <TaskBoardView tasks={visible} categories={categories} showClosed={showClosed} />
      ) : visible.length === 0 ? (
        <EmptyState icon={SearchX} title={tp.noMatch}>
          {tp.emptyHint(filtersActive, showClosed)}
        </EmptyState>
      ) : (
        <TaskListView tasks={visible} categories={categories} selection={selection} />
      )}
      {view === 'list' && ticked.length > 0 && <TaskBulkBar tasks={ticked} categories={categories} allLabels={labels} onClear={clearSelection} />}

      <NewTaskDialog
        open={newOpen || manualOpen}
        onOpenChange={(o) => {
          setManualOpen(o)
          if (!o) set('new', null)
        }}
        defaultCategoryId={category !== ALL ? category : undefined}
      />
    </div>
  )
}
