// FR-KN-EXT-11: in external mode a page's file can change outside Cairn while it is edited. *Save*
// then finds a different file and this dialog asks which version stays: *Keep mine*, *Take the disk
// version*, or first *Show differences* (the file as it is now against the editor's text, with the
// Assistant's diff view).
import { FileWarning, Loader2 } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { commands, type Page } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { folderPath } from '@/api/storage'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { PageDiff } from '@/features/assistant/PageDiff'
import { useT } from '@/i18n'

/**
 * The text as the user knows it, for the diff only: the index's page links `[Title](page:<id>)` as
 * `[[Title]]`, and `folder:` targets as their paths from the knowledge folder.
 */
function readable(body: string): string {
  return body
    .replace(/(?<!!)\[([^\]\n]*)\]\(page:[^)\s]+\)/g, '[[$1]]')
    .replace(/\]\((folder:[^)\s]+)\)/g, (_m, target: string) => `](${folderPath(target)})`)
}

export function SaveConflictDialog({
  open,
  pageId,
  mine,
  busy,
  onKeepMine,
  onTakeDisk,
  onClose,
}: {
  open: boolean
  pageId: string
  /** The editor's title and whole body, as *Save* would write them. */
  mine: { title: string; body: string }
  /** *Keep mine* or *Take the disk version* is running. */
  busy: boolean
  onKeepMine: () => void
  onTakeDisk: () => void
  /** Back to editing, nothing decided. */
  onClose: () => void
}) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && !busy && onClose()}>
      {/* The body mounts on each opening, so the differences are read again. */}
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl" data-testid="save-conflict-dialog">
        {open && <ConflictBody pageId={pageId} mine={mine} busy={busy} onKeepMine={onKeepMine} onTakeDisk={onTakeDisk} onClose={onClose} />}
      </DialogContent>
    </Dialog>
  )
}

function ConflictBody({ pageId, mine, busy, onKeepMine, onTakeDisk, onClose }: Omit<Parameters<typeof SaveConflictDialog>[0], 'open'>) {
  // undefined: not asked for; null: the file is gone.
  const [disk, setDisk] = useState<Page | null | undefined>(undefined)
  const [loading, setLoading] = useState(false)
  const c = useT().knowledge.conflict
  const showDiff = () => {
    setLoading(true)
    commands.getPageOnDisk(pageId).then(
      (p) => {
        setDisk(p)
        setLoading(false)
      },
      (e) => {
        toast.error(errorMessage(e))
        setLoading(false)
      },
    )
  }
  return (
    <>
      <DialogHeader>
        <div className="mb-1 flex size-10 items-center justify-center rounded-xl bg-tint-amber text-tint-amber-fg">
          <FileWarning className="size-5" />
        </div>
        <DialogTitle>{c.title}</DialogTitle>
        <DialogDescription>{c.description}</DialogDescription>
      </DialogHeader>
      <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
        <li>{c.keepMineExplain(<span className="font-medium text-foreground">{c.keepMine}</span>)}</li>
        <li>{c.takeDiskExplain(<span className="font-medium text-foreground">{c.takeDisk}</span>)}</li>
      </ul>

      {disk === undefined ? (
        <Button variant="outline" size="sm" className="justify-self-start" onClick={showDiff} disabled={loading} data-testid="conflict-show-diff">
          {loading && <Loader2 className="animate-spin" />} {c.showDifferences}
        </Button>
      ) : disk === null ? (
        <p className="rounded-xl border p-3 text-sm text-muted-foreground" data-testid="conflict-diff">
          {c.fileGone}
        </p>
      ) : (
        <div className="space-y-2" data-testid="conflict-diff">
          <p className="text-xs text-muted-foreground">{c.diffHint}</p>
          {disk.title !== mine.title && (
            <p className="text-sm">{c.titles(<span className="font-medium">{disk.title}</span>, <span className="font-medium">{mine.title}</span>)}</p>
          )}
          {disk.body === mine.body ? (
            <p className="text-sm text-muted-foreground">{c.sameText}</p>
          ) : (
            <PageDiff oldBody={readable(disk.body)} newBody={readable(mine.body)} />
          )}
        </div>
      )}

      <DialogFooter>
        <Button variant="ghost" onClick={onClose} disabled={busy}>
          {c.backToEditing}
        </Button>
        <Button variant="outline" onClick={onTakeDisk} disabled={busy} data-testid="conflict-take-disk">
          {c.takeDisk}
        </Button>
        <Button onClick={onKeepMine} disabled={busy} data-testid="conflict-keep-mine">
          {busy ? c.saving : c.keepMine}
        </Button>
      </DialogFooter>
    </>
  )
}
