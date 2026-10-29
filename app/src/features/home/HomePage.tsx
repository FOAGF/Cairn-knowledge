// Home (FR-HOME-1..6, D-54): what needs you today. A large clock, the greeting with a line of
// counts, the Ask box and a *Needs attention* line, then the Today list beside the calendar.
// Everything comes from one Rust call (api/home.ts): Home never loads every task or page.
import { format, parseISO } from 'date-fns'
import {
  Archive,
  ArrowLeft,
  ArrowRight,
  Bot,
  CalendarDays,
  ChevronDown,
  ChevronUp,
  CircleAlert,
  DatabaseZap,
  FileSearch,
  type LucideIcon,
  TriangleAlert,
} from 'lucide-react'
import { type ComponentProps, type ReactNode, useMemo, useState } from 'react'
import { Link } from 'react-router'
import { useAppInfo } from '@/api/app'
import { useBackupStatus } from '@/api/backups'
import type { DueDay, Task } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { type HomeCounts, type TodayGroup, type TodayGroupKind, useHomeOverview, useLocalDay } from '@/api/home'
import { useModels } from '@/api/providers'
import { useSettings } from '@/api/settings'
import { useTasks } from '@/api/tasks'
import { AskBar } from '@/components/layout/AskBar'
import { SectionCard } from '@/components/shared/PageHeader'
import { StatusDropdown } from '@/components/shared/task-status'
import { PriorityIcon, TaskTypeIcon } from '@/components/shared/tint'
import { Calendar, CalendarDayButton } from '@/components/ui/calendar'
import { dateLocale, dueLabel, formatDate } from '@/lib/format'
import { cn } from '@/lib/utils'
import { getT, useLang, useT } from '@/i18n'
import { FirstStartCard } from './FirstStartCard'
import { LocalClock } from './LocalClock'
import { SearchModelNotice } from './SearchModelNotice'

/** Tasks shown per Today group before "Show N more" (Rust sends up to 10 per group). */
const FOLDED = 3
/** Tasks shown for a picked calendar day before "Show N more". */
const DAY_SHOWN = 10

const dayKey = (d: Date) => format(d, 'yyyy-MM-dd')

function greeting() {
  const h = new Date().getHours()
  const g = getT().home.greeting
  return h < 12 ? g.morning : h < 18 ? g.afternoon : g.evening
}

/** Each group's colour and filtered task list; its name is `home.today.group`. */
const GROUP_META: Record<TodayGroupKind, { tone: string; all: string }> = {
  overdue: { tone: 'text-destructive', all: '/tasks?due=overdue' },
  due_today: { tone: 'text-tint-amber-fg', all: '/tasks?due=today' },
  in_progress: { tone: 'text-tint-blue-fg', all: '/tasks?status=in_progress' },
  blocked: { tone: 'text-tint-rose-fg', all: '/tasks?status=blocked' },
}

/** The counts under the greeting; each opens the matching filtered screen (FR-HOME-2). */
function CountsLine({ counts }: { counts: HomeCounts }) {
  const t = useT()
  const c = t.home.counts
  // `label` puts the bold number into its words.
  const items: { n: number; label: (num: ReactNode, n: number) => ReactNode; to: string; warn?: boolean }[] = [
    { n: counts.active, label: c.active, to: '/tasks?status=active' },
    { n: counts.blocked, label: c.blocked, to: '/tasks?status=blocked' },
    { n: counts.dueThisWeek, label: c.dueThisWeek, to: '/tasks?due=week' },
    { n: counts.overdue, label: c.overdue, to: '/tasks?due=overdue', warn: true },
    { n: counts.doneThisWeek, label: c.doneThisWeek, to: '/tasks?status=done&sort=updated' },
    { n: counts.pages, label: c.pages, to: '/knowledge' },
    { n: counts.categories, label: c.categories, to: '/categories' },
  ]
  return (
    <p className="flex flex-wrap justify-center text-sm text-muted-foreground">
      {items.map((it, i) => (
        <span key={it.to} className="flex items-center">
          {i > 0 && (
            <span className="mx-2.5" aria-hidden>
              ·
            </span>
          )}
          <Link
            to={it.to}
            className="underline decoration-muted-foreground/40 decoration-dotted underline-offset-4 hover:text-foreground hover:decoration-solid"
          >
            {it.label(<span className={cn('font-semibold text-foreground', it.warn && it.n > 0 && 'text-destructive')}>{it.n}</span>, it.n)}
          </Link>
        </span>
      ))}
    </p>
  )
}

/** Things that need you, each a link; nothing at all when there are none (FR-HOME-3). */
function NeedsAttention({ pagesNotChecked }: { pagesNotChecked: number }) {
  const backup = useBackupStatus().data
  const models = useModels().data
  const dataFolderWarning = useAppInfo().data?.dataFolderWarning
  const t = useT()
  const items: { icon: LucideIcon; text: string; to: string }[] = []
  // D-56: a live database inside a synced OneDrive folder can be damaged.
  if (dataFolderWarning) items.push({ icon: TriangleAlert, text: dataFolderWarning, to: '/settings/data' })
  if (pagesNotChecked > 0)
    items.push({ icon: FileSearch, text: t.home.attention.notChecked(pagesNotChecked), to: '/knowledge/new?check=unchecked' })
  if (backup?.error) items.push({ icon: Archive, text: t.home.attention.backupFailed(backup.error), to: '/settings/data' })
  if (backup?.checkFailed) items.push({ icon: DatabaseZap, text: t.home.attention.checkFailed, to: '/settings/data' })
  if (models && models.length === 0) items.push({ icon: Bot, text: t.home.attention.noModel, to: '/settings/providers' })
  if (items.length === 0) return null
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 rounded-xl bg-tint-amber px-3.5 py-2.5 text-sm">
      <span className="flex items-center gap-2 font-semibold text-tint-amber-fg">
        <CircleAlert className="size-4" /> {t.home.attention.title}
      </span>
      {items.map((it) => (
        <Link key={it.text} to={it.to} className="flex items-center gap-1.5 underline decoration-tint-amber-fg/50 underline-offset-3 hover:text-tint-amber-fg">
          <it.icon className="size-3.5 shrink-0 text-tint-amber-fg" />
          {it.text}
        </Link>
      ))}
    </div>
  )
}

function TaskRow({ task }: { task: Task }) {
  useLang() // the due date is in the interface language
  const due = task.dueDate ? dueLabel(task.dueDate) : null
  return (
    <li className="grid grid-cols-[1.25rem_4.5rem_minmax(0,1fr)_7rem_1rem_7.5rem] items-center gap-3 border-t py-2">
      <TaskTypeIcon type={task.type} />
      <span className="truncate font-mono text-xs text-muted-foreground">{task.key}</span>
      <Link to={`/tasks/${task.key}`} className="truncate text-sm hover:text-primary">
        {task.title}
      </Link>
      <span
        className={cn(
          'truncate text-right text-sm',
          due?.tone === 'overdue' || due?.tone === 'today' ? 'font-medium text-destructive' : 'text-muted-foreground',
        )}
      >
        {due?.text ?? '—'}
      </span>
      <PriorityIcon priority={task.priority} />
      <StatusDropdown task={task} align="end" className="w-[7.5rem] justify-between" />
    </li>
  )
}

/** `shown` rows, the rest folded behind "Show N more". Tasks beyond the loaded ones link to the task list. */
function FoldedRows({ tasks, total, shown, allTo }: { tasks: Task[]; total: number; shown: number; allTo?: string }) {
  const [open, setOpen] = useState(false)
  const t = useT()
  const hidden = Math.max(0, tasks.length - shown)
  const unloaded = total - tasks.length
  return (
    <>
      <ul>
        {(open ? tasks : tasks.slice(0, shown)).map((t) => (
          <TaskRow key={t.id} task={t} />
        ))}
      </ul>
      {(hidden > 0 || (unloaded > 0 && allTo)) && (
        <div className="flex items-center gap-4 border-t border-dashed py-2 pl-8 text-sm text-muted-foreground">
          {hidden > 0 && (
            <button type="button" onClick={() => setOpen(!open)} className="flex items-center gap-1.5 hover:text-foreground">
              {open ? <ChevronUp className="size-4" /> : <ChevronDown className="size-4" />}
              {open ? t.common.showLess : t.home.today.showMore(hidden)}
            </button>
          )}
          {unloaded > 0 && allTo && (
            <Link to={allTo} className="flex items-center gap-1 text-primary hover:underline">
              {t.home.today.allInTasks(total)} <ArrowRight className="size-3.5" />
            </Link>
          )}
        </div>
      )}
    </>
  )
}

function GroupBlock({ group }: { group: TodayGroup }) {
  const meta = GROUP_META[group.kind]
  const t = useT()
  return (
    <div>
      <p className={cn('flex items-center gap-2 py-1.5 text-[11.5px] font-semibold tracking-[0.06em] uppercase', meta.tone)}>
        <span className="size-2 rounded-full bg-current" />
        {t.home.today.group[group.kind]}
        <span className="font-medium tracking-normal text-muted-foreground">{group.total}</span>
      </p>
      <FoldedRows tasks={group.tasks} total={group.total} shown={FOLDED} allTo={meta.all} />
    </div>
  )
}

/** Overdue, due today, in progress and blocked; empty groups are left out (FR-HOME-4). */
function TodayCard({ groups, today }: { groups: TodayGroup[]; today: string }) {
  const t = useT()
  const filled = groups.filter((g) => g.total > 0)
  const n = filled.reduce((sum, g) => sum + g.total, 0)
  const allTasksLink = (
    <Link to="/tasks" className="flex items-center gap-1 text-sm font-medium whitespace-nowrap text-primary">
      {t.home.today.allTasks} <ArrowRight className="size-4" />
    </Link>
  )
  return (
    <SectionCard title={t.home.today.title} description={t.home.today.description(formatDate(today, 'weekdayDayMonth'), n)} action={allTasksLink}>
      {filled.length === 0 ? (
        <p className="py-8 text-center text-sm text-muted-foreground">{t.home.today.empty}</p>
      ) : (
        <div className="space-y-3">
          {filled.map((g) => (
            <GroupBlock key={g.kind} group={g} />
          ))}
        </div>
      )}
    </SectionCard>
  )
}

/** The open tasks due on a day picked in the calendar (FR-HOME-5). */
function DayCard({ day, onBack }: { day: string; onBack: () => void }) {
  const tasks = useTasks({ dueOn: day, sort: 'priority' })
  const list = tasks.data ?? []
  const t = useT()
  return (
    <SectionCard
      title={formatDate(day, 'weekdayDayMonth')}
      description={tasks.isPending ? t.home.loading : t.home.day.due(list.length)}
      action={
        <button type="button" onClick={onBack} className="flex items-center gap-1 text-sm font-medium whitespace-nowrap text-primary">
          <ArrowLeft className="size-4" /> {t.home.day.back}
        </button>
      }
    >
      {tasks.error ? (
        <p className="py-6 text-center text-sm text-destructive">{errorMessage(tasks.error)}</p>
      ) : !tasks.isPending && list.length === 0 ? (
        <p className="py-8 text-center text-sm text-muted-foreground">{t.home.day.empty}</p>
      ) : (
        <FoldedRows tasks={list} total={list.length} shown={DAY_SHOWN} />
      )}
    </SectionCard>
  )
}

/** A calendar day with the number of open tasks due ("9+" above 9), red on days already past. */
function DueDayButton({ counts, today, children, ...props }: ComponentProps<typeof CalendarDayButton> & { counts: Map<string, number>; today: string }) {
  const key = dayKey(props.day.date)
  const n = counts.get(key) ?? 0
  const selected = props.modifiers.selected
  const t = useT()
  return (
    <CalendarDayButton {...props} className={cn(props.className, n > 0 && 'gap-0.5')}>
      {children}
      {n > 0 && (
        <span
          className={cn(
            'flex h-[15px] min-w-[15px] items-center justify-center rounded-full px-1 text-[9px]! leading-none font-semibold opacity-100!',
            selected ? 'bg-primary-foreground text-primary' : key < today ? 'bg-destructive text-primary-foreground' : 'bg-primary text-primary-foreground',
          )}
          aria-label={t.home.calendar.tasksDue(n)}
        >
          {n > 9 ? '9+' : n}
        </span>
      )}
    </CalendarDayButton>
  )
}

function CalendarCard({
  dueDays,
  today,
  selected,
  onSelect,
}: {
  dueDays: DueDay[]
  today: string
  selected: Date
  onSelect: (d: Date | undefined) => void
}) {
  const t = useT()
  const counts = useMemo(() => new Map(dueDays.map((d) => [d.date, d.count])), [dueDays])
  const components = useMemo(
    () => ({ DayButton: (props: ComponentProps<typeof CalendarDayButton>) => <DueDayButton {...props} counts={counts} today={today} /> }),
    [counts, today],
  )
  return (
    <SectionCard>
      <p className="mb-3 flex items-center gap-2 text-sm font-semibold">
        <CalendarDays className="size-4 text-primary" /> {t.home.calendar.title}
      </p>
      <Calendar
        mode="single"
        locale={dateLocale()}
        selected={selected}
        onSelect={onSelect}
        weekStartsOn={1}
        components={components}
        className="w-full p-0 [--cell-size:2.15rem]"
      />
      <p className="mt-3 border-t pt-3 text-xs text-muted-foreground">{t.home.calendar.hint}</p>
    </SectionCard>
  )
}

export default function HomePage() {
  const { userName } = useSettings()
  const t = useT()
  const overview = useHomeOverview()
  const localDay = useLocalDay()
  const data = overview.data
  const today = data?.today ?? localDay
  // A day picked in the calendar; none (or today) shows the Today list.
  const [picked, setPicked] = useState<string | null>(null)
  const day = picked && picked !== today ? picked : null

  return (
    <div className="mx-auto max-w-[1100px] space-y-5">
      {/* FR-DATA-8: only on a copy with no data yet */}
      <FirstStartCard />
      {/* FR-RAG-EM-3, D-82: adding the search model, or the one-time notice without it */}
      <SearchModelNotice />

      <section className="flex flex-col items-center gap-3 pt-2 text-center">
        <LocalClock />
        <h1 className="mt-1 text-2xl font-semibold tracking-tight">
          {userName ? t.home.greetingNamed(greeting(), userName) : greeting()}
        </h1>
        {data && <CountsLine counts={data.counts} />}
      </section>

      <AskBar size="large" />
      {data && <NeedsAttention pagesNotChecked={data.counts.pagesNotChecked} />}
      {overview.error && <p className="text-sm text-destructive">{errorMessage(overview.error)}</p>}

      <div className="grid items-start gap-5 lg:grid-cols-[minmax(0,1fr)_340px]">
        {day ? (
          <DayCard key={day} day={day} onBack={() => setPicked(null)} />
        ) : data ? (
          <TodayCard groups={data.groups} today={today} />
        ) : (
          <SectionCard title={t.home.today.title}>
            <p className="py-8 text-center text-sm text-muted-foreground">{t.home.loading}</p>
          </SectionCard>
        )}
        <CalendarCard
          dueDays={data?.dueDays ?? []}
          today={today}
          selected={parseISO(day ?? today)}
          onSelect={(d) => setPicked(d ? dayKey(d) : null)}
        />
      </div>
    </div>
  )
}
