// The export and import dialogs (FR-DATA-1..6), shared by Settings › Backup & data, a backup's
// *Restore…* and the first-start screen (FR-DATA-8).
import type { TableCount } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { type ImportRequest, useBackupInfo, useExportData, useImportData } from '@/api/data'
import { useIsExternal } from '@/api/storage'
import { Download, FolderOpen, Info, TriangleAlert } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { fileSize } from '@/lib/format'
import { pickFolder } from '@/lib/pickFolder'

/** One line per table that has rows, e.g. "categories 5 · settings 14". */
function tableCounts(tables: TableCount[]) {
  return tables.filter((t) => t.rows > 0).map((t) => `${t.table} ${t.rows}`).join(' · ')
}

/** FR-DATA-1..5: explain what is included, then "Save as" and export in Rust. */
export function ExportDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const exportData = useExportData()
  const external = useIsExternal()
  const { reset } = exportData
  useEffect(() => { if (open) reset() }, [open, reset])
  const result = exportData.data
  const t = useT()
  const e = t.settings.exportDialog
  return (
    <Dialog open={open} onOpenChange={(o) => !exportData.isPending && onOpenChange(o)}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{e.title}</DialogTitle>
          <DialogDescription>{e.description}</DialogDescription>
        </DialogHeader>
        {result ? (
          <div className="space-y-1 py-2 text-sm">
            <p className="font-medium text-tint-green-fg">{e.finished(fileSize(result.fileSize))}</p>
            <p className="break-all text-muted-foreground">{result.path}</p>
            <p className="text-xs text-muted-foreground">{tableCounts(result.tables)}{result.attachments > 0 && e.attachments(result.attachments)}</p>
          </div>
        ) : (
          <div className="space-y-3 text-sm">
            <p className="font-medium">{e.included}</p>
            <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
              <li>{e.includesTasks}</li>
              <li>{e.includesPages}</li>
              <li>{e.includesConversations}</li>
              <li>{e.includesSettings}</li>
            </ul>
            {/* FR-KN-EXT-14 */}
            {external && <p className="text-muted-foreground">{e.externalNote}</p>}
            <Alert>
              <Info />
              <AlertTitle>{e.noKeysTitle}</AlertTitle>
              <AlertDescription>{e.noKeysBody}</AlertDescription>
            </Alert>
          </div>
        )}
        <DialogFooter>
          {result ? (
            <Button onClick={() => onOpenChange(false)}>{t.common.close}</Button>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)} disabled={exportData.isPending}>{t.common.cancel}</Button>
              <Button onClick={() => exportData.mutate()} disabled={exportData.isPending}>
                <Download /> {exportData.isPending ? e.exporting : e.export}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/**
 * FR-DATA-3, -4, -6: confirm (replaces everything, automatic backup first), then import in Rust.
 * FR-KN-EXT-14: the archive is read first. An external-mode backup whose folder isn't on this computer
 * asks where the folder is now; without it, a warning offers to import only the tasks and categories.
 */
export function ImportDialog({ path, onClose, onImported }: { path: string | null; onClose: () => void; onImported?: () => void }) {
  const importData = useImportData()
  const { reset } = importData
  useEffect(() => { if (path) reset() }, [path, reset])
  // While it is read the button waits. A file Rust won't import (a switch backup, which holds
  // knowledge pages only, or a damaged file) says why here, before anything is replaced.
  const infoQuery = useBackupInfo(path)
  const info = infoQuery.data
  const refused = infoQuery.isError ? errorMessage(infoQuery.error) : null
  // Where the backup's folder is now (picked here), or the tasks-only import after the warning.
  const [root, setRoot] = useState<string | null>(null)
  const [tasksOnly, setTasksOnly] = useState(false)
  const [forPath, setForPath] = useState(path)
  if (forPath !== path) {
    setForPath(path)
    setRoot(null)
    setTasksOnly(false)
  }
  const external = info?.mode === 'external'
  const needsRoot = external && !info.rootExists
  const result = importData.data
  const t = useT()
  const d = t.settings.importDialog
  const pick = async () => {
    const folder = await pickFolder(d.pickRoot)
    if (folder) {
      setRoot(folder)
      setTasksOnly(false)
    }
  }
  const run = (request: ImportRequest) => importData.mutate(request, { onSuccess: () => onImported?.() })

  // The folder is missing and none was picked: where is it, or go on without it.
  const asking = !result && !refused && needsRoot && !root && !tasksOnly
  const warning = !result && needsRoot && tasksOnly
  return (
    <Dialog open={!!path} onOpenChange={(o) => !o && !importData.isPending && onClose()}>
      <DialogContent className="sm:max-w-lg" data-testid="import-dialog">
        <DialogHeader>
          <DialogTitle>
            {result ? d.titleFinished : refused ? d.titleRefused : asking ? d.titleAsking : warning ? d.titleWarning : d.titleConfirm}
          </DialogTitle>
          <DialogDescription className="break-all">{path}</DialogDescription>
        </DialogHeader>
        {result ? (
          <div className="space-y-1 py-2 text-sm">
            <p className="text-muted-foreground">{tableCounts(result.tables)}{result.attachments > 0 && t.settings.exportDialog.attachments(result.attachments)}</p>
            <p className="text-xs text-muted-foreground">{d.savedTo(<span className="break-all">{result.backupPath}</span>)}</p>
          </div>
        ) : refused ? (
          <Alert variant="destructive" data-testid="import-refused">
            <TriangleAlert />
            <AlertTitle>{d.nothingChanged}</AlertTitle>
            <AlertDescription>{refused}</AlertDescription>
          </Alert>
        ) : asking ? (
          <div className="space-y-3 text-sm">
            <p>
              {d.askingBody(<span className="font-mono text-xs break-all">{info.root}</span>)}
            </p>
            <p className="text-muted-foreground">{d.askingHint}</p>
          </div>
        ) : warning ? (
          <Alert variant="destructive" data-testid="import-tasks-only-warning">
            <TriangleAlert />
            <AlertTitle>{d.warningTitle}</AlertTitle>
            <AlertDescription>
              {d.warningBody}
            </AlertDescription>
          </Alert>
        ) : (
          <>
            <Alert variant="destructive">
              <Info />
              <AlertTitle>{d.replaceTitle}</AlertTitle>
              <AlertDescription>{d.replaceBody}</AlertDescription>
            </Alert>
            {external && (
              <div className="flex items-center gap-2 text-sm text-muted-foreground" data-testid="import-root">
                <FolderOpen className="size-4 shrink-0" />
                <span className="min-w-0 flex-1">
                  {d.readFrom(<span className="font-mono text-xs break-all text-foreground">{root ?? info.root}</span>)}
                </span>
                {needsRoot && (
                  <Button size="sm" variant="ghost" onClick={() => void pick()} disabled={importData.isPending}>
                    {d.change}
                  </Button>
                )}
              </div>
            )}
          </>
        )}
        <DialogFooter>
          {result || refused ? (
            <Button onClick={onClose}>{t.common.close}</Button>
          ) : asking ? (
            <>
              <Button variant="outline" onClick={onClose}>{t.common.cancel}</Button>
              <Button variant="outline" onClick={() => setTasksOnly(true)} data-testid="import-without-root">{d.withoutFolder}</Button>
              <Button onClick={() => void pick()} data-testid="import-choose-root"><FolderOpen /> {d.chooseFolder}</Button>
            </>
          ) : warning ? (
            <>
              <Button variant="outline" onClick={() => setTasksOnly(false)} disabled={importData.isPending}>{t.common.back}</Button>
              <Button variant="destructive" onClick={() => path && run({ path, tasksOnly: true })} disabled={importData.isPending} data-testid="import-confirm">
                {importData.isPending ? d.importing : d.importTasks}
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={onClose} disabled={importData.isPending}>{t.common.cancel}</Button>
              <Button
                variant="destructive"
                onClick={() => path && run(root ? { path, root } : { path })}
                disabled={importData.isPending || infoQuery.isPending}
                data-testid="import-confirm"
              >
                {importData.isPending ? d.importing : d.replace}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
