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
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
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
      <ConfirmDialog
        open={resetOpen}
        title={d.confirmTitle}
        description={d.confirmBody}
        confirmLabel={busy ? d.resetting : d.confirm}
        busy={busy}
        onClose={() => setResetOpen(false)}
        onConfirm={() => void reset()}
      />
    </>
  )
}
