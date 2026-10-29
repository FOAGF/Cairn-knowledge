// A find list (FR-LLM-AS-2, UI-AS-2): the model's intro line, then a row per match. Tasks show
// their live status and priority from the task data, pages their group and category. The first 20
// rows show; *Show more* adds 20.
import { CircleCheckBig, FileText } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import type { FindPart } from '@/api/assistant'
import { useGroups, usePagesByIds } from '@/api/knowledge'
import { useCategoryList } from '@/api/categories'
import { groupPath } from '@/features/knowledge/groupTree'
import { ALL_TASKS, useTaskList } from '@/api/tasks'
import { CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { priorityLabel, PRIORITY_TINT, statusLabel, STATUS_TINT, TINT_CLASS } from '@/lib/labels'
import { cn } from '@/lib/utils'

const PAGE = 20

export function FindList({ find }: { find: FindPart }) {
  const [shown, setShown] = useState(PAGE)
  const tasks = useTaskList(ALL_TASKS)
  const categories = useCategoryList()
  const groups = useGroups()
  const pageIds = useMemo(() => find.items.filter((i) => i.kind === 'page').map((i) => i.id).sort(), [find.items])
  const pages = usePagesByIds(pageIds).data ?? []
  // `t` is a task below.
  const m = useT()

  return (
    <div>
      {find.intro && <p className="mb-2 text-sm">{find.intro}</p>}
      {find.items.length > 0 && (
        <ul className="divide-y overflow-hidden rounded-2xl border bg-card">
          {find.items.slice(0, shown).map((item) => {
            const category = categories.find((p) => p.id === item.categoryId)
            if (item.kind === 'task') {
              const t = tasks.find((x) => x.id === item.id)
              const key = t?.key ?? item.key ?? item.id
              return (
                <li key={`t-${item.id}`}>
                  <Link to={`/tasks/${key}`} className="flex items-center gap-2.5 px-3 py-2 text-sm hover:bg-muted/50">
                    <CircleCheckBig className="size-4 shrink-0 text-tint-green-fg" />
                    <span className="shrink-0 font-mono text-xs text-muted-foreground">{key}</span>
                    <span className="min-w-0 flex-1 truncate">{t?.title ?? item.title}</span>
                    {t && (
                      <>
                        <span className={cn('shrink-0 rounded-md px-2 py-0.5 text-[11px] font-semibold tracking-wide uppercase', TINT_CLASS[STATUS_TINT[t.status]])}>{statusLabel(t.status)}</span>
                        <span className={cn('shrink-0 rounded-md px-2 py-0.5 text-[11px] font-semibold', TINT_CLASS[PRIORITY_TINT[t.priority]])}>{priorityLabel(t.priority)}</span>
                      </>
                    )}
                  </Link>
                </li>
              )
            }
            const p = pages.find((x) => x.id === item.id)
            const group = groups.find((g) => g.id === p?.groupId)
            return (
              <li key={`p-${item.id}`}>
                <Link to={`/knowledge/${item.id}`} className="flex items-center gap-2.5 px-3 py-2 text-sm hover:bg-muted/50">
                  <FileText className="size-4 shrink-0 text-tint-purple-fg" />
                  <span className="min-w-0 flex-1 truncate">{p?.title ?? item.title}</span>
                  <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
                    {category && <CategoryDot color={category.color} className="size-2" />}
                    {[category?.name, group && groupPath(group, groups)].filter(Boolean).join(' › ')}
                  </span>
                </Link>
              </li>
            )
          })}
        </ul>
      )}
      {find.items.length > shown && (
        <Button size="sm" variant="ghost" className="mt-1" onClick={() => setShown(shown + PAGE)}>
          {m.assistant.find.showMore(find.items.length - shown)}
        </Button>
      )}
    </div>
  )
}
