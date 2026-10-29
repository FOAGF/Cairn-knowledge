// Change history (FR-TASK-HI-1..3): one version per day, with what changed vs the previous version.
import { ArrowRight } from 'lucide-react'
import { PriorityIcon, StatusBadge } from '@/components/shared/tint'
import { useT } from '@/i18n'
import { fullDate } from '@/lib/format'
import { type TaskSnapshot, useTaskVersions } from '@/api/tasks'
import { useCategoryList } from '@/api/categories'

type Change = { field: string; from?: React.ReactNode; to?: React.ReactNode }

function Arrow({ from, to }: { from?: React.ReactNode; to?: React.ReactNode }) {
  const t = useT()
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      {from ?? <span className="text-muted-foreground">{t.tasks.history.none}</span>}
      <ArrowRight className="size-3.5 text-muted-foreground" />
      {to ?? <span className="text-muted-foreground">{t.tasks.history.none}</span>}
    </span>
  )
}

export function TaskHistory({ taskId }: { taskId: string }) {
  // Oldest first (Rust).
  const versions = useTaskVersions(taskId)
  const categories = useCategoryList()
  const t = useT()
  const pname = (id: string) => categories.find((p) => p.id === id)?.name ?? id

  const diff = (prev: TaskSnapshot, cur: TaskSnapshot): Change[] => {
    const out: Change[] = []
    if (prev.title !== cur.title) out.push({ field: t.common.title, from: <span className="line-through">{prev.title}</span>, to: cur.title })
    if (prev.status !== cur.status) out.push({ field: t.common.status, from: <StatusBadge status={prev.status} />, to: <StatusBadge status={cur.status} /> })
    if (prev.priority !== cur.priority) out.push({ field: t.common.priority, from: <PriorityIcon priority={prev.priority} withLabel />, to: <PriorityIcon priority={cur.priority} withLabel /> })
    if (prev.categoryId !== cur.categoryId) out.push({ field: t.common.category, from: pname(prev.categoryId), to: pname(cur.categoryId) })
    if ((prev.dueDate ?? null) !== (cur.dueDate ?? null)) out.push({ field: t.common.dueDate, from: prev.dueDate && fullDate(prev.dueDate), to: cur.dueDate && fullDate(cur.dueDate) })
    if (prev.labels.join('|') !== cur.labels.join('|')) {
      const added = cur.labels.filter((l) => !prev.labels.includes(l))
      const removed = prev.labels.filter((l) => !cur.labels.includes(l))
      out.push({ field: t.common.labels, from: removed.length ? `− ${removed.join(', ')}` : undefined, to: added.length ? `+ ${added.join(', ')}` : undefined })
    }
    if (prev.description !== cur.description) out.push({ field: t.common.description })
    return out
  }

  if (versions.length === 0) return <p className="py-2 text-sm text-muted-foreground">{t.tasks.history.empty}</p>

  return (
    <ol className="relative space-y-5 border-l pl-5">
      {versions
        .map((v, i) => ({ v, changes: i === 0 ? null : diff(versions[i - 1].snapshot, v.snapshot) }))
        .reverse()
        .map(({ v, changes }) => (
          <li key={v.date} className="relative">
            <span className="absolute top-1.5 -left-[25px] size-2.5 rounded-full bg-primary ring-4 ring-card" />
            <p className="text-sm font-medium">{fullDate(v.date)}</p>
            {changes === null ? (
              <p className="mt-1 text-sm text-muted-foreground">{t.tasks.history.created(v.snapshot.title)}</p>
            ) : changes.length === 0 ? (
              <p className="mt-1 text-sm text-muted-foreground">{t.tasks.history.noChanges}</p>
            ) : (
              <ul className="mt-1.5 space-y-1.5">
                {changes.map((c) => (
                  <li key={c.field} className="flex flex-wrap items-center gap-2 text-sm">
                    <span className="w-24 shrink-0 text-muted-foreground">{c.field}</span>
                    {c.field === t.common.description ? <span>{t.tasks.history.edited}</span> : <Arrow from={c.from} to={c.to} />}
                  </li>
                ))}
              </ul>
            )}
          </li>
        ))}
    </ol>
  )
}
