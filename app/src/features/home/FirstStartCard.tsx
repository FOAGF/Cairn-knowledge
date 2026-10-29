// First start of a copy with no data yet (FR-DATA-8): Home offers *Start fresh* or an import, and on
// a lite package the search model's download (D-82).
import { pickImportArchive } from '@/api/data'
import { aboutMB, useSearchModelStatus } from '@/api/searchModel'
import { useUpdateSettings } from '@/api/settings'
import { Info, ScanSearch, Sprout, Upload } from 'lucide-react'
import { useState } from 'react'
import { SectionCard } from '@/components/shared/PageHeader'
import { SearchModelDownload, SearchModelProgress, SearchModelTaskLine } from '@/components/shared/SearchModel'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { ImportDialog } from '@/features/settings/dataDialogs'
import { useT } from '@/i18n'
import { useFirstStart } from './firstStart'

/**
 * FR-DATA-8, D-82: on a lite package, the search model isn't installed. Search by words works; the
 * note offers the download (after the Gemma terms tick), which carries on after the card closes.
 * Once started, the note shows its progress. *From file…* is only in Settings › Search.
 */
function SearchModelNote() {
  const status = useSearchModelStatus()
  const m = useT().searchModel
  if (!status || status.install !== 'notInstalled') return null
  const task = status.task
  return (
    <Alert>
      <ScanSearch />
      <AlertTitle>{m.firstStartTitle}</AlertTitle>
      <AlertDescription className="space-y-3">
        <p>{m.firstStartText(aboutMB(status.sizeBytes))}</p>
        {task?.phase === 'failed' ? (
          <SearchModelTaskLine />
        ) : task ? (
          <SearchModelProgress task={task} />
        ) : (
          <SearchModelDownload status={status} label={m.downloadInBackground} />
        )}
      </AlertDescription>
    </Alert>
  )
}

export function FirstStartCard() {
  const firstStart = useFirstStart() === true
  const update = useUpdateSettings()
  const [importPath, setImportPath] = useState<string | null>(null)
  // The import dialog stays open to show its result, after the data has appeared.
  const [importing, setImporting] = useState(false)
  const t = useT()
  if (!firstStart && !importing) return null

  return (
    <>
      {firstStart && (
        <SectionCard icon={Sprout} tint="green" title={t.home.firstStart.title} description={t.home.firstStart.description}>
          <div className="space-y-4">
            <div className="flex flex-wrap gap-3">
              <Button onClick={() => update({ welcomeDone: true })}><Sprout /> {t.home.firstStart.fresh}</Button>
              <Button
                variant="outline"
                onClick={() => void pickImportArchive().then((p) => { if (p) { setImporting(true); setImportPath(p) } })}
              >
                <Upload /> {t.home.firstStart.import}
              </Button>
            </div>
            <Alert>
              <Info />
              <AlertTitle>{t.home.firstStart.keysTitle}</AlertTitle>
              <AlertDescription>{t.home.firstStart.keysText}</AlertDescription>
            </Alert>
            <SearchModelNote />
          </div>
        </SectionCard>
      )}
      <ImportDialog
        path={importPath}
        onImported={() => update({ welcomeDone: true })}
        onClose={() => { setImportPath(null); setImporting(false) }}
      />
    </>
  )
}
