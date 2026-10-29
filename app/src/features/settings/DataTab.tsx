// Settings › Backup & data (UI-SET-4). Each card lives in its own file.
import { pickImportArchive } from '@/api/data'
import { useIsExternal } from '@/api/storage'
import { Download, Upload } from 'lucide-react'
import { useState } from 'react'
import { SectionCard } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { BackupCard } from './BackupCard'
import { ExportDialog, ImportDialog } from './dataDialogs'
import { DemoDataCard } from './DemoDataCard'
import { SwitchBackupsCard } from './KnowledgeStorageCard'
import { LogsCard } from './LogsCard'
import { SettingRow } from './parts'

export function DataTab() {
  const [exportOpen, setExportOpen] = useState(false)
  const [importPath, setImportPath] = useState<string | null>(null)
  const external = useIsExternal()
  const d = useT().settings.data

  return (
    <div className="space-y-6">
      {/* NFR-REL-2 */}
      <BackupCard />

      <SectionCard icon={Download} tint="blue" title={d.title} description={d.description}>
        <div className="divide-y">
          <SettingRow label={d.exportLabel} hint={d.exportHint}>
            <Button onClick={() => setExportOpen(true)}><Download /> {d.export}</Button>
          </SettingRow>
          <SettingRow label={d.importLabel} hint={d.importHint}>
            <Button variant="outline" onClick={() => void pickImportArchive().then(setImportPath)}><Upload /> {d.chooseFile}</Button>
          </SettingRow>
        </div>
      </SectionCard>

      {/* D-72: the backups made before every switch of where knowledge pages live (General). */}
      <SwitchBackupsCard />

      {/* NFR-SEC-3 */}
      <LogsCard />

      {/* FR-KN-EXT-7: sample pages only in Cairn's own database. */}
      {import.meta.env.DEV && !external && <DemoDataCard />}

      <ExportDialog open={exportOpen} onOpenChange={setExportOpen} />
      <ImportDialog path={importPath} onClose={() => setImportPath(null)} />
    </div>
  )
}
