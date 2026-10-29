// Edit-in-place form for one glossary entry (FR-KN-GL-1, FR-KN-GL-2). Used for both "Add entry" and "Edit".
import { useState } from 'react'
import type { GlossaryEntry } from '@/api/glossary'
import { useCategoryList } from '@/api/categories'
import { GlobalDot, CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { TagInput } from '@/features/knowledge/TagInput'
import { useT } from '@/i18n'

const GLOBAL = '__global'

/** What the form edits. */
export interface EntryDraft {
  term: string
  otherNames: string[]
  description: string
  /** `null` = global. */
  categoryId: string | null
}

export function GlossaryForm({
  initial,
  title,
  all,
  editingId,
  saving,
  conflict,
  onDirty,
  onSave,
  onCancel,
}: {
  initial: EntryDraft
  title: string
  /** Every entry, to find a duplicate term in the same scope. */
  all: GlossaryEntry[]
  /** The entry being edited (it does not clash with itself); undefined when adding. */
  editingId?: string
  saving?: boolean
  /** Message of a `conflict` error from Rust (e.g. the term exists in this scope), shown under the term. */
  conflict?: string | null
  /** Called on every change, so the page can clear the conflict message. */
  onDirty?: () => void
  onSave: (d: EntryDraft) => void
  onCancel: () => void
}) {
  const categories = useCategoryList().filter((p) => p.status === 'active')
  const [draft, setDraft] = useState(initial)
  const [submitted, setSubmitted] = useState(false)
  const t = useT()
  const set = (patch: Partial<EntryDraft>) => {
    setDraft((d) => ({ ...d, ...patch }))
    onDirty?.()
  }

  // Validation: term required; the same term can't exist twice in the same scope (global + category with the same name is fine).
  const term = draft.term.trim()
  const duplicate = term !== '' && all.some((e) => e.id !== editingId && e.categoryId === draft.categoryId && e.term.trim().toLowerCase() === term.toLowerCase())
  const error = conflict ?? (term === '' ? (submitted ? t.glossary.form.required : '') : duplicate ? t.glossary.form.duplicate(term) : '')

  const save = () => {
    setSubmitted(true)
    if (!term || duplicate) return
    onSave({ ...draft, term, description: draft.description.trim(), otherNames: draft.otherNames.filter((n) => n.toLowerCase() !== term.toLowerCase()) })
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault()
        save()
      }}
      className="space-y-5 rounded-2xl border bg-card p-6 shadow-[0_1px_2px_rgba(16,24,40,0.04)]"
    >
      <h2 className="text-xl font-semibold tracking-tight">{title}</h2>

      <div className="space-y-1.5">
        <Label htmlFor="gl-term">{t.glossary.form.term}</Label>
        <Input id="gl-term" autoFocus value={draft.term} onChange={(e) => set({ term: e.target.value })} placeholder={t.glossary.form.termPlaceholder} aria-invalid={!!error} />
        {error && <p className="text-sm text-destructive">{error}</p>}
      </div>

      <div className="space-y-1.5">
        <Label>{t.glossary.otherNames}</Label>
        <div className="rounded-lg border border-input px-2.5 py-2">
          <TagInput tags={draft.otherNames} suggestions={[]} onChange={(otherNames) => set({ otherNames })} placeholder={t.glossary.form.otherNamesPlaceholder} />
        </div>
      </div>

      <div className="space-y-1.5">
        <Label htmlFor="gl-desc">{t.glossary.form.description}</Label>
        <Textarea id="gl-desc" value={draft.description} onChange={(e) => set({ description: e.target.value })} rows={4} placeholder={t.glossary.form.descriptionPlaceholder} />
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label>{t.glossary.form.scope}</Label>
          <Select value={draft.categoryId ?? GLOBAL} onValueChange={(v) => set({ categoryId: v === GLOBAL ? null : v })}>
            <SelectTrigger className="w-full" aria-label={t.glossary.form.scope}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={GLOBAL}>
                <GlobalDot className="size-2" /> {t.glossary.global}
              </SelectItem>
              {categories.map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  <CategoryDot color={p.color} className="size-2" /> {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      <div className="flex justify-end gap-2 border-t pt-4">
        <Button type="button" variant="outline" onClick={onCancel}>
          {t.common.cancel}
        </Button>
        <Button type="submit" disabled={duplicate || saving}>
          {t.common.save}
        </Button>
      </div>
    </form>
  )
}
