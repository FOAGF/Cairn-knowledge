// Where knowledge pages live (spec §5.11, D-72), in two places of Settings:
// - General › *Knowledge pages* (`KnowledgeStorageEntry`): the mode and folder, the switch in either
//   direction (FR-KN-EXT-8), *Rescan* and *Open folder* in external mode, and what keeps the folder
//   from being read or watched;
// - Backup & data › *Switch backups* (`SwitchBackupsCard`): the backups every switch makes and their
//   warning size (FR-KN-EXT-9; the start-up pop-up's *Open switch backups* lands on this list).
import { Database, FolderOpen, FolderSync, HardDrive, RefreshCw, Trash2, TriangleAlert } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useLocation } from 'react-router'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { useSettings, useUpdateSettings } from '@/api/settings'
import {
  type KnowledgeProgress,
  openStorageFolder,
  progressText,
  refreshSwitchBackups,
  rescanKnowledgeFolder,
  type SwitchBackup,
  useDeleteSwitchBackup,
  useKnowledgeStorage,
  useScanRun,
  useSwitchBackups,
} from '@/api/storage'
import { SectionCard } from '@/components/shared/PageHeader'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'
import { dateTime, largeFileSize } from '@/lib/format'
import { pickFolder } from '@/lib/pickFolder'
import { SettingRow } from './parts'
import { SwitchToDatabaseDialog, SwitchToFolderDialog } from './switchDialogs'

/** A row of General's list (plus what to say under it); the switch dialogs open from here. */
export function KnowledgeStorageEntry() {
  const storage = useKnowledgeStorage().data
  const list = useSwitchBackups().data
  const settings = useSettings()
  const scan = useScanRun()
  const [folder, setFolder] = useState<string | null>(null)
  // External mode: another folder picked, to switch to.
  const [otherFolder, setOtherFolder] = useState<string | null>(null)
  const [toDatabase, setToDatabase] = useState(false)
  // *Rescan* asked for here, and the scan event shown when it was asked (an earlier scan's).
  const [rescan, setRescan] = useState<{ from: KnowledgeProgress | null } | null>(null)
  const external = (storage?.mode ?? settings.knowledgeMode) === 'external'
  const t = useT()
  const k = t.settings.knowledgeStorage

  const pick = async () => {
    const f = await pickFolder(t.storage.pickFolder.knowledge)
    if (f) setFolder(f)
  }
  const pickOther = async () => {
    const f = await pickFolder(t.storage.pickFolder.anotherKnowledge)
    if (f) setOtherFolder(f)
  }
  const startRescan = () => {
    setRescan({ from: scan })
    rescanKnowledgeFolder().catch((e) => {
      setRescan(null)
      toast.error(errorMessage(e))
    })
  }
  // The scan asked for here: running, or what it found once it finished.
  const mine = rescan && scan !== rescan.from ? scan : null
  const scanning = !!rescan && !mine?.finished
  const skipped = external && scan?.finished ? (scan.scan?.skipped ?? []) : []

  // In General's divided list: the wrapper is the list item, so the row itself has no padding.
  return (
    <div className="py-3.5 last:pb-0" data-testid="knowledge-storage-card">
      <SettingRow
        className="py-0"
        label={external ? k.labelExternal : k.labelInternal}
        hint={
          external ? (
            <>
              <span className="block font-mono break-all text-foreground" data-testid="knowledge-root">
                {storage?.root}
              </span>
              {k.hintExternal}
            </>
          ) : (
            k.hintInternal
          )
        }
      >
        {storage &&
          (external ? (
            <>
              {storage.rootExists && (
                <Button variant="outline" onClick={() => void openStorageFolder('knowledge')} data-testid="storage-open-folder">
                  <FolderOpen /> {k.openFolder}
                </Button>
              )}
              <Button variant="outline" onClick={startRescan} disabled={scanning} data-testid="storage-rescan">
                <RefreshCw className={scanning ? 'animate-spin' : undefined} /> {scanning ? k.reading : k.rescan}
              </Button>
              <Button variant="outline" onClick={() => void pickOther()} data-testid="storage-change-folder">
                <FolderSync /> {k.changeFolder}
              </Button>
              <Button variant="outline" onClick={() => setToDatabase(true)} data-testid="storage-switch-internal">
                <Database /> {k.switchToInternal}
              </Button>
            </>
          ) : (
            <Button variant="outline" onClick={() => void pick()} data-testid="storage-switch-external">
              <FolderOpen /> {k.switchToExternal}
            </Button>
          ))}
      </SettingRow>

      {external && storage && !storage.rootExists && (
        <Alert variant="destructive" className="mt-4" data-testid="knowledge-root-missing">
          <TriangleAlert />
          <AlertTitle>{k.missingTitle}</AlertTitle>
          <AlertDescription>
            {k.missingBody}
          </AlertDescription>
        </Alert>
      )}

      {external && storage?.rootExists && storage.watchError && (
        <Alert className="mt-4" data-testid="knowledge-watch-error">
          <TriangleAlert />
          <AlertTitle>{t.storage.notWatched.title}</AlertTitle>
          <AlertDescription>{k.watchError(storage.watchError)}</AlertDescription>
        </Alert>
      )}

      {external && mine && (
        <p className="mt-3 text-sm text-muted-foreground" role="status" data-testid="rescan-status">
          {!mine.finished
            ? `${progressText(mine)}…`
            : mine.error
              ? k.readFailed(mine.error)
              : mine.scan
                ? k.readAgain(mine.scan.added, mine.scan.changed, mine.scan.renamed, mine.scan.removed)
                : k.readAgainDone}
        </p>
      )}

      {skipped.length > 0 && (
        <div className="mt-4 border-t pt-4" data-testid="knowledge-skipped">
          <p className="mb-1 text-sm font-medium">{k.skippedTitle(skipped.length)}</p>
          <p className="mb-2 text-xs text-muted-foreground">{k.skippedHint}</p>
          <ul className="max-h-40 divide-y overflow-y-auto rounded-lg border text-xs">
            {skipped.map((s) => (
              <li key={s.path} className="flex flex-wrap gap-x-3 px-3 py-1.5">
                <span className="min-w-0 flex-1 font-mono break-all">{s.path}</span>
                <span className="text-muted-foreground">{s.reason}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <SwitchToFolderDialog root={folder} onPick={() => void pick()} onClose={() => setFolder(null)} />
      <SwitchToFolderDialog root={otherFolder} current={storage?.root} onPick={() => void pickOther()} onClose={() => setOtherFolder(null)} />
      <SwitchToDatabaseDialog open={toDatabase} backups={list?.backups ?? []} onClose={() => setToDatabase(false)} />
    </div>
  )
}

/** Settings › Backup & data: the backups made before every switch (FR-KN-EXT-9). */
export function SwitchBackupsCard() {
  const list = useSwitchBackups().data
  const settings = useSettings()
  const update = useUpdateSettings()
  const deleteBackup = useDeleteSwitchBackup()
  const [deleting, setDeleting] = useState<SwitchBackup | null>(null)
  // Typed as text, so "0." can be typed; only a size above 0 is saved.
  const [warnText, setWarnText] = useState<string | null>(null)
  const warnOk = warnText === null || (Number(warnText) > 0 && Number.isFinite(Number(warnText)))
  const backups = list?.backups ?? []
  const t = useT()
  const s = t.settings.switchBackups

  // *Open switch backups* (the start-up pop-up) comes here with #switch-backups.
  const { hash } = useLocation()
  const listShown = list !== undefined
  useEffect(() => {
    if (hash === '#switch-backups' && listShown) document.getElementById('switch-backups')?.scrollIntoView({ block: 'start' })
  }, [hash, listShown])

  return (
    <div id="switch-backups" className="scroll-mt-24" data-testid="switch-backups">
      <SectionCard
        icon={HardDrive}
        tint="purple"
        title={s.title}
        description={s.description}
        action={
          <Button size="sm" variant="outline" onClick={() => void openStorageFolder('switchBackups')} data-testid="switch-backups-open-folder">
            <FolderOpen /> {s.openFolder}
          </Button>
        }
      >
        <div>
          <p className="mb-2 text-xs text-muted-foreground">
            {s.about}
            {list && <span className="block break-all">{list.folder}</span>}
          </p>
          {backups.length === 0 ? (
            <p className="text-sm text-muted-foreground">{s.empty}</p>
          ) : (
            <ul className="max-h-72 divide-y overflow-y-auto rounded-lg border" aria-label={s.listAria}>
              {backups.map((b) => (
                <li key={b.fileName} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2 text-sm" data-testid="switch-backup-row">
                  <span className="w-40 shrink-0 font-medium">{dateTime(b.exportedAt)}</span>
                  <Badge variant={b.mode === 'internal' ? 'secondary' : 'outline'}>{s.mode[b.mode]}</Badge>
                  <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={b.root ?? b.fileName}>
                    {s.pages(b.pages)}
                  </span>
                  <span className="w-16 shrink-0 text-right text-xs text-muted-foreground">{largeFileSize(b.size)}</span>
                  <Button size="sm" variant="outline" className="text-destructive hover:text-destructive" onClick={() => setDeleting(b)}>
                    <Trash2 /> {t.common.delete}
                  </Button>
                </li>
              ))}
            </ul>
          )}
          {list && backups.length > 0 && (
            <p className={list.overWarning ? 'mt-2 text-xs text-tint-amber-fg' : 'mt-2 text-xs text-muted-foreground'} data-testid="switch-backups-total">
              {s.total(largeFileSize(list.totalSize), list.overWarning)}
            </p>
          )}
        </div>

        <div className="mt-4 border-t pt-4">
          <SettingRow label={s.warnLabel} hint={s.warnHint}>
            <Input
              type="number"
              min={0.1}
              step={0.5}
              className="w-28"
              aria-label={s.warnAria}
              aria-invalid={!warnOk}
              value={warnText ?? String(settings.switchBackupWarnGb ?? 2)}
              onChange={(e) => {
                setWarnText(e.target.value)
                const v = Number(e.target.value)
                if (e.target.value.trim() && Number.isFinite(v) && v > 0) update({ switchBackupWarnGb: v }, { onSuccess: () => void refreshSwitchBackups() })
              }}
              onBlur={() => setWarnText(null)}
            />
            <span className="text-sm text-muted-foreground">{s.gb}</span>
          </SettingRow>
          {!warnOk && <p className="mt-2 text-xs text-destructive">{s.warnInvalid}</p>}
        </div>
      </SectionCard>

      <Dialog open={!!deleting} onOpenChange={(o) => !o && !deleteBackup.isPending && setDeleting(null)}>
        <DialogContent className="sm:max-w-md" data-testid="switch-backup-delete-dialog">
          <DialogHeader>
            <DialogTitle>{s.deleteTitle}</DialogTitle>
            <DialogDescription>
              {deleting && s.deleteDetails(dateTime(deleting.exportedAt), deleting.pages, s.modeText[deleting.mode], largeFileSize(deleting.size))}
              {s.deleteForGood}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleting(null)} disabled={deleteBackup.isPending}>
              {t.common.cancel}
            </Button>
            <Button
              variant="destructive"
              disabled={deleteBackup.isPending}
              onClick={() => deleting && deleteBackup.mutate(deleting.fileName, { onSuccess: () => setDeleting(null) })}
              data-testid="switch-backup-delete-confirm"
            >
              {s.deleteConfirm}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
