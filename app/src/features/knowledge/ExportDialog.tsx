// Export a page as Markdown (FR-KN-ED-12, TC-8a): ask each time for Folder or Zip, then for the place to save.
import { save } from '@tauri-apps/plugin-dialog'
import { useState } from 'react'
import { toast } from 'sonner'
import { commands } from '@/api/bindings'
import { isTauri } from '@/api/client'
import { type MarkdownExportMode, type Page, useExportPageMarkdown } from '@/api/knowledge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { useT } from '@/i18n'

export function ExportDialog({ page, open, onClose }: { page: Page; open: boolean; onClose: () => void }) {
  const [mode, setMode] = useState<MarkdownExportMode>('folder')
  const { mutateAsync: exportPage, isPending } = useExportPageMarkdown()
  const t = useT()
  const x = t.knowledge.exportPage

  const run = async () => {
    if (!isTauri()) return toast.error(x.onlyInApp)
    const ext = mode === 'zip' ? 'zip' : 'md'
    // D-58: a name that is also valid on Windows (Rust does the cleaning, the same on every platform).
    // The fallback name is a suggested file name, so it stays English.
    const name = await commands.safeFileName(page.title, 'page')
    const path = await save({
      defaultPath: `${name}.${ext}`,
      filters: [{ name: mode === 'zip' ? x.zipArchive : 'Markdown', extensions: [ext] }],
    })
    if (!path) return // cancelled
    const result = await exportPage({ pageId: page.id, path, mode })
    toast.success(x.exported, { description: `${result.path}${result.images ? x.images(result.images) : ''}` })
    onClose()
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{x.title}</DialogTitle>
          <DialogDescription>{x.description(page.title)}</DialogDescription>
        </DialogHeader>
        <RadioGroup value={mode} onValueChange={(v) => setMode(v as MarkdownExportMode)}>
          <Label className="flex items-start gap-3 rounded-xl border p-3 font-normal">
            <RadioGroupItem value="folder" className="mt-0.5" />
            <span>
              <span className="block text-sm font-medium">{x.folder}</span>
              <span className="block text-xs text-muted-foreground">{x.folderHint}</span>
            </span>
          </Label>
          <Label className="flex items-start gap-3 rounded-xl border p-3 font-normal">
            <RadioGroupItem value="zip" className="mt-0.5" />
            <span>
              <span className="block text-sm font-medium">{x.zip}</span>
              <span className="block text-xs text-muted-foreground">{x.zipHint}</span>
            </span>
          </Label>
        </RadioGroup>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button disabled={isPending} onClick={() => void run()}>
            {x.chooseLocation}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
