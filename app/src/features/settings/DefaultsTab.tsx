import { type ModelConfig, useModelList, useProviderList, webSearchModel } from '@/api/providers'
import { useSettings, useUpdateSettings } from '@/api/settings'
import { Bot, MessageSquareText, TriangleAlert } from 'lucide-react'
import { Link } from 'react-router'
import { SectionCard } from '@/components/shared/PageHeader'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { NumberSetting, SettingRow } from './parts'
import { SearchModelRow } from './SearchModelRow'

/** UI-SET-3, D-91: the default models (background, OCR, web search, search model), then the Assistant's budgets. */
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

  // `automatic`: the web search model nobody chose, shown as such (FR-LLM-AS-6).
  const picker = (value: string | null | undefined, onChange: (v: string) => void, fits: (m: ModelConfig) => boolean, automatic = false) => (
    <Select value={value ?? ''} onValueChange={onChange}>
      <SelectTrigger className="w-full">
        <SelectValue placeholder={d.choose} />
      </SelectTrigger>
      <SelectContent>
        {providers.filter((p) => p.enabled).map((p) => {
          const ms = models.filter((m) => m.providerId === p.id && fits(m))
          if (!ms.length) return null
          return (
            <SelectGroup key={p.id}>
              <SelectLabel>{p.name}</SelectLabel>
              {ms.map((m) => (
                <SelectItem key={m.id} value={m.id}>{automatic && m.id === value ? d.automatic(m.label) : m.label}</SelectItem>
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

  const web = webSearchModel(providers, models, settings.webSearchModelId)
  const anyTicked = models.some((m) => m.webSearch)

  return (
    <div className="space-y-6">
      <SectionCard icon={Bot} tint="blue" title={d.defaultTitle} description={d.defaultDescription}>
        <div className="divide-y">
          <SettingRow label={d.background} hint={d.backgroundHint}>
            <div className="w-72">
              {picker(settings.defaultModelId, (v) => update({ defaultModelId: v }), () => true)}
              {warning(settings.defaultModelId)}
            </div>
          </SettingRow>
          <SettingRow label={d.ocr} hint={d.ocrHint}>
            <div className="w-72">
              {picker(settings.ocrModelId, (v) => update({ ocrModelId: v }), (m) => m.vision)}
              {warning(settings.ocrModelId)}
            </div>
          </SettingRow>
          {/* FR-LLM-AS-12, FR-HOME-7 */}
          <SettingRow label={d.webSearch} hint={d.webSearchHint}>
            <div className="w-72">
              {anyTicked ? (
                <>
                  {picker(web?.model.id, (v) => update({ webSearchModelId: v }), (m) => m.webSearch, web?.automatic)}
                  {warning(web?.model.id)}
                </>
              ) : (
                <p className="text-sm text-muted-foreground">
                  {d.webSearchNone((text) => (
                    <Link to="/settings/providers" className="text-primary underline-offset-4 hover:underline">
                      {text}
                    </Link>
                  ))}
                </p>
              )}
            </div>
          </SettingRow>
          {/* FR-RAG-EM-1, FR-RAG-EM-3 */}
          <SearchModelRow />
        </div>
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
    </div>
  )
}
