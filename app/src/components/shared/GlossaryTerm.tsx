// A recognised glossary term in rendered text: dotted underline, and a hover card with the entry (FR-KN-GL-6).
import { BookA } from 'lucide-react'
import { useMemo, useRef, useState, type ReactNode } from 'react'
import { Link } from 'react-router'
import { type GlossaryEntry, useGlossary } from '@/api/glossary'
import { useCategoryList } from '@/api/categories'
import { buildMatcher, effectiveEntries } from '@/lib/glossary'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useT } from '@/i18n'
import { GlobalDot, CategoryDot } from './tint'

/** The matcher for text that belongs to `categoryId` (global entries + that category's, with overrides). */
export function useGlossaryMatcher(categoryId: string | null) {
  const entries = useGlossary().data
  return useMemo(() => buildMatcher(effectiveEntries(entries ?? [], categoryId)), [entries, categoryId])
}

/** Name of an entry's scope, with the category colour. */
export function ScopeLabel({ categoryId }: { categoryId: string | null }) {
  const category = useCategoryList().find((p) => p.id === categoryId)
  const t = useT().shared.glossaryTerm
  return (
    <span className="inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
      {categoryId === null ? <GlobalDot className="size-2" /> : <CategoryDot color={category?.color ?? 'slate'} className="size-2" />}
      {categoryId === null ? t.global : (category?.name ?? t.unknownCategory)}
    </span>
  )
}

export function GlossaryEntryCard({ entry }: { entry: GlossaryEntry }) {
  const t = useT().shared.glossaryTerm
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <span className="font-semibold">{entry.term}</span>
        <ScopeLabel categoryId={entry.categoryId} />
      </div>
      {entry.otherNames.length > 0 && <p className="text-xs text-muted-foreground">{t.also(entry.otherNames)}</p>}
      <p className="line-clamp-4 whitespace-pre-line">{entry.description}</p>
      <Link to={`/glossary?term=${entry.id}`} className="inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">
        <BookA className="size-3.5" /> {t.open}
      </Link>
    </div>
  )
}

/** Dotted-underlined term with a hover card. The card stays open while the pointer is over it. */
export function GlossaryTerm({ entry, children }: { entry: GlossaryEntry; children: ReactNode }) {
  const [open, setOpen] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  const show = () => {
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setOpen(true), 250)
  }
  const hide = () => {
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setOpen(false), 150)
  }
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <span
          onMouseEnter={show}
          onMouseLeave={hide}
          onClick={(e) => e.preventDefault()}
          className="cursor-help underline decoration-dotted decoration-muted-foreground underline-offset-4"
        >
          {children}
        </span>
      </PopoverTrigger>
      <PopoverContent className="w-80" onMouseEnter={show} onMouseLeave={hide} onOpenAutoFocus={(e) => e.preventDefault()}>
        <GlossaryEntryCard entry={entry} />
      </PopoverContent>
    </Popover>
  )
}
