// Small building blocks shared by the creation screens.
import { Check, ChevronRight, FileText, Info, Loader2, Minus, Search, TriangleAlert } from 'lucide-react'
import { useMemo, useState, type KeyboardEvent } from 'react'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { CategoryBadge, TagChip } from '@/components/shared/tint'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Progress } from '@/components/ui/progress'
import { usePage, usePageList, type PageSummary } from '@/api/knowledge'
import { useSettings } from '@/api/settings'
import { type MeaningStatus, rankedByMeaning } from '@/api/search'
import { MeaningNotice } from '@/components/shared/MeaningNotice'
import type { Match } from '@/lib/search'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import type { StepState } from './usePipeline'
import { useCategoryList } from '@/api/categories'

// ------------------------------------------------------------------- Stepper

/** The four pipeline steps (§5.4). */
export function Stepper({ steps, lastLabel }: { steps: StepState[]; lastLabel?: string }) {
  const t = useT()
  const labels = [t.creation.steps.topic, t.creation.steps.group, t.creation.steps.duplicates, t.common.review]
  return (
    <ol className="flex flex-wrap items-center gap-x-2 gap-y-3 rounded-2xl border bg-card p-4 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      {steps.map((s, i) => (
        <li key={i} className="flex items-center gap-2">
          <span
            className={cn(
              'flex size-7 items-center justify-center rounded-full text-xs font-semibold',
              s === 'done' && 'bg-tint-green text-tint-green-fg',
              s === 'active' && 'bg-primary text-primary-foreground',
              s === 'skipped' && 'bg-tint-slate text-tint-slate-fg',
              s === 'pending' && 'border text-muted-foreground',
            )}
          >
            {s === 'done' ? <Check className="size-4" /> : s === 'skipped' ? <Minus className="size-4" /> : s === 'active' && i < 3 ? <Loader2 className="size-4 animate-spin" /> : i + 1}
          </span>
          <span className="leading-tight">
            <span className={cn('block text-sm font-medium', s === 'pending' && 'text-muted-foreground')}>
              {i === 3 && lastLabel ? lastLabel : labels[i]}
            </span>
            {s === 'skipped' && <span className="block text-xs text-muted-foreground">{t.common.skipped}</span>}
          </span>
          {i < 3 && <ChevronRight className="mx-1 size-4 text-muted-foreground/60" />}
        </li>
      ))}
    </ol>
  )
}

// ----------------------------------------------------------------- TagsInput

export function TagsInput({ tags, onChange }: { tags: string[]; onChange: (t: string[]) => void }) {
  const [text, setText] = useState('')
  const t = useT()
  const add = () => {
    const tag = text.trim().replace(/,$/, '')
    if (tag && !tags.includes(tag)) onChange([...tags, tag])
    setText('')
  }
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault()
      add()
    } else if (e.key === 'Backspace' && !text && tags.length) onChange(tags.slice(0, -1))
  }
  return (
    <div className="flex min-h-9 flex-wrap items-center gap-1.5 rounded-lg border bg-background px-2 py-1">
      {tags.map((t) => (
        <TagChip key={t} tag={t} onRemove={() => onChange(tags.filter((x) => x !== t))} />
      ))}
      <input
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={onKey}
        onBlur={add}
        placeholder={tags.length ? '' : t.creation.tagsInput.placeholder}
        className="min-w-24 flex-1 bg-transparent py-0.5 text-sm outline-none placeholder:text-muted-foreground"
      />
    </div>
  )
}

// ------------------------------------------------------------ Page dialogs

export function PagePreviewDialog({ page: summary, onClose }: { page: PageSummary | null; onClose: () => void }) {
  const page = usePage(summary?.id).data
  const category = useCategoryList().find((p) => p.id === summary?.categoryId)
  return (
    <Dialog open={!!summary} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{summary?.title}</DialogTitle>
          <DialogDescription asChild>
            <div className="flex flex-wrap items-center gap-2">
              <CategoryBadge category={category} />
              {summary?.tags.map((t) => <TagChip key={t} tag={t} />)}
            </div>
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-[60vh] overflow-y-auto rounded-xl bg-muted/40 p-4">
          {page && <MarkdownView text={page.body} />}
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** Searchable list of pages, used to pick a merge target (FR-TASK-KN-4). */
export function PagePickerDialog({ open, onClose, onPick }: { open: boolean; onClose: () => void; onPick: (p: PageSummary) => void }) {
  const pages = usePageList()
  const categories = useCategoryList()
  const [q, setQ] = useState('')
  const t = useT()
  const list = useMemo(
    () => pages.filter((p) => `${p.title} ${p.tags.join(' ')}`.toLowerCase().includes(q.toLowerCase())),
    [pages, q],
  )
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t.creation.pagePicker.title}</DialogTitle>
          <DialogDescription>{t.creation.pagePicker.description}</DialogDescription>
        </DialogHeader>
        <div className="relative">
          <Search className="absolute top-2.5 left-3 size-4 text-muted-foreground" />
          <Input autoFocus className="pl-9" placeholder={t.creation.pagePicker.search} value={q} onChange={(e) => setQ(e.target.value)} />
        </div>
        <div className="max-h-80 space-y-1 overflow-y-auto">
          {list.map((p) => (
            <button key={p.id} type="button" onClick={() => onPick(p)} className="flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left hover:bg-muted">
              <FileText className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{p.title}</span>
                <CategoryBadge category={categories.find((x) => x.id === p.categoryId)} />
              </span>
            </button>
          ))}
          {!list.length && <p className="py-6 text-center text-sm text-muted-foreground">{t.creation.pagePicker.empty}</p>}
        </div>
      </DialogContent>
    </Dialog>
  )
}

// ------------------------------------------------------- Duplicate matches

/** FR-KN-DU-1..2: up to 3 similar pages with a similarity bar. */
export function DuplicateMatches({
  matches,
  loading,
  offline,
  mergeLabel,
  onMerge,
  footer,
  meaning,
}: {
  /** `likely` (from `find_duplicates`) says "likely the same topic"; without it the Settings score decides. */
  matches: (Match & { likely?: boolean })[]
  loading?: boolean
  offline?: boolean
  mergeLabel?: string
  onMerge: (p: PageSummary) => void
  footer?: React.ReactNode
  /** Set when the scores are cosine similarities from search by meaning ("Find similar pages"): shows
   *  the likely-duplicate label and the FR-RAG-IX-6 warning. Unset for the keyword-based draft check. */
  meaning?: MeaningStatus
}) {
  // Without the meaning index (or without a search model, D-82) the matches are keyword matches:
  // their scores aren't similarities.
  const showScore = !meaning || rankedByMeaning(meaning)
  const t = useT()
  const categories = useCategoryList()
  const likelyScore = useSettings().duplicateScore ?? 0.8
  const isLikely = (m: Match & { likely?: boolean }) => m.likely ?? (!!meaning && showScore && m.score >= likelyScore)
  const [preview, setPreview] = useState<PageSummary | null>(null)
  if (loading)
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" /> {t.creation.matches.looking}
      </p>
    )
  return (
    <div className="space-y-3">
      {offline && (
        <Badge variant="secondary" className="rounded-full">
          {t.creation.matches.offline}
        </Badge>
      )}
      <MeaningNotice meaning={meaning} />
      {!matches.length && <p className="text-sm text-muted-foreground">{t.creation.matches.none}</p>}
      {matches.map((m) => (
        <div key={m.page.id} className="rounded-xl border bg-background p-3">
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0">
              <p className="truncate text-sm font-medium">{m.page.title}</p>
              <CategoryBadge category={categories.find((p) => p.id === m.page.categoryId)} />
            </div>
            <span className="flex shrink-0 items-center gap-2">
              {isLikely(m) ? (
                <Badge className="bg-tint-amber text-tint-amber-fg">{t.creation.matches.likely}</Badge>
              ) : (
                <Badge variant="outline" className="font-normal">{t.creation.matches.related}</Badge>
              )}
              {showScore && <span className="text-sm font-semibold tabular-nums text-primary">{Math.round(m.score * 100)}%</span>}
            </span>
          </div>
          {showScore ? <Progress value={m.score * 100} className="my-2 h-1.5" /> : <div className="h-2" />}
          <p className="line-clamp-2 text-xs text-muted-foreground">{m.snippet}</p>
          <div className="mt-3 flex gap-2">
            <Button size="sm" variant="outline" onClick={() => setPreview(m.page)}>
              {t.creation.matches.open}
            </Button>
            {/* FR-KN-DU-1: *Merge…* is the obvious next step for a likely duplicate; for a related page it stays possible, but quieter. */}
            <Button size="sm" variant={isLikely(m) ? 'default' : 'ghost'} onClick={() => onMerge(m.page)}>
              {mergeLabel ?? t.creation.matches.mergeIntoThis}
            </Button>
          </div>
        </div>
      ))}
      {footer}
      <PagePreviewDialog page={preview} onClose={() => setPreview(null)} />
    </div>
  )
}

// ---------------------------------------------------------------- CheckCard

/** Card for one pipeline check, with a pastel icon and a status line. */
export function CheckCard({
  icon: Icon,
  tint,
  title,
  status,
  children,
}: {
  icon: React.ComponentType<{ className?: string }>
  tint: 'blue' | 'green' | 'purple' | 'amber' | 'rose' | 'slate'
  title: string
  status?: React.ReactNode
  children?: React.ReactNode
}) {
  const bg = { blue: 'bg-tint-blue text-tint-blue-fg', green: 'bg-tint-green text-tint-green-fg', purple: 'bg-tint-purple text-tint-purple-fg', amber: 'bg-tint-amber text-tint-amber-fg', rose: 'bg-tint-rose text-tint-rose-fg', slate: 'bg-tint-slate text-tint-slate-fg' }[tint]
  return (
    <section className="rounded-2xl border bg-card p-4 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      <div className="flex items-center gap-3">
        <span className={cn('flex size-8 shrink-0 items-center justify-center rounded-lg', bg)}>
          <Icon className="size-4" />
        </span>
        <h3 className="flex-1 text-sm font-semibold">{title}</h3>
        {status}
      </div>
      {children && <div className="mt-3">{children}</div>}
    </section>
  )
}

/** A check failed for another reason than a missing model: *Retry* or *Skip* it (FR-KN-SV-1). */
export function CheckError({ message, onRetry, onSkip }: { message: string; onRetry: () => void; onSkip: () => void }) {
  const t = useT()
  return (
    <div role="alert" className="flex flex-wrap items-center gap-3 rounded-2xl border border-destructive/40 bg-destructive/5 p-4 text-sm">
      <TriangleAlert className="size-4 shrink-0 text-destructive" />
      <p className="min-w-0 flex-1">
        {t.creation.checkError.failed(message)}
      </p>
      <Button size="sm" onClick={onRetry}>{t.creation.checkError.retry}</Button>
      <Button size="sm" variant="outline" onClick={onSkip}>{t.creation.checkError.skip}</Button>
    </div>
  )
}

/** FR-KN-SV-3: the topic and group checks were skipped because no model can be used. */
export function UnavailableNote({ message }: { message: string }) {
  const t = useT()
  return (
    <p className="flex items-start gap-2 rounded-xl bg-muted/50 px-3 py-2 text-xs text-muted-foreground">
      <Info className="mt-0.5 size-3.5 shrink-0" />
      <span>{t.creation.unavailable(message)}</span>
    </p>
  )
}

export function Waiting({ text }: { text: string }) {
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
      <Loader2 className="size-3.5 animate-spin" /> {text}
    </span>
  )
}
