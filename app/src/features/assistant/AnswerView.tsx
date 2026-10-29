// An answer under an assistant message (UI-AS-2, FR-LLM-AS-1, -4, -9, -11, -12): the text with
// its `[n]` markers as links, chips for the cited sources only, a collapsed "N more searched", the
// basis label, *Search all categories* and the glossary suggestions. Rust has already read the
// markers (`assistant::markers::read`), so `[n]` in the text is `answer.sources[n - 1]`.
import { ChevronRight, CircleCheckBig, FileText, Globe, Search, TriangleAlert } from 'lucide-react'
import { type ReactNode, useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { openUrl, searchAllCategories, type AnswerBasis, type SourceRef } from '@/api/assistant'
import { type ChatMessage, updateMessage } from '@/api/conversations'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useT } from '@/i18n'
import { TINT_CLASS } from '@/lib/labels'
import { cn } from '@/lib/utils'
import { TermSuggestions } from '../glossary/TermSuggestions'
import { markersToLinks } from './messages'

/** The labels that warn (their texts are in `assistant.answer.basis`). */
const BASIS_AMBER: Partial<Record<AnswerBasis, boolean>> = { not_in_notes: true, no_web_search: true }

/** The address a web chip shows next to the page title. */
function host(url: string | null): string {
  try {
    return url ? new URL(url).host.replace(/^www\./, '') : ''
  } catch {
    return ''
  }
}

/** Where a source opens: its page or task, its web address, nothing for a file part. */
function SourceLink({ source, className, children }: { source: SourceRef; className?: string; children: ReactNode }) {
  if (source.kind === 'page' && source.id) return <Link to={`/knowledge/${source.id}`} className={className}>{children}</Link>
  if (source.kind === 'task' && (source.key || source.id)) return <Link to={`/tasks/${source.key ?? source.id}`} className={className}>{children}</Link>
  if (source.kind === 'web' && source.url) {
    const url = source.url
    // Never navigate the window (D-43): the system browser opens it.
    return <button type="button" onClick={() => void openUrl(url).catch((e) => toast.error(errorMessage(e)))} className={className}>{children}</button>
  }
  return <span className={className}>{children}</span>
}

/** What the tooltip of a marker or chip says: where in the item, and the start of that piece. */
function SourceTip({ source }: { source: SourceRef }) {
  return (
    <div className="max-w-72 space-y-0.5 text-left">
      <p className="font-medium">{source.title}</p>
      {source.part && <p className="opacity-80">{source.part}</p>}
      {source.snippet && <p className="line-clamp-5 whitespace-pre-line opacity-90">{source.snippet}</p>}
    </div>
  )
}

const KIND_ICON = { page: FileText, task: CircleCheckBig, file: FileText, web: Globe }
const KIND_TINT = { page: TINT_CLASS.purple, task: TINT_CLASS.green, file: TINT_CLASS.slate, web: TINT_CLASS.blue }

function SourceChip({ source }: { source: SourceRef }) {
  const Icon = KIND_ICON[source.kind]
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <SourceLink source={source} className={cn('inline-flex max-w-72 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium', KIND_TINT[source.kind], source.kind !== 'file' && 'hover:opacity-80')}>
          <Icon className="size-3.5 shrink-0" />
          {source.key && <span className="shrink-0 font-mono whitespace-nowrap">{source.key}</span>}
          <span className="truncate font-normal">{source.title}</span>
          {source.kind === 'web' && <span className="shrink-0 font-normal opacity-70">{host(source.url)}</span>}
          {source.kind !== 'web' && source.part && <span className="hidden shrink-0 font-normal opacity-70 sm:inline">· {source.part}</span>}
        </SourceLink>
      </TooltipTrigger>
      <TooltipContent><SourceTip source={source} /></TooltipContent>
    </Tooltip>
  )
}

/** The `[n]` in the text: a small link to the source (a file part has no link). */
function Marker({ n, source }: { n: number; source: SourceRef }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <SourceLink source={source} className={cn('mx-px inline-flex min-w-4 items-center justify-center rounded bg-tint-blue px-1 align-baseline text-[0.75em] leading-4 font-semibold text-tint-blue-fg no-underline', source.kind !== 'file' && 'hover:opacity-80')}>
          {n}
        </SourceLink>
      </TooltipTrigger>
      <TooltipContent><SourceTip source={source} /></TooltipContent>
    </Tooltip>
  )
}

/** Messages whose "model ignored the web search" warning already toasted (once per message). */
const toasted = new Set<string>()
/** How long after it was written a message still counts as new for that toast. */
const FRESH_MS = 2 * 60_000

export function AnswerView({ message }: { message: ChatMessage }) {
  const answer = message.answer!
  const [searching, setSearching] = useState(false)
  const droppedNow = useRef<string[]>([])
  const t = useT()
  const web = answer.basis === 'web'
  const basis: Partial<Record<AnswerBasis, string>> = t.assistant.answer.basis
  const label = basis[answer.basis]
  const cited = answer.cited.flatMap((n) => answer.sources[n - 1] ?? [])

  // FR-LLM-AS-12: the tick was removed by this message; say so once more as a toast, only while
  // the message is new (reopening the conversation later shows just the warning on the message).
  useEffect(() => {
    if (!answer.webUnticked || toasted.has(message.id)) return
    toasted.add(message.id)
    if (Date.now() - Date.parse(message.createdAt) < FRESH_MS) toast.warning(answer.webUnticked)
  }, [answer.webUnticked, message.id, message.createdAt])

  const termResolved = (term: string) => {
    droppedNow.current = [...droppedNow.current, term]
    void updateMessage(message.id, { answer: { ...answer, terms: answer.terms.filter((t) => !droppedNow.current.includes(t.term)) } })
  }

  const widen = () => {
    setSearching(true)
    searchAllCategories(message.id)
      .catch((e) => toast.error(errorMessage(e)))
      .finally(() => setSearching(false))
  }

  const renderLink = (href: string, children: ReactNode) => {
    if (href.startsWith('cite:')) {
      const n = Number(href.slice('cite:'.length))
      const source = answer.sources[n - 1]
      return source ? <Marker n={n} source={source} /> : <>[{n}]</>
    }
    // A link the model wrote itself: the system browser opens it, and without a web search it is no source.
    if (/^https?:\/\//i.test(href))
      return (
        <button type="button" className="text-primary underline underline-offset-2" onClick={() => void openUrl(href).catch((e) => toast.error(errorMessage(e)))}>
          {children}
        </button>
      )
    return undefined
  }

  return (
    <div>
      {answer.webUnticked && (
        <p role="alert" className="mb-2 flex items-start gap-1.5 rounded-lg bg-tint-amber px-3 py-2 text-xs text-tint-amber-fg">
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" /> {answer.webUnticked}
        </p>
      )}
      {label && (
        <p className={cn('mb-1.5 inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium', BASIS_AMBER[answer.basis] ? TINT_CLASS.amber : web ? TINT_CLASS.blue : TINT_CLASS.slate)}>
          {web && <Globe className="size-3.5" />}
          {label}
        </p>
      )}
      {message.text && <MarkdownView text={markersToLinks(message.text, answer.sources.length)} className="text-sm" renderLink={renderLink} />}

      {cited.length > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <span className="text-xs text-muted-foreground">{t.assistant.answer.sources}</span>
          {cited.map((s, i) => <SourceChip key={`${s.kind}-${s.id ?? s.url ?? s.title}-${s.part ?? i}`} source={s} />)}
        </div>
      )}
      {answer.more.length > 0 && (
        <Collapsible className="mt-2">
          <CollapsibleTrigger className="group flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
            <ChevronRight className="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
            {t.assistant.answer.moreSearched(answer.more.length)}
          </CollapsibleTrigger>
          <CollapsibleContent className="mt-2 flex flex-wrap gap-2">
            {answer.more.map((s, i) => <SourceChip key={`${s.kind}-${s.id ?? s.url ?? s.title}-${s.part ?? i}`} source={s} />)}
          </CollapsibleContent>
        </Collapsible>
      )}

      {/* FR-LLM-AS-4: the question named categories outside the scope */}
      {answer.outsideScope.length > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-2 rounded-xl border bg-muted/30 px-3 py-2">
          <p className="min-w-0 flex-1 text-xs text-muted-foreground">{t.assistant.answer.outsideScope(answer.outsideScope)}</p>
          <Button size="sm" variant="outline" disabled={searching} onClick={widen}><Search /> {t.assistant.answer.searchAll}</Button>
        </div>
      )}

      {/* FR-KN-GL-7: terms from the attached document */}
      {answer.terms.length > 0 && (
        <TermSuggestions className="mt-3" categoryId={answer.termsCategoryId ?? ''} terms={answer.terms} onResolved={termResolved} />
      )}
    </div>
  )
}
