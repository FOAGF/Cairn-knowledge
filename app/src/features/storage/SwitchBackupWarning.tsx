// FR-KN-EXT-9: the switch backups are never deleted by Cairn, so every start checks their size; past
// the warning size (Settings › Backup & data) a pop-up advises a clean-up.
import { FolderOpen, HardDrive } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router'
import { commands, type SwitchBackups } from '@/api/bindings'
import { isTauri } from '@/api/client'
import { openStorageFolder } from '@/api/storage'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { largeFileSize } from '@/lib/format'

/** Once per start (React's development double-run included). */
let checked = false

export function SwitchBackupWarning() {
  const navigate = useNavigate()
  const [list, setList] = useState<SwitchBackups | null>(null)
  const w = useT().storage.backupWarning
  useEffect(() => {
    if (checked || !isTauri()) return
    checked = true
    commands.listSwitchBackups().then(
      (l) => l.overWarning && setList(l),
      (e) => console.warn('could not read the switch backups', e),
    )
  }, [])
  if (!list) return null
  return (
    <Dialog open onOpenChange={(o) => !o && setList(null)}>
      <DialogContent className="sm:max-w-md" data-testid="switch-backup-warning">
        <DialogHeader>
          <div className="mb-1 flex size-10 items-center justify-center rounded-xl bg-tint-amber text-tint-amber-fg">
            <HardDrive className="size-5" />
          </div>
          <DialogTitle>{w.title(largeFileSize(list.totalSize))}</DialogTitle>
          <DialogDescription>
            {w.body(largeFileSize(list.warnSize))}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => setList(null)}>
            {w.skip}
          </Button>
          <Button variant="ghost" onClick={() => void openStorageFolder('switchBackups')} data-testid="switch-backup-warning-open-folder">
            <FolderOpen /> {w.openFolder}
          </Button>
          <Button
            onClick={() => {
              setList(null)
              navigate('/settings/data#switch-backups')
            }}
            data-testid="open-switch-backups"
          >
            {w.openList}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
