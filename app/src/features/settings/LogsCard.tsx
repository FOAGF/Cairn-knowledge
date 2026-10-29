// Settings › Backup & data › Logs (NFR-SEC-3, UI-SET-4): *Open logs folder*, the *Detailed logs*
// switch (until the next start) and the space the logs take.
import { ScrollText } from 'lucide-react'
import { commands } from '@/api/bindings'
import { useLogStatus, useSetDetailedLogs } from '@/api/logs'
import { SectionCard } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useT } from '@/i18n'
import { fileSize } from '@/lib/format'
import { SettingRow } from './parts'

export function LogsCard() {
  const status = useLogStatus()
  const setDetailed = useSetDetailedLogs()
  const l = useT().settings.logs
  return (
    <SectionCard icon={ScrollText} tint="slate" title={l.title} description={l.description}>
      <div className="divide-y">
        <SettingRow label={l.size} hint={l.sizeHint}>
          <span className="text-sm tabular-nums" data-testid="log-size">{status.data ? fileSize(status.data.totalBytes) : '…'}</span>
        </SettingRow>
        <SettingRow label={l.detailed} hint={l.detailedHint}>
          <Switch
            checked={status.data?.detailed ?? false}
            disabled={!status.data || setDetailed.isPending}
            onCheckedChange={(on) => setDetailed.mutate(on)}
            aria-label={l.detailedAria}
          />
        </SettingRow>
        <SettingRow label={l.open} hint={l.openHint}>
          <Button variant="outline" onClick={() => void commands.openFolder('logs')}>{l.open}</Button>
        </SettingRow>
      </div>
    </SectionCard>
  )
}
