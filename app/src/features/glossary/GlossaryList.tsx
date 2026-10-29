// Left pane of the glossary screen: search, scope filter and the alphabetical list (UI-KN-8).
import { Plus, Search } from 'lucide-react'
import { Fragment } from 'react'
import type { GlossaryEntry } from '@/api/glossary'
import { useCategoryList } from '@/api/categories'
import { GlobalDot, CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'

/** 'all', 'global' or a category id. */
export type ScopeFilter = string

/** Heading letter: A-Z, or "#" for digits, Chinese and everything else. */
const letterOf = (term: string) => {
  const c = term.trim().charAt(0).toUpperCase()
  return /[A-Z]/.test(c) ? c : '#'
}

/**
 * D-38: which entries a scope filter shows. *All*: everything. *Global*: global entries only.
 * A category: its own entries plus the global ones that apply there (a global entry the category overrides is left out).
 */
export function inScope(e: GlossaryEntry, scope: ScopeFilter) {
  if (scope === 'all') return true
  if (scope === 'global') return e.categoryId === null
  return e.categoryId === scope || (e.categoryId === null && !e.overriddenIn.includes(scope))
}

/** Does the entry match the search text (term, other names, description)? */
export function matchesQuery(e: GlossaryEntry, query: string) {
  const q = query.trim().toLowerCase()
  return !q || [e.term, ...e.otherNames, e.description].some((s) => s.toLowerCase().includes(q))
}

export function GlossaryList({
  total,
  entries,
  query,
  setQuery,
  scope,
  setScope,
  selectedId,
  onSelect,
  onAdd,
}: {
  /** How many entries exist in all (the header count). */
  total: number
  /** Entries that match the filters. */
  entries: GlossaryEntry[]
  query: string
  setQuery: (q: string) => void
  scope: ScopeFilter
  setScope: (s: ScopeFilter) => void
  selectedId: string | null
  onSelect: (id: string) => void
  onAdd: () => void
}) {
  const categories = useCategoryList()
  const t = useT()
  const colorOf = (categoryId: string) => categories.find((p) => p.id === categoryId)?.color ?? 'slate'
  const nameOf = (categoryId: string) => categories.find((p) => p.id === categoryId)?.name ?? t.glossary.list.unknownCategory
  const sorted = [...entries].sort((a, b) => a.term.localeCompare(b.term, undefined, { sensitivity: 'base' }))

  return (
    <aside className="flex flex-col gap-3 rounded-2xl border bg-card p-3 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      <div className="flex items-center justify-between gap-2 px-1">
        <span className="text-sm font-semibold">
          {t.glossary.list.entries} <span className="font-normal text-muted-foreground">{total}</span>
        </span>
        <Button size="sm" onClick={onAdd}>
          <Plus /> {t.glossary.list.add}
        </Button>
      </div>
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t.glossary.list.search} className="h-9 rounded-xl pl-9" />
      </div>
      <Select value={scope} onValueChange={setScope}>
        <SelectTrigger className="w-full" aria-label={t.glossary.list.scopeFilter}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">{t.glossary.list.allScopes}</SelectItem>
          {/* Markers as in the rest of the glossary: a hollow ring for global, the category's colour. */}
          <SelectItem value="global">
            <GlobalDot className="size-2" /> {t.glossary.global}
          </SelectItem>
          {categories
            .filter((p) => p.status === 'active')
            .map((p) => (
              <SelectItem key={p.id} value={p.id}>
                <CategoryDot color={p.color} className="size-2" /> {p.name}
              </SelectItem>
            ))}
        </SelectContent>
      </Select>

      <div className="-mx-1 max-h-[calc(100vh-22rem)] min-h-40 overflow-y-auto border-t px-1 pt-2">
        {sorted.length === 0 && <p className="px-2 py-6 text-center text-sm text-muted-foreground">{t.glossary.list.noMatch}</p>}
        {sorted.map((e, i) => {
          const letter = letterOf(e.term)
          const newLetter = i === 0 || letterOf(sorted[i - 1].term) !== letter
          return (
            <Fragment key={e.id}>
              {newLetter && <p className="mt-2 px-2 pb-1 text-xs font-semibold text-muted-foreground first:mt-0">{letter}</p>}
              <button
                onClick={() => onSelect(e.id)}
                className={cn(
                  'flex w-full flex-col items-start gap-0.5 rounded-lg px-2 py-1.5 text-left text-sm transition-colors hover:bg-muted',
                  e.id === selectedId && 'bg-accent font-medium text-accent-foreground hover:bg-accent',
                )}
              >
                <span className="flex w-full items-center gap-2">
                  <span className="min-w-0 flex-1 truncate">{e.term}</span>
                  {e.categoryId === null && scope !== 'all' && scope !== 'global' && <span className="shrink-0 text-xs font-normal text-muted-foreground">{t.glossary.global}</span>}
                  {e.categoryId && (
                    <span className="inline-flex max-w-[45%] shrink-0 items-center gap-1 text-xs font-normal text-muted-foreground" title={nameOf(e.categoryId)}>
                      <CategoryDot color={colorOf(e.categoryId)} className="size-2" />
                      <span className="truncate">{nameOf(e.categoryId)}</span>
                    </span>
                  )}
                </span>
                {e.otherNames[0] && <span className="w-full truncate text-xs font-normal text-muted-foreground">{e.otherNames[0]}</span>}
                {e.overridesId && <span className="text-[0.7rem] font-normal text-tint-amber-fg">{t.glossary.list.overridesGlobal}</span>}
              </button>
            </Fragment>
          )
        })}
      </div>
    </aside>
  )
}
