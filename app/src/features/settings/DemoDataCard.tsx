// Settings › Backup & data › Demo data: development builds only (UI-SET-4). *Reset* replaces
// all data with an empty database (Rust), then the app reloads and the dev seed in `AppShell`
// adds the samples again.
import { commands } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { RotateCcw } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { SectionCard } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { resetDemoData } from '@/store'
import { SettingRow } from './parts'

export function DemoDataCard() {
  const [resetOpen, setResetOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const t = useT()
  const d = t.settings.demo

  const reset = async () => {
    setBusy(true)
    try {
      await commands.devResetData()
      resetDemoData()
      // The samples come back when the app shell starts again.
      window.location.reload()
    } catch (e) {
      setBusy(false)
      toast.error(errorMessage(e))
    }
  }
  return (
    <>
      <SectionCard icon={RotateCcw} tint="amber" title={d.title} description={d.description}>
        <SettingRow label={d.reset} hint={d.resetHint}>
          <Button variant="destructive" onClick={() => setResetOpen(true)}>{d.resetButton}</Button>
        </SettingRow>
      </SectionCard>
      <Dialog open={resetOpen} onOpenChange={setResetOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{d.confirmTitle}</DialogTitle>
            <DialogDescription>{d.confirmBody}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setResetOpen(false)} disabled={busy}>{t.common.cancel}</Button>
            <Button variant="destructive" onClick={() => void reset()} disabled={busy}>{busy ? d.resetting : d.confirm}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
