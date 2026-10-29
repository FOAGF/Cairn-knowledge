// FR-KN-EXT-8: a switch between Cairn's database and a folder runs in the background while the window
// waits. This dialog shows its steps and can't be closed until the switch finished; then it says
// what happened. Rust sends the steps as `KnowledgeProgress` events (api/storage.ts).
import { CircleCheck, Loader2, TriangleAlert } from 'lucide-react'
import { closeSwitchRun, progressText, useSwitchRun } from '@/api/storage'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Progress } from '@/components/ui/progress'
import { useT } from '@/i18n'
import { fileSize } from '@/lib/format'

export function KnowledgeSwitchDialog() {
  const { progress: p, to } = useSwitchRun()
  const t = useT()
  const d = t.storage.switchDialog
  if (!p) return null
  const failed = p.finished && !!p.error
  const percent = p.total > 0 ? Math.min(100, Math.round((p.done / p.total) * 100)) : null
  const result = p.switched
  const scan = result?.scan ?? p.scan
  // Pages now in the index: those put back from an earlier switch backup (a folder used before, whose
  // unchanged files the scan then doesn't count), plus new files, less files that are gone.
  const pagesNow = scan ? Math.max(0, (result?.restored ?? 0) + scan.added - scan.removed) : 0
  const title = !p.finished
    ? to === 'external'
      ? d.toExternal
      : to === 'folder'
        ? d.toFolder
        : to === 'internal'
          ? d.toInternal
        : d.switching
    : failed
      ? d.failed
      : to === 'internal'
        ? d.nowInternal
        : to === 'folder'
          ? d.nowFolder
          : d.nowExternal

  return (
    <Dialog open onOpenChange={(o) => !o && p.finished && closeSwitchRun()}>
      <DialogContent className="sm:max-w-lg" showCloseButton={p.finished} data-testid="knowledge-progress-dialog">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            {!p.finished ? (
              <Loader2 className="size-5 animate-spin text-primary" />
            ) : failed ? (
              <TriangleAlert className="size-5 text-destructive" />
            ) : (
              <CircleCheck className="size-5 text-tint-green-fg" />
            )}
            {title}
          </DialogTitle>
          {!p.finished && <DialogDescription>{d.keepOpen}</DialogDescription>}
        </DialogHeader>

        {!p.finished && (
          <div className="space-y-2">
            <p className="text-sm" data-testid="knowledge-progress-step">
              {progressText(p)}…
            </p>
            {percent !== null ? <Progress value={percent} className="h-2" /> : <div className="h-2 w-full animate-pulse rounded-full bg-muted" />}
          </div>
        )}

        {failed && (
          <p role="alert" className="text-sm text-destructive" data-testid="knowledge-progress-error">
            {p.error}
          </p>
        )}

        {p.finished && !failed && (
          <ul className="list-disc space-y-1.5 pl-5 text-sm text-muted-foreground" data-testid="knowledge-progress-result">
            {result && (
              <li>
                {d.backupKept(<span className="break-all text-foreground">{result.backup.fileName}</span>, fileSize(result.backup.size))}
              </li>
            )}
            {to === 'internal' ? (
              <li>{d.internalResult(result?.restored ?? 0)}</li>
            ) : (
              <>
                {scan && <li>{d.pagesInFolder(pagesNow)}</li>}
                {result && result.restored > 0 && <li>{d.detailsBack(result.restored)}</li>}
                {scan && scan.skipped.length > 0 && <li>{d.skipped(scan.skipped.length)}</li>}
                {to === 'folder' && <li>{d.previousLeft}</li>}
              </>
            )}
            <li>{d.searchRebuilt}</li>
          </ul>
        )}

        {p.finished && (
          <DialogFooter>
            <Button onClick={closeSwitchRun} data-testid="knowledge-progress-close">
              {t.common.close}
            </Button>
          </DialogFooter>
        )}
      </DialogContent>
    </Dialog>
  )
}
