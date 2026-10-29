// Glossary screen, route /glossary, a top-navigation tab (UI-KN-8, §5.9, D-39). The selected entry is in the URL (`?term=<id>`),
// so "Open in glossary" links from the page view and Ctrl+K can point at it.
import { BookA } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { type GlossaryEntry, useCreateGlossaryEntry, useDeleteGlossaryEntry, useGlossary, useUpdateGlossaryEntry } from '@/api/glossary'
import { isApiError } from '@/api/client'
import { EmptyState, PageHeader } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/features/knowledge/GroupDialogs'
import { GlossaryDetail } from './GlossaryDetail'
import { GlossaryForm, type EntryDraft } from './GlossaryForm'
import { GlossaryList, inScope, matchesQuery, type ScopeFilter } from './GlossaryList'
import { useT } from '@/i18n'

/** What the right pane shows: the entry, the edit form, or the empty add form. */
type Mode = { kind: 'view' } | { kind: 'edit' } | { kind: 'new' }

const EMPTY_DRAFT: EntryDraft = { term: '', otherNames: [], description: '', categoryId: null }
const NO_ENTRIES: GlossaryEntry[] = []

export default function GlossaryPage() {
  const [params, setParams] = useSearchParams()
  const { data, isPending, isError, error, refetch } = useGlossary()
  const entries = data ?? NO_ENTRIES
  const createEntry = useCreateGlossaryEntry()
  const updateEntry = useUpdateGlossaryEntry()
  const deleteEntry = useDeleteGlossaryEntry()
  const t = useT()

  const [query, setQuery] = useState('')
  const [scope, setScope] = useState<ScopeFilter>('all')
  const [mode, setMode] = useState<Mode>({ kind: 'view' })
  const [confirmDelete, setConfirmDelete] = useState(false)
  // A `conflict` error from Rust (same term in the same scope) is also shown inside the form.
  const [conflict, setConflict] = useState<string | null>(null)

  const selectedId = params.get('term')
  const selected = entries.find((e) => e.id === selectedId) ?? null
  const select = (id: string | null) => {
    setMode({ kind: 'view' })
    setConflict(null)
    setParams(id ? { term: id } : {}, { replace: true })
  }

  const visible = useMemo(() => entries.filter((e) => matchesQuery(e, query) && inScope(e, scope)), [entries, query, scope])
  // FR-KN-GL-3: Rust computes the related ids (D-38 scope rule); here they are only looked up.
  const related = useMemo(() => (selected ? entries.filter((e) => selected.relatedIds.includes(e.id)) : []), [entries, selected])

  const onFailed = (e: unknown) => {
    if (isApiError(e) && e.kind === 'conflict') setConflict(e.message)
  }
  const save = (draft: EntryDraft) => {
    setConflict(null)
    if (mode.kind === 'edit' && selected) {
      // In a patch, "" means: make global.
      const patch = { ...draft, categoryId: draft.categoryId ?? '' }
      updateEntry.mutate({ id: selected.id, patch }, { onSuccess: () => setMode({ kind: 'view' }), onError: onFailed })
    } else {
      createEntry.mutate(draft, { onSuccess: (e) => select(e.id), onError: onFailed })
    }
  }
  const startAdd = () => {
    setMode({ kind: 'new' })
    setConflict(null)
    setParams({}, { replace: true })
  }
  const cancel = () => {
    setMode({ kind: 'view' })
    setConflict(null)
  }

  const draftFor = (e: GlossaryEntry): EntryDraft => ({ term: e.term, otherNames: e.otherNames, description: e.description, categoryId: e.categoryId })
  const saving = createEntry.isPending || updateEntry.isPending

  return (
    <div>
      <PageHeader
        icon={BookA}
        tint="purple"
        title={t.common.nav.glossary}
        description={t.glossary.description}
      />
      {isPending ? (
        <p className="py-10 text-center text-sm text-muted-foreground">{t.glossary.loading}</p>
      ) : isError ? (
        <EmptyState icon={BookA} title={t.glossary.loadFailed}>
          <p>{isApiError(error) ? error.message : String(error)}</p>
          <Button className="mt-3" variant="outline" onClick={() => void refetch()}>
            {t.common.tryAgain}
          </Button>
        </EmptyState>
      ) : (
        <div className="grid items-start gap-6 lg:grid-cols-[300px_1fr]">
          <GlossaryList total={entries.length} entries={visible} query={query} setQuery={setQuery} scope={scope} setScope={setScope} selectedId={mode.kind === 'new' ? null : selectedId} onSelect={select} onAdd={startAdd} />
          <div className="min-w-0">
            {mode.kind === 'new' ? (
              <GlossaryForm key="new" title={t.glossary.newEntry} initial={{ ...EMPTY_DRAFT, categoryId: scope !== 'all' && scope !== 'global' ? scope : null }} all={entries} saving={saving} conflict={conflict} onDirty={() => setConflict(null)} onSave={save} onCancel={cancel} />
            ) : selected && mode.kind === 'edit' ? (
              <GlossaryForm key={selected.id} title={t.glossary.editEntry(selected.term)} initial={draftFor(selected)} all={entries} editingId={selected.id} saving={saving} conflict={conflict} onDirty={() => setConflict(null)} onSave={save} onCancel={cancel} />
            ) : selected ? (
              <GlossaryDetail entry={selected} related={related} onSelect={select} onEdit={() => setMode({ kind: 'edit' })} onDelete={() => setConfirmDelete(true)} />
            ) : (
              <EmptyState icon={BookA} title={entries.length === 0 ? t.glossary.empty : selectedId ? t.glossary.notFound : t.glossary.nothingSelected}>
                {entries.length === 0 ? t.glossary.emptyHint : t.glossary.pickHint}
              </EmptyState>
            )}
          </div>
        </div>
      )}

      <ConfirmDialog
        open={confirmDelete}
        title={t.glossary.deleteTitle(selected?.term ?? '')}
        description={t.glossary.deleteDescription}
        confirmLabel={t.glossary.deleteConfirm}
        onClose={() => setConfirmDelete(false)}
        onConfirm={() => {
          if (selected) deleteEntry.mutate(selected.id, { onSuccess: () => select(null) })
          setConfirmDelete(false)
        }}
      />
    </div>
  )
}
