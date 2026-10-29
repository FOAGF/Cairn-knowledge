// /knowledge with no page selected: per-category cards (groups + recent pages).
import { FileText, FolderOpen, ScanSearch } from 'lucide-react'
import { Link, useLocation } from 'react-router'
import { EmptyState } from '@/components/shared/PageHeader'
import { CategoryIcon, categoryTint, TagChip, TintIcon } from '@/components/shared/tint'
import { useT } from '@/i18n'
import { timeAgo } from '@/lib/format'
import { useGroups, type PageSummary } from '@/api/knowledge'
import { cn } from '@/lib/utils'
import { useCategoryList } from '@/api/categories'
import { groupPath } from './groupTree'

export function KnowledgeOverview({ pages }: { pages: PageSummary[] }) {
  const categories = useCategoryList()
  const groups = useGroups()
  const { search } = useLocation()
  const unchecked = pages.filter((p) => p.checkStatus === 'not_checked')
  const cards = categories.filter((p) => p.status === 'active' && pages.some((x) => x.categoryId === p.id))
  const k = useT().knowledge
  const o = k.overview

  if (pages.length === 0) return <EmptyState icon={FileText} title={o.noPages}>{o.noPagesHint}</EmptyState>

  return (
    <div className="space-y-5">
      {unchecked.length > 0 && (
        <div className="flex items-center gap-3 rounded-2xl border bg-tint-amber/50 p-4">
          <TintIcon icon={ScanSearch} tint="amber" />
          <p className="flex-1 text-sm">
            {k.sentences(
              <span className="font-semibold">{o.notCheckedYet(unchecked.length)}</span>,
              <span className="text-muted-foreground">{o.runChecksHint}</span>,
            )}
          </p>
          <Link to="/knowledge/new?check=unchecked" className="text-sm font-medium text-primary">
            {o.checkNow}
          </Link>
        </div>
      )}
      <div className="grid gap-5 xl:grid-cols-2">
        {cards.map((category) => {
          const projPages = pages.filter((p) => p.categoryId === category.id)
          const recent = [...projPages].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).slice(0, 3)
          const projGroups = groups.filter((g) => g.categoryId === category.id)
          const ungrouped = projPages.filter((p) => !projGroups.some((g) => g.id === p.groupId)).length
          return (
            <section key={category.id} className={cn('rounded-2xl border p-5', categoryTint(category.color, true).className)} style={categoryTint(category.color, true).style}>
              <div className="mb-4 flex items-center gap-3">
                <CategoryIcon icon={FolderOpen} color={category.color} size="lg" />
                <div>
                  <h2 className="font-semibold">{category.name}</h2>
                  <p className="text-sm text-muted-foreground">{o.counts(projPages.length, projGroups.length)}</p>
                </div>
              </div>
              <div className="mb-4 flex flex-wrap gap-1.5">
                {projGroups.map((g) => (
                  <span key={g.id} className="inline-flex items-center gap-1 rounded-full bg-card px-2.5 py-0.5 text-xs font-medium">
                    {groupPath(g, projGroups)}
                    <span className="text-muted-foreground">{projPages.filter((p) => p.groupId === g.id).length}</span>
                  </span>
                ))}
                {ungrouped > 0 && (
                  <span className="inline-flex items-center gap-1 rounded-full border border-dashed px-2.5 py-0.5 text-xs text-muted-foreground">
                    {o.ungroupedCount(ungrouped)}
                  </span>
                )}
              </div>
              <ul className="space-y-2">
                {recent.map((p) => (
                  <li key={p.id}>
                    <Link to={`/knowledge/${p.id}${search}`} className="flex items-center gap-3 rounded-xl border bg-card p-3 transition-shadow hover:shadow-md">
                      <FileText className="size-4 shrink-0 text-muted-foreground" />
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">{p.title}</p>
                        <p className="text-xs text-muted-foreground">{o.updated(timeAgo(p.updatedAt))}</p>
                      </div>
                      {p.tags.slice(0, 1).map((t) => (
                        <TagChip key={t} tag={t} />
                      ))}
                    </Link>
                  </li>
                ))}
              </ul>
            </section>
          )
        })}
      </div>
    </div>
  )
}
