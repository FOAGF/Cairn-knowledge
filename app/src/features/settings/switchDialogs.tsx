// The switches of Settings › General › Knowledge pages (FR-KN-EXT-8): to external mode, to another
// folder while in external mode, and to internal mode. Each explains what happens and starts the switch; the progress dialog (features/storage/KnowledgeSwitchDialog.tsx)
// takes over while it runs in the background.
import { Archive, FolderOpen } from 'lucide-react'
import { useState } from 'react'
import { errorMessage } from '@/api/client'
import { switchKnowledge, type SwitchBackup } from '@/api/storage'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { useT } from '@/i18n'
import { dateTime, largeFileSize } from '@/lib/format'

/**
 * Internal → external, or (with `current`, in external mode) from the current folder to another:
 * `root` is the folder already picked; *Change…* picks another.
 */
export function SwitchToFolderDialog({
  root,
  current,
  onPick,
  onClose,
}: {
  root: string | null
  /** The folder in use now: the dialog switches from it to `root`. */
  current?: string | null
  onPick: () => void
  onClose: () => void
}) {
  const other = !!current
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const t = useT()
  const f = t.settings.switchToFolder
  const start = async () => {
    if (!root) return
    setBusy(true)
    setError('')
    try {
      await switchKnowledge(other ? { mode: 'folder', root } : { mode: 'external', root })
      onClose()
    } catch (e) {
      setError(errorMessage(e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <Dialog open={!!root} onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="sm:max-w-lg" data-testid={other ? 'switch-folder-dialog' : 'switch-external-dialog'}>
        <DialogHeader>
          <DialogTitle>{other ? f.titleOther : f.titleExternal}</DialogTitle>
          <DialogDescription>
            {other ? f.descriptionOther : f.descriptionExternal}
          </DialogDescription>
        </DialogHeader>
        {other && (
          <p className="min-w-0 text-xs text-muted-foreground">
            {f.now(<span className="font-mono break-all">{current}</span>)}
          </p>
        )}
        {/* min-w-0: a dialog is a grid, whose rows otherwise grow to fit a long path and push the
            text out of the box. The path wraps, so the whole folder chosen stays readable. */}
        <div className="flex min-w-0 items-start gap-2">
          <div className="min-w-0 flex-1 rounded-lg border bg-muted/40 px-2.5 py-1.5 font-mono text-xs break-all" data-testid="switch-external-root">
            {root}
          </div>
          <Button type="button" variant="outline" size="sm" onClick={onPick} disabled={busy}>
            <FolderOpen /> {f.change}
          </Button>
        </div>
        <ul className="list-disc space-y-1.5 pl-5 text-sm text-muted-foreground">
          <li>{f.backupFirst}</li>
          {other ? (
            <li>{f.currentLeft}</li>
          ) : (
            <li>{f.packedAway}</li>
          )}
          <li>{f.onlyMarkdown(other)}</li>
          <li>{f.unchanged}</li>
          <li>{f.searchRebuilt}</li>
        </ul>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={busy}>
            {t.common.cancel}
          </Button>
          <Button onClick={() => void start()} disabled={busy} data-testid="switch-confirm">
            {busy ? f.starting : f.confirm}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

const NO_PAGES = '__none'

/** External → internal: the pages of a switch backup, or none. */
export function SwitchToDatabaseDialog({ open, backups, onClose }: { open: boolean; backups: SwitchBackup[]; onClose: () => void }) {
  // Only pages kept in internal mode can be loaded back; a folder's switch backups only serve that
  // folder (Rust refuses them here too).
  const internal = backups.filter((b) => b.mode === 'internal')
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {/* The body mounts on each opening, so the choice starts from the newest backup again. */}
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg" data-testid="switch-internal-dialog">
        {open && <DatabaseBody backups={internal} onClose={onClose} />}
      </DialogContent>
    </Dialog>
  )
}

function DatabaseBody({ backups, onClose }: { backups: SwitchBackup[]; onClose: () => void }) {
  // The pages kept when knowledge last went to a folder: the newest internal-mode backup (newest first).
  const [choice, setChoice] = useState(() => backups[0]?.fileName ?? NO_PAGES)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const t = useT()
  const d = t.settings.switchToDatabase
  const start = async () => {
    setBusy(true)
    setError('')
    try {
      await switchKnowledge({ mode: 'internal', backup: choice === NO_PAGES ? null : choice })
      onClose()
    } catch (e) {
      setError(errorMessage(e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <>
      <DialogHeader>
        <DialogTitle>{d.title}</DialogTitle>
        <DialogDescription>
          {d.description}
        </DialogDescription>
      </DialogHeader>
      <div className="min-w-0 space-y-2">
        <p className="text-sm font-medium">{d.question}</p>
        {backups.length === 0 && <p className="text-xs text-muted-foreground">{d.noBackup}</p>}
        <RadioGroup value={choice} onValueChange={setChoice} className="max-h-72 overflow-y-auto">
          {backups.map((b) => (
            <Label key={b.fileName} className="flex min-w-0 items-start gap-3 rounded-xl border p-3 font-normal" data-testid="switch-backup-choice">
              <RadioGroupItem value={b.fileName} className="mt-0.5" />
              <span className="min-w-0">
                <span className="block text-sm font-medium">
                  {d.choice(b.pages, dateTime(b.exportedAt))}
                </span>
                <span className="block text-xs text-muted-foreground">{d.keptThen(largeFileSize(b.size))}</span>
              </span>
            </Label>
          ))}
          <Label className="flex items-start gap-3 rounded-xl border p-3 font-normal" data-testid="switch-start-empty">
            <RadioGroupItem value={NO_PAGES} className="mt-0.5" />
            <span>
              <span className="block text-sm font-medium">{d.startEmpty}</span>
              <span className="block text-xs text-muted-foreground">{d.startEmptyHint}</span>
            </span>
          </Label>
        </RadioGroup>
        <p className="flex items-start gap-1.5 text-xs text-muted-foreground">
          <Archive className="mt-0.5 size-3.5 shrink-0" /> {d.onlyPages}
        </p>
      </div>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <DialogFooter>
        <Button variant="outline" onClick={onClose} disabled={busy}>
          {t.common.cancel}
        </Button>
        <Button onClick={() => void start()} disabled={busy} data-testid="switch-confirm">
          {busy ? d.starting : d.confirm}
        </Button>
      </DialogFooter>
    </>
  )
}
