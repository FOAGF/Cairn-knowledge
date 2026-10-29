// Right pane of the glossary screen: one entry (FR-KN-GL-1..4, UI-KN-8).
import { FileText, Link2, Pencil, SquareCheck, Trash2 } from 'lucide-react'
import { Link } from 'react-router'
import { type GlossaryEntry, useGlossaryUsage, USAGE_STEP } from '@/api/glossary'
import { useCategoryList } from '@/api/categories'
import { ScopeLabel } from '@/components/shared/GlossaryTerm'
import { SectionCard } from '@/components/shared/PageHeader'
import { TagChip } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/api/client'
import { useT } from '@/i18n'

function Prop({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      {children}
    </div>
  )
}

export function GlossaryDetail({
  entry,
  related,
  onSelect,
  onEdit,
  onDelete,
}: {
  entry: GlossaryEntry
  /** Related terms (FR-KN-GL-3): the entries of `entry.relatedIds`; Rust already applied the scope rule (D-38). */
  related: GlossaryEntry[]
  onSelect: (id: string) => void
  onEdit: () => void
  onDelete: () => void
}) {
  const categories = useCategoryList()
  const t = useT()
  // A global entry that some categories override with their own meaning.
  const overriddenIn = entry.overriddenIn.map((id) => categories.find((p) => p.id === id)?.name ?? id)

  return (
    <div className="space-y-4">
      <div className="rounded-2xl border bg-card p-6 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <h2 className="text-2xl font-semibold tracking-tight">{entry.term}</h2>
            <div className="mt-1.5 flex flex-wrap items-center gap-2">
              <ScopeLabel categoryId={entry.categoryId} />
              {entry.overridesId && (
                <span className="rounded-full bg-tint-amber px-2 py-0.5 text-xs font-medium text-tint-amber-fg">{t.glossary.detail.overrides}</span>
              )}
              {overriddenIn.length > 0 && (
                <span className="text-xs text-muted-foreground">{t.glossary.detail.overriddenIn(overriddenIn)}</span>
              )}
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" onClick={onEdit}>
              <Pencil /> {t.common.edit}
            </Button>
            <Button variant="outline" onClick={onDelete}>
              <Trash2 /> {t.common.delete}
            </Button>
          </div>
        </div>

        <div className="mt-5 space-y-5">
          {entry.otherNames.length > 0 && (
            <Prop label={t.glossary.otherNames}>
              <div className="flex flex-wrap gap-1.5">
                {entry.otherNames.map((n) => (
                  <TagChip key={n} tag={n} tint="slate" />
                ))}
              </div>
            </Prop>
          )}
          <Prop label={t.glossary.detail.description}>
            <p className="whitespace-pre-line text-sm leading-relaxed">{entry.description || <span className="text-muted-foreground italic">{t.glossary.detail.noDescription}</span>}</p>
          </Prop>
          <Prop label={t.glossary.detail.related}>
            {related.length === 0 ? (
              <p className="text-sm text-muted-foreground">{t.glossary.detail.noRelated}</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {related.map((r) => (
                  <button
                    key={r.id}
                    onClick={() => onSelect(r.id)}
                    className="inline-flex items-center gap-1 rounded-full bg-accent px-2.5 py-0.5 text-xs font-medium text-accent-foreground hover:underline"
                  >
                    <Link2 className="size-3" /> {r.term}
                    {r.categoryId && <span className="font-normal opacity-70">({categories.find((p) => p.id === r.categoryId)?.name})</span>}
                  </button>
                ))}
              </div>
            )}
          </Prop>
        </div>
      </div>

      <UsedIn key={entry.id} entryId={entry.id} />
    </div>
  )
}

/**
 * Pages and tasks that use the term (UI-KN-8): the first 3, then "Show more" adds 10 at a time.
 * Rust finds them (glossary_usage), a page of results per call, so this never waits for a full scan.
 */
function UsedIn({ entryId }: { entryId: string }) {
  const { items, total, isPending, isError, error, refetch, hasNextPage, fetchNextPage, isFetchingNextPage } = useGlossaryUsage(entryId)
  const rest = total - items.length
  const t = useT()
  return (
    <SectionCard title={t.glossary.usedIn.title(total)} description={t.glossary.usedIn.description} icon={FileText} tint="purple">
      {isPending ? (
        <p className="text-sm text-muted-foreground">{t.glossary.usedIn.looking}</p>
      ) : isError ? (
        <div className="flex items-center justify-between gap-3">
          <p className="text-sm text-destructive">{t.glossary.usedIn.loadFailed(errorMessage(error))}</p>
          <Button variant="outline" size="sm" onClick={() => void refetch()}>
            {t.common.tryAgain}
          </Button>
        </div>
      ) : total === 0 ? (
        <p className="text-sm text-muted-foreground">{t.glossary.usedIn.none}</p>
      ) : (
        <>
          <ul className="divide-y">
            {items.map((u) => (
              <li key={`${u.kind}-${u.id}`}>
                <Link to={u.kind === 'task' ? `/tasks/${u.key ?? u.id}` : `/knowledge/${u.id}`} className="flex items-start gap-3 rounded-lg px-2 py-2 hover:bg-muted">
                  {u.kind === 'task' ? <SquareCheck className="mt-0.5 size-4 shrink-0 text-tint-blue-fg" /> : <FileText className="mt-0.5 size-4 shrink-0 text-tint-purple-fg" />}
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-medium">
                      {u.key && <span className="mr-1.5 text-muted-foreground">{u.key}</span>}
                      {u.title}
                    </span>
                    <span className="line-clamp-2 text-xs text-muted-foreground">
                      {u.before}
                      <mark className="rounded bg-tint-amber px-0.5 text-foreground">{u.matched}</mark>
                      {u.after}
                    </span>
                  </span>
                </Link>
              </li>
            ))}
          </ul>
          {(rest > 0 || hasNextPage) && (
            <div className="mt-2 flex items-center justify-between gap-3 border-t pt-3">
              <span className="text-xs text-muted-foreground">
                {t.glossary.usedIn.showing(items.length, total)}
              </span>
              <Button variant="outline" size="sm" disabled={isFetchingNextPage} onClick={() => void fetchNextPage()}>
                {isFetchingNextPage ? t.glossary.usedIn.loading : t.glossary.usedIn.showMore(Math.min(rest, USAGE_STEP))}
              </Button>
            </div>
          )}
        </>
      )}
    </SectionCard>
  )
}
