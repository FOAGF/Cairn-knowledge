import { BookA, BookOpen, CircleCheckBig, FilePlus2, FileText, FolderKanban, House, ListPlus, MessageSquareText, Settings } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { useGlossary } from '@/api/glossary'
import { type SearchHit, useHybridSearch, useSearch } from '@/api/search'
import { useSearchModelStatus } from '@/api/searchModel'
import { MeaningNotice } from '@/components/shared/MeaningNotice'
import { Snippet } from '@/components/shared/Snippet'
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from '@/components/ui/command'
import { useT } from '@/i18n'
import { useAppStore } from '@/store'

/** How long typing must pause before the slower search by meaning starts (FR-RAG-SR-5). */
const MEANING_DELAY_MS = 300

/** Marks a result that was found only by its meaning (FR-RAG-SR-4). */
function ByMeaning({ hit }: { hit: SearchHit }) {
  const t = useT()
  if (hit.matchedBy !== 'meaning') return null
  return (
    <span className="ml-2 inline-block rounded-full bg-tint-purple px-1.5 py-px align-middle text-[0.65rem] font-medium text-tint-purple-fg">
      {t.layout.palette.byMeaning}
    </span>
  )
}

/** Everything Ctrl+K searches: pages, tasks and glossary terms (FR-KN-GL-5). */
const PALETTE_KINDS = ['page', 'task', 'term'] as const

/** Ctrl+K: search tasks, pages and glossary terms, or jump anywhere (NFR-UX-2). */
export function CommandPalette() {
  const open = useAppStore((s) => s.commandOpen)
  const setOpen = useAppStore((s) => s.setCommandOpen)
  const [q, setQ] = useState('')
  // Global search (FR-KN-UI-2): keyword hits in pages and tasks, with the matching text shown.
  const query = useMemo(() => ({ text: q, kinds: [...PALETTE_KINDS], includeClosed: true, limit: 20 }), [q])
  const keyword = useSearch(query)

  // FR-RAG-SR-5: keyword results show while typing; after a pause the merged keyword + meaning
  // results replace them, if they are for the text that is in the box now.
  const [pausedText, setPausedText] = useState('')
  useEffect(() => {
    if (!q.trim()) {
      setPausedText('')
      return
    }
    const timer = setTimeout(() => setPausedText(q), MEANING_DELAY_MS)
    return () => clearTimeout(timer)
  }, [q])
  const hybridQuery = useMemo(() => ({ text: pausedText, kinds: [...PALETTE_KINDS], includeClosed: true, limit: 20 }), [pausedText])
  // D-82: without an installed search model (a lite package) Ctrl+K stays with the keyword results,
  // with no "searching by meaning" line and no warning.
  const noModel = useSearchModelStatus()?.install === 'notInstalled'
  const hybrid = useHybridSearch(hybridQuery, open && !noModel)
  const merged = !noModel && q.trim() && pausedText === q ? hybrid.data : undefined
  const meaningPending = !noModel && q.trim().length > 0 && !merged && !hybrid.isError
  const hits = merged ? merged.hits : keyword.hits
  const meaning = merged
    ? merged.meaning
    : !noModel && hybrid.isError && pausedText === q
      ? { state: 'unavailable' as const, items: 0, itemsReady: 0, detail: null }
      : undefined
  const pageHits = hits.filter((h) => h.kind === 'page')
  const taskHits = hits.filter((h) => h.kind === 'task')
  const termHits = hits.filter((h) => h.kind === 'term')
  const glossary = useGlossary().data
  const matches = (label: string) => !q.trim() || label.toLowerCase().includes(q.trim().toLowerCase())
  const navigate = useNavigate()
  const t = useT()
  const p = t.layout.palette

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setOpen(!useAppStore.getState().commandOpen)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [setOpen])

  const go = (to: string) => {
    setOpen(false)
    setQ('')
    navigate(to)
  }

  const actions: { label: string; icon: React.ReactNode; to: string }[] = [
    { label: t.common.newTask, icon: <ListPlus />, to: '/tasks?new=1' },
    { label: p.newPage, icon: <FilePlus2 />, to: '/knowledge/new' },
    { label: p.askAssistant, icon: <MessageSquareText />, to: '/assistant' },
  ].filter((a) => matches(a.label))
  const nav: { label: string; icon: React.ReactNode; to: string }[] = [
    { label: t.common.nav.home, icon: <House />, to: '/' },
    { label: t.common.nav.knowledge, icon: <BookOpen />, to: '/knowledge' },
    { label: t.common.nav.tasks, icon: <CircleCheckBig />, to: '/tasks' },
    { label: t.common.nav.categories, icon: <FolderKanban />, to: '/categories' },
    { label: t.common.nav.glossary, icon: <BookA />, to: '/glossary' },
    { label: t.common.nav.settings, icon: <Settings />, to: '/settings' },
  ].filter((a) => matches(a.label))

  return (
    <CommandDialog open={open} onOpenChange={(o) => { setOpen(o); if (!o) setQ('') }} title={t.common.search} description={p.description}>
      <Command shouldFilter={false}>
        <CommandInput value={q} onValueChange={setQ} placeholder={p.placeholder} />
        <CommandList>
          <CommandEmpty>{p.noResults}</CommandEmpty>
          {actions.length > 0 && (
            <CommandGroup heading={p.actions}>
              {actions.map((a) => (
                <CommandItem key={a.to} value={`action ${a.label}`} onSelect={() => go(a.to)}>
                  {a.icon} {a.label}
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {termHits.length > 0 && (
            <>
              <CommandSeparator />
              <CommandGroup heading={t.common.nav.glossary}>
                {termHits.map((h) => {
                  const otherName = glossary?.find((e) => e.id === h.id)?.otherNames[0]
                  return (
                    <CommandItem key={h.id} value={`term ${h.id}`} onSelect={() => go(`/glossary?term=${h.id}`)} className="items-start">
                      <BookA className="mt-0.5" />
                      <span className="min-w-0">
                        <span className="block truncate">
                          {h.title}
                          {otherName && <span className="ml-1.5 text-xs text-muted-foreground">{otherName}</span>}
                          <ByMeaning hit={h} />
                        </span>
                        {h.snippet && <Snippet text={h.snippet} className="line-clamp-2 block text-xs text-muted-foreground" />}
                      </span>
                    </CommandItem>
                  )
                })}
              </CommandGroup>
            </>
          )}
          {pageHits.length > 0 && (
            <>
              <CommandSeparator />
              <CommandGroup heading={t.common.nav.knowledge}>
                {pageHits.map((h) => (
                  <CommandItem key={h.id} value={`page ${h.id}`} onSelect={() => go(`/knowledge/${h.id}`)} className="items-start">
                    <FileText className="mt-0.5" />
                    <span className="min-w-0">
                      <span className="block truncate">{h.title}<ByMeaning hit={h} /></span>
                      {h.snippet && <Snippet text={h.snippet} className="line-clamp-2 block text-xs text-muted-foreground" />}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </>
          )}
          {taskHits.length > 0 && (
            <>
              <CommandSeparator />
              <CommandGroup heading={t.common.nav.tasks}>
                {taskHits.map((h) => (
                  <CommandItem key={h.id} value={`task ${h.id}`} onSelect={() => go(`/tasks/${h.key ?? h.id}`)} className="items-start">
                    <CircleCheckBig className="mt-0.5" />
                    <span className="min-w-0">
                      <span className="block truncate">
                        <span className="mr-1.5 font-mono text-xs text-muted-foreground">{h.key}</span>
                        {h.title}
                        <ByMeaning hit={h} />
                      </span>
                      {h.snippet && <Snippet text={h.snippet} className="line-clamp-2 block text-xs text-muted-foreground" />}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </>
          )}
          {nav.length > 0 && (
            <>
              <CommandSeparator />
              <CommandGroup heading={p.goTo}>
                {nav.map((a) => (
                  <CommandItem key={a.to} value={`go ${a.label}`} onSelect={() => go(a.to)}>
                    {a.icon} {a.label}
                  </CommandItem>
                ))}
              </CommandGroup>
            </>
          )}
        </CommandList>
        {/* A line of fixed height, so results merging in never move anything (FR-RAG-IX-6). */}
        <div className="flex h-7 items-center px-2">
          {meaningPending ? (
            <p className="text-xs text-muted-foreground">{p.searchingByMeaning}</p>
          ) : (
            <MeaningNotice meaning={meaning} />
          )}
        </div>
      </Command>
    </CommandDialog>
  )
}
