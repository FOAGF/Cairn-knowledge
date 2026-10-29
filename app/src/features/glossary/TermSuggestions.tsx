// "N possible glossary terms" (FR-KN-GL-7, UI-KN-6): a collapsed card under a chat draft and in the
// review step. *Add* opens a pre-filled entry (global by default, FR-KN-GL-2); *Ignore* remembers the
// term for the category. Nothing is added without the user. The terms come already filtered by Rust
// (`checks::filter_terms`).
//
// M5 contract: the props are final. The checks agent owns this file and may refine its look; the
// chat agent only uses it.
import { BookA, ChevronRight } from 'lucide-react'
import { useState } from 'react'
import { commands, type TermSuggestion } from '@/api/bindings'
import { isApiError } from '@/api/client'
import { useCreateGlossaryEntry, useGlossary } from '@/api/glossary'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { GlossaryForm, type EntryDraft } from './GlossaryForm'

/** A term with the pages it came from, when the card gathers several pages (the bulk checks). */
export type SourcedTerm = TermSuggestion & { from?: { title: string; categoryId: string }[] }

export function TermSuggestions({
  categoryId,
  terms,
  onResolved,
  className,
}: {
  /** The category of the page the terms came from (*Ignore* is remembered there); a term's `from` overrides it. */
  categoryId: string
  terms: SourcedTerm[]
  /** A term was added or ignored: the parent drops it from its list. */
  onResolved: (term: string) => void
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const [adding, setAdding] = useState<TermSuggestion | null>(null)
  const [conflict, setConflict] = useState<string | null>(null)
  const glossary = useGlossary()
  const create = useCreateGlossaryEntry()
  const t = useT()
  // `t` is a term inside the list below: its messages are read here.
  const m = t.glossary.suggestions
  const addLabel = t.common.add
  if (!terms.length) return null

  const ignore = (t: SourcedTerm) => {
    const categories = t.from?.length ? [...new Set(t.from.map((f) => f.categoryId))] : [categoryId]
    void Promise.all(categories.map((p) => commands.ignoreGlossaryTerm(p, t.term))).then(() => onResolved(t.term))
  }
  const save = (d: EntryDraft) => {
    setConflict(null)
    create.mutate(d, {
      onSuccess: () => {
        if (adding) onResolved(adding.term)
        setAdding(null)
      },
      onError: (e) => {
        if (isApiError(e) && e.kind === 'conflict') setConflict(e.message)
      },
    })
  }

  return (
    <Collapsible open={open} onOpenChange={setOpen} className={cn('rounded-xl border bg-muted/30 px-3 py-2', className)}>
      <CollapsibleTrigger className="flex w-full items-center gap-2 text-sm font-medium text-muted-foreground hover:text-foreground">
        <ChevronRight className={cn('size-4 transition-transform', open && 'rotate-90')} />
        <BookA className="size-4" />
        {m.count(terms.length)}
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-2 space-y-2">
        {terms.map((t) => (
          <div key={t.term} className="flex items-start gap-3 rounded-lg bg-background px-3 py-2">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="font-medium">{t.term}</span>
                <Badge variant="outline" className="text-xs font-normal">{m.confidence[t.confidence]}</Badge>
              </div>
              <p className="mt-0.5 text-sm text-muted-foreground">{t.meaning || m.noMeaning}</p>
              {t.from?.length ? <p className="mt-0.5 truncate text-xs text-muted-foreground">{m.from(t.from.map((f) => f.title))}</p> : null}
            </div>
            <Button size="sm" variant="outline" onClick={() => setAdding(t)}>{addLabel}</Button>
            <Button size="sm" variant="ghost" onClick={() => ignore(t)}>{m.ignore}</Button>
          </div>
        ))}
      </CollapsibleContent>
      <Dialog open={!!adding} onOpenChange={(o) => !o && setAdding(null)}>
        <DialogContent className="sm:max-w-xl">
          <DialogTitle className="sr-only">{m.addTitle}</DialogTitle>
          {adding && (
            <GlossaryForm
              title={m.addTitle}
              initial={{ term: adding.term, otherNames: [], description: adding.meaning, categoryId: null }}
              all={glossary.data ?? []}
              saving={create.isPending}
              conflict={conflict}
              onDirty={() => setConflict(null)}
              onSave={save}
              onCancel={() => setAdding(null)}
            />
          )}
        </DialogContent>
      </Dialog>
    </Collapsible>
  )
}
