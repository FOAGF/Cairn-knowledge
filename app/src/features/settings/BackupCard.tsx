// Settings › Backup & data › Backup (NFR-REL-2, UI-SET-4; M7 part A).
import { pickBackupFolder, useBackupNow, useBackups, useBackupStatus, useSetBackupFolder } from '@/api/backups'
import { commands } from '@/api/bindings'
import { useSettings, useUpdateSettings } from '@/api/settings'
import { Archive, FolderOpen, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { SectionCard } from '@/components/shared/PageHeader'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useT } from '@/i18n'
import { dateTime, fileSize } from '@/lib/format'
import { ImportDialog } from './dataDialogs'
import { NumberSetting, SettingRow } from './parts'

export function BackupCard() {
  const settings = useSettings()
  const update = useUpdateSettings()
  const status = useBackupStatus().data
  const backups = useBackups().data ?? []
  const backupNow = useBackupNow()
  const setFolder = useSetBackupFolder()
  const [restorePath, setRestorePath] = useState<string | null>(null)
  const m = useT().settings.backup

  const chooseFolder = async () => {
    const folder = await pickBackupFolder()
    if (folder) setFolder.mutate(folder)
  }

  return (
    <SectionCard icon={Archive} tint="green" title={m.title} description={m.description}>
      <div className="divide-y">
        <SettingRow label={m.daily} hint={m.dailyHint}>
          <Switch checked={settings.backupEnabled} onCheckedChange={(v) => update({ backupEnabled: v })} aria-label={m.daily} />
        </SettingRow>
        <SettingRow
          label={m.folder}
          hint={
            <>
              <span className="break-all">{status?.folder ?? '…'}</span>
              <br />
              {status?.isDefaultFolder === false ? m.folderElsewhere : m.folderDefault}
            </>
          }
        >
          <Button variant="outline" onClick={() => void chooseFolder()} disabled={setFolder.isPending}>{m.chooseFolder}</Button>
          <Button variant="outline" onClick={() => void commands.openFolder('backups')}><FolderOpen /> {m.openFolder}</Button>
          {status?.isDefaultFolder === false && (
            <Button variant="ghost" onClick={() => setFolder.mutate(null)} disabled={setFolder.isPending}>{m.useDefault}</Button>
          )}
        </SettingRow>
        <SettingRow label={m.keep} hint={m.keepHint}>
          <NumberSetting value={settings.backupKeep} min={1} max={1000} label={m.keepAria} onSave={(n) => update({ backupKeep: n })} />
          <span className="text-sm text-muted-foreground">{m.keepUnit}</span>
        </SettingRow>
        <SettingRow label={m.last} hint={<LastBackup status={status} />}>
          <Button variant="outline" onClick={() => backupNow.mutate()} disabled={backupNow.isPending}>
            {backupNow.isPending ? m.backingUp : m.backUpNow}
          </Button>
        </SettingRow>
      </div>

      {status?.checkFailed && (
        <Alert variant="destructive" className="mt-4">
          <TriangleAlert />
          <AlertTitle>{m.checkFailedTitle}</AlertTitle>
          <AlertDescription>{m.checkFailedBody}</AlertDescription>
        </Alert>
      )}

      <div className="mt-4 border-t pt-4">
        <p className="mb-2 text-sm font-medium">{m.listTitle}</p>
        <p className="mb-2 text-xs text-muted-foreground">{m.listHint}</p>
        {backups.length === 0 ? (
          <p className="text-sm text-muted-foreground">{m.empty}</p>
        ) : (
          <ul className="max-h-72 divide-y overflow-y-auto rounded-lg border" aria-label={m.listAria}>
            {backups.map((b) => (
              <li key={b.path} className="flex flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2 text-sm">
                <span className="w-40 shrink-0 font-medium">{dateTime(b.exportedAt)}</span>
                <Badge variant={b.kind === 'daily' ? 'secondary' : 'outline'}>{b.kind === 'daily' ? m.kindDaily : m.kindManual}</Badge>
                <span className="w-44 shrink-0 truncate text-xs" title={b.computer ?? m.noComputer}>
                  {b.computer === null ? (
                    <span className="text-muted-foreground">{m.earlier}</span>
                  ) : b.computer.toLowerCase() === status?.computer.toLowerCase() ? (
                    <>
                      {b.computer} <span className="text-muted-foreground">{m.thisComputer}</span>
                    </>
                  ) : (
                    b.computer
                  )}
                </span>
                <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground" title={b.fileName}>{b.fileName}</span>
                <span className="w-16 shrink-0 text-right text-xs text-muted-foreground">{fileSize(b.size)}</span>
                <Button size="sm" variant="outline" onClick={() => setRestorePath(b.path)}>{m.restore}</Button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <ImportDialog path={restorePath} onClose={() => setRestorePath(null)} />
    </SectionCard>
  )
}

/** UI-SET-4's *Last backup* line: time, file and size, or why the last try failed. */
function LastBackup({ status }: { status: ReturnType<typeof useBackupStatus>['data'] }) {
  const m = useT().settings.backup
  if (!status) return <>…</>
  const { last, error, errorAt } = status
  return (
    <>
      {error && (
        <span className="block text-destructive">
          {m.lastFailed(errorAt ? dateTime(errorAt) : null, error)}
        </span>
      )}
      {last ? (
        <span className="block break-all">
          {dateTime(last.exportedAt)} · {last.fileName} · {fileSize(last.size)}
        </span>
      ) : (
        !error && <span className="block">{m.noneYet}</span>
      )}
    </>
  )
}
