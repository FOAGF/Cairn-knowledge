// Pieces for adding the search model after installing (FR-RAG-EM-3, D-82), used by Settings › Search,
// the first-start card (FR-DATA-8), the Home notice and the top bar.
import { CircleAlert, Download } from 'lucide-react'
import { useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { openUrl } from '@/api/assistant'
import { errorMessage } from '@/api/client'
import {
  type ModelTask,
  type SearchModelStatus,
  taskPercent,
  toMB,
  useCancelSearchModelTask,
  useDownloadSearchModel,
  useSearchModelStatus,
} from '@/api/searchModel'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Progress } from '@/components/ui/progress'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'

const GEMMA_TERMS = 'https://ai.google.dev/gemma/terms'

/** The Gemma Terms of Use, opened in the system browser (the window never leaves the app, D-43). */
function TermsLink() {
  const t = useT()
  return (
    <a
      href={GEMMA_TERMS}
      className="font-medium text-primary underline underline-offset-3"
      onClick={(e) => {
        e.preventDefault()
        void openUrl(GEMMA_TERMS).catch((err) => toast.error(errorMessage(err)))
      }}
    >
      {t.searchModel.terms}
    </a>
  )
}

/**
 * The Gemma Terms of Use with an *I accept* tick (only until accepted once) and the download button.
 * `label`: *Download* in Settings, *Download in the background* on Home and the first-start card.
 */
export function SearchModelDownload({ status, label, className }: { status: SearchModelStatus; label: string; className?: string }) {
  const [ticked, setTicked] = useState(false)
  const download = useDownloadSearchModel()
  const m = useT().searchModel
  return (
    <div className={cn('flex flex-wrap items-center gap-x-4 gap-y-2', className)}>
      {status.termsAccepted ? (
        <p className="text-xs text-muted-foreground">{m.accepted(<TermsLink />)}</p>
      ) : (
        <label className="flex items-center gap-2 text-sm text-foreground">
          <Checkbox checked={ticked} onCheckedChange={(v) => setTicked(v === true)} />
          <span>{m.accept(<TermsLink />)}</span>
        </label>
      )}
      <Button size="sm" disabled={!(status.termsAccepted || ticked) || download.isPending} onClick={() => download.mutate(ticked)}>
        <Download /> {label}
      </Button>
    </div>
  )
}

/** A running download or import: what it is doing, MB done of total, a bar and *Cancel*. */
export function SearchModelProgress({ task, className }: { task: ModelTask; className?: string }) {
  const cancel = useCancelSearchModelTask()
  const t = useT()
  const m = t.searchModel
  return (
    <div className={cn('space-y-1.5', className)}>
      <div className="flex items-center justify-between gap-3 text-sm text-foreground">
        <span>{task.phase === 'checking' ? m.checking : m.downloading}</span>
        <span className="flex items-center gap-3">
          <span className="text-xs text-muted-foreground tabular-nums">{m.progress(toMB(task.doneBytes), toMB(task.totalBytes))}</span>
          <Button size="sm" variant="outline" disabled={cancel.isPending} onClick={() => cancel.mutate()}>
            {t.common.cancel}
          </Button>
        </span>
      </div>
      <Progress value={taskPercent(task)} className="h-2" />
    </div>
  )
}

/**
 * One line while the model is being added, with a thin bar, or a short line after it failed; it links
 * to Settings › Search. Nothing when no task runs. `compact` is the top bar's shorter text.
 */
export function SearchModelTaskLine({ compact = false, className }: { compact?: boolean; className?: string }) {
  const task = useSearchModelStatus()?.task
  const m = useT().searchModel
  if (!task) return null
  const failed = task.phase === 'failed'
  const pct = taskPercent(task)
  const long = failed ? m.lineFailed : task.phase === 'checking' ? m.lineChecking(pct) : m.lineDownloading(pct)
  const text = compact ? (failed ? m.compactFailed : m.compact(pct)) : long
  return (
    <Link
      to="/settings/search"
      title={compact ? long : undefined}
      className={cn('flex min-w-0 flex-col gap-1 text-xs hover:text-foreground', failed ? 'text-destructive' : 'text-muted-foreground', className)}
    >
      <span className="flex min-w-0 items-center gap-1.5">
        {failed ? <CircleAlert className="size-3.5 shrink-0" /> : <Download className="size-3.5 shrink-0" />}
        <span className="truncate">{text}</span>
      </span>
      {!failed && <Progress value={pct} className="h-0.5" />}
    </Link>
  )
}
