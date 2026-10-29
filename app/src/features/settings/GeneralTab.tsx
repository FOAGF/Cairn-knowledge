import { useDataFolder, useSetDataFolder, useSettings, useUpdateSettings } from '@/api/settings'
import { Folder } from 'lucide-react'
import { useState } from 'react'
import { SectionCard } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'
import { toast } from 'sonner'
import { useT } from '@/i18n'
import { KnowledgeStorageEntry } from './KnowledgeStorageCard'
import { SettingRow } from './parts'

/** TC-9: the new folder is used from the next start; existing data is not moved. */
function DataFolderDialog({ open, onOpenChange, current }: { open: boolean; onOpenChange: (o: boolean) => void; current: string }) {
  const [folder, setFolder] = useState(current)
  const save = useSetDataFolder()
  const t = useT()
  const submit = () =>
    save.mutate(folder, {
      onSuccess: () => {
        toast.success(t.settings.dataFolderDialog.saved)
        onOpenChange(false)
      },
    })
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t.settings.dataFolderDialog.title}</DialogTitle>
          <DialogDescription>{t.settings.dataFolderDialog.description}</DialogDescription>
        </DialogHeader>
        <Input autoFocus value={folder} placeholder={t.settings.dataFolderDialog.placeholder} onChange={(e) => setFolder(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && submit()} />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t.common.cancel}</Button>
          <Button onClick={submit} disabled={save.isPending || !folder.trim()}>{t.common.save}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function GeneralTab() {
  const settings = useSettings()
  const update = useUpdateSettings()
  const { data: folder } = useDataFolder()
  const [dialogOpen, setDialogOpen] = useState(false)
  const t = useT()
  const g = t.settings.general
  return (
    <SectionCard title={g.title} description={g.description}>
      <div className="divide-y">
        <SettingRow label={g.yourName} hint={g.yourNameHint}>
          <Input className="w-56" value={settings.userName} onChange={(e) => update({ userName: e.target.value })} />
        </SettingRow>
        <SettingRow label={g.theme} hint={g.themeHint}>
          <ToggleGroup type="single" variant="outline" value={settings.theme} onValueChange={(v) => v && update({ theme: v as typeof settings.theme })}>
            <ToggleGroupItem value="system" className="px-4">{g.themeSystem}</ToggleGroupItem>
            <ToggleGroupItem value="light" className="px-4">{g.themeLight}</ToggleGroupItem>
            <ToggleGroupItem value="dark" className="px-4">{g.themeDark}</ToggleGroupItem>
          </ToggleGroup>
        </SettingRow>
        <SettingRow label={g.language} hint={g.languageHint}>
          <ToggleGroup type="single" variant="outline" value={settings.language} onValueChange={(v) => v && update({ language: v as typeof settings.language })}>
            <ToggleGroupItem value="system" className="px-4">{g.languageSystem}</ToggleGroupItem>
            <ToggleGroupItem value="en" lang="en" className="px-4">{g.languageEnglish}</ToggleGroupItem>
            <ToggleGroupItem value="zh-CN" lang="zh-CN" className="px-4">{g.languageChinese}</ToggleGroupItem>
          </ToggleGroup>
        </SettingRow>
        <SettingRow
          label={g.dataFolder}
          hint={folder?.nextStart ? g.dataFolderNextStart(folder.nextStart) : g.dataFolderHint}
        >
          <code className="flex items-center gap-2 rounded-lg bg-muted px-3 py-1.5 text-xs">
            <Folder className="size-3.5 text-muted-foreground" />
            {folder?.current ?? '…'}
          </code>
          <Button variant="outline" disabled={!folder} onClick={() => (folder?.canChange ? setDialogOpen(true) : toast.info(g.folderChangeInDev))}>
            {g.changeFolder}
          </Button>
          {dialogOpen && <DataFolderDialog open onOpenChange={setDialogOpen} current={folder?.nextStart ?? folder?.current ?? ''} />}
        </SettingRow>
        {/* D-72: in Cairn's database or a folder of Markdown files, and the switch between them. */}
        <KnowledgeStorageEntry />
      </div>
    </SectionCard>
  )
}
