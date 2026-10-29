import { useModelList, useProviderList } from '@/api/providers'
import { useSettings, useUpdateSettings } from '@/api/settings'
import { Bot, MessageSquareText, ScanText, TriangleAlert } from 'lucide-react'
import { SectionCard } from '@/components/shared/PageHeader'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { NumberSetting, SettingRow } from './parts'

/** A whole number typed as text, so the field can be emptied while typing; only a value in range is saved. */
/** FR-LLM-AS-6: default background model and OCR model. */
export function DefaultsTab() {
  const settings = useSettings()
  const update = useUpdateSettings()
  const models = useModelList()
  const providers = useProviderList()
  const d = useT().settings.defaults

  const disabledIds = new Set(providers.filter((p) => !p.enabled).map((p) => p.id))
  const isOff = (id?: string | null) => {
    const m = models.find((x) => x.id === id)
    return !!m && disabledIds.has(m.providerId)
  }
  const warning = (id?: string | null) =>
    isOff(id) && (
      <p className="mt-2 flex items-start gap-1.5 rounded-lg bg-tint-amber px-3 py-2 text-xs text-tint-amber-fg">
        <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
        {d.providerOff}
      </p>
    )

  const picker = (value: string | null | undefined, onChange: (v: string) => void, visionOnly: boolean) => (
    <Select value={value ?? ''} onValueChange={onChange}>
      <SelectTrigger className="w-full">
        <SelectValue placeholder={d.choose} />
      </SelectTrigger>
      <SelectContent>
        {providers.filter((p) => p.enabled).map((p) => {
          const ms = models.filter((m) => m.providerId === p.id && (!visionOnly || m.vision))
          if (!ms.length) return null
          return (
            <SelectGroup key={p.id}>
              <SelectLabel>{p.name}</SelectLabel>
              {ms.map((m) => (
                <SelectItem key={m.id} value={m.id}>{m.label}</SelectItem>
              ))}
            </SelectGroup>
          )
        })}
        {isOff(value) && (
          <SelectGroup>
            <SelectLabel>{d.providerDisabled}</SelectLabel>
            <SelectItem value={value!} disabled>{models.find((m) => m.id === value)?.label}</SelectItem>
          </SelectGroup>
        )}
      </SelectContent>
    </Select>
  )

  return (
    <div className="space-y-6">
      <SectionCard icon={Bot} tint="blue" title={d.defaultTitle} description={d.defaultDescription}>
        <SettingRow
          label={d.background}
          hint={d.backgroundHint}
        >
          <div className="w-72">
            {picker(settings.defaultModelId, (v) => update({ defaultModelId: v }), false)}
            {warning(settings.defaultModelId)}
          </div>
        </SettingRow>
      </SectionCard>
      {/* UI-SET-3: the Assistant's budgets (FR-LLM-AS-1, -10, -12) */}
      <SectionCard icon={MessageSquareText} tint="green" title={d.assistantTitle} description={d.assistantDescription}>
        <div className="divide-y">
          <SettingRow label={d.sourceBudget} hint={d.sourceBudgetHint}>
            <NumberSetting label={d.sourceBudgetAria} value={settings.assistantSourceBudget} min={500} max={1000000} onSave={(n) => update({ assistantSourceBudget: n })} />
            <span className="text-sm text-muted-foreground">{d.tokens}</span>
          </SettingRow>
          <SettingRow label={d.historyBudget} hint={d.historyBudgetHint}>
            <NumberSetting label={d.historyBudgetAria} value={settings.assistantHistoryBudget} min={500} max={10000000} onSave={(n) => update({ assistantHistoryBudget: n })} />
            <span className="text-sm text-muted-foreground">{d.tokens}</span>
          </SettingRow>
          <SettingRow label={d.webSearches} hint={d.webSearchesHint}>
            <NumberSetting label={d.webSearches} value={settings.webSearchesPerMessage} min={1} max={20} onSave={(n) => update({ webSearchesPerMessage: n })} />
          </SettingRow>
        </div>
      </SectionCard>
      <SectionCard icon={ScanText} tint="purple" title={d.ocrTitle} description={d.ocrDescription}>
        <SettingRow label={d.ocr} hint={d.ocrHint}>
          <div className="w-72">
            {picker(settings.ocrModelId, (v) => update({ ocrModelId: v }), true)}
            {warning(settings.ocrModelId)}
          </div>
        </SettingRow>
      </SectionCard>
    </div>
  )
}
