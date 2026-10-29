// Settings › Search, the *Model* row (FR-RAG-EM-1, FR-RAG-EM-3, D-82): a model built into the
// package, one added after installing (with *Remove*), or none, with the ways to add it: download
// (after the Gemma Terms of Use tick) or *From file…* (a `.zip` or a folder). While one runs, its
// progress; after one failed, its reason.
import { CircleAlert, FileArchive, Folder, FolderOpen } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import {
  type ModelTask,
  aboutMB,
  pickModelFolder,
  pickModelZip,
  useAddSearchModel,
  useCancelSearchModelTask,
  useDownloadSearchModel,
  useRemoveSearchModel,
  useSearchModelStatus,
} from '@/api/searchModel'
import { SearchModelDownload, SearchModelProgress } from '@/components/shared/SearchModel'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useT } from '@/i18n'
import { SettingRow } from './parts'

/** *From file…*: a `.zip` or a folder holding the three model files; checked like a download. */
function FromFileMenu() {
  const add = useAddSearchModel()
  const m = useT().searchModel
  const pick = (picker: () => Promise<string | null>) => void picker().then((p) => p && add.mutate(p))
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button size="sm" variant="outline" disabled={add.isPending}>
          <FolderOpen /> {m.fromFile}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={() => pick(pickModelZip)}>
          <FileArchive /> {m.fromZip}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={() => pick(pickModelFolder)}>
          <Folder /> {m.fromFolder}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** A download or import that failed: its reason, *Try again* (a download) and *Dismiss*. */
function FailedTask({ task }: { task: ModelTask }) {
  const download = useDownloadSearchModel()
  const cancel = useCancelSearchModelTask()
  const t = useT()
  const m = t.searchModel
  return (
    <div className="space-y-2">
      <Alert variant="destructive">
        <CircleAlert />
        <AlertTitle>{m.failedTitle}</AlertTitle>
        {task.error && <AlertDescription>{task.error}</AlertDescription>}
      </Alert>
      <div className="flex gap-2">
        {/* The terms were accepted when it started. */}
        {task.kind === 'download' && (
          <Button size="sm" disabled={download.isPending} onClick={() => download.mutate(false)}>
            {t.common.tryAgain}
          </Button>
        )}
        <Button size="sm" variant="outline" disabled={cancel.isPending} onClick={() => cancel.mutate()}>
          {m.dismiss}
        </Button>
      </div>
    </div>
  )
}

export function SearchModelRow() {
  const status = useSearchModelStatus()
  const remove = useRemoveSearchModel()
  const [removeOpen, setRemoveOpen] = useState(false)
  const t = useT()
  const s = t.settings.search
  const m = t.searchModel

  // Until the state is in, and for a bundled model: as before.
  if (!status || status.install === 'bundled')
    return (
      <SettingRow label={s.model} hint={s.modelHint}>
        <span className="rounded-lg bg-muted px-3 py-1.5 text-sm">{s.builtIn}</span>
      </SettingRow>
    )

  if (status.install === 'added')
    return (
      <>
        <SettingRow label={s.model} hint={m.addedHint}>
          <span className="rounded-lg bg-muted px-3 py-1.5 text-sm">{m.added}</span>
          <Button variant="outline" onClick={() => setRemoveOpen(true)}>
            {t.common.remove}
          </Button>
        </SettingRow>
        <Dialog open={removeOpen} onOpenChange={(o) => !remove.isPending && setRemoveOpen(o)}>
          <DialogContent className="sm:max-w-md">
            <DialogHeader>
              <DialogTitle>{m.removeTitle}</DialogTitle>
              <DialogDescription>{m.removeBody}</DialogDescription>
            </DialogHeader>
            <DialogFooter>
              <Button variant="outline" onClick={() => setRemoveOpen(false)} disabled={remove.isPending}>
                {t.common.cancel}
              </Button>
              <Button
                variant="destructive"
                disabled={remove.isPending}
                onClick={() =>
                  remove.mutate(undefined, {
                    onSuccess: () => toast.success(m.removed),
                    onSettled: () => setRemoveOpen(false),
                  })
                }
              >
                {remove.isPending ? m.removing : t.common.remove}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </>
    )

  // Not installed (a lite package): the ways to add it, or the task adding it.
  const task = status.task
  return (
    <div className="pb-3.5">
      <SettingRow label={s.model} hint={m.notInstalledHint(aboutMB(status.sizeBytes))}>
        <Badge variant="secondary" className="rounded-full">
          {m.notInstalled}
        </Badge>
      </SettingRow>
      <div className="rounded-xl border bg-background p-3">
        {task?.phase === 'failed' ? (
          <FailedTask task={task} />
        ) : task ? (
          <SearchModelProgress task={task} />
        ) : (
          <div className="space-y-3">
            <SearchModelDownload status={status} label={m.download} />
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t pt-3">
              <FromFileMenu />
              <p className="min-w-0 flex-1 basis-64 text-xs text-muted-foreground">{m.fromFileHint}</p>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
