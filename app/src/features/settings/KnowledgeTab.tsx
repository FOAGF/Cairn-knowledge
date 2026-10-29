import { useSettings, useUpdateSettings } from '@/api/settings'
import { Plus, X } from 'lucide-react'
import { useState } from 'react'
import { SectionCard } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { useT } from '@/i18n'
import { NumberSetting, SettingRow } from './parts'

export function KnowledgeTab() {
  const settings = useSettings()
  const update = useUpdateSettings()
  const t = useT()
  const k = t.settings.knowledge
  const [word, setWord] = useState('')
  // Typed as text, so "0." and "" can be typed; only a value between 0 and 1 is saved.
  const [score, setScore] = useState<string | null>(null)
  const scoreOk = score === null || (Number(score) > 0 && Number(score) < 1)
  const num = (v: string, fallback: number) => (Number.isFinite(Number(v)) && Number(v) > 0 ? Number(v) : fallback)

  const addWord = () => {
    const w = word.trim()
    if (w && !settings.personalDictionary.includes(w)) update({ personalDictionary: [...settings.personalDictionary, w] })
    setWord('')
  }

  return (
    <div className="space-y-6">
      <SectionCard title={k.limitsTitle} description={k.limitsDescription}>
        <div className="divide-y">
          <SettingRow label={k.wordWarning} hint={k.wordWarningHint}>
            <Input type="number" min={100} className="w-28" value={settings.wordWarningThreshold} onChange={(e) => update({ wordWarningThreshold: num(e.target.value, 1000) })} />
            <span className="text-sm text-muted-foreground">{k.words}</span>
          </SettingRow>
          <SettingRow label={k.largeFile} hint={k.largeFileHint}>
            <Input type="number" min={1} className="w-28" value={settings.largeFileWarningMB} onChange={(e) => update({ largeFileWarningMB: num(e.target.value, 10) })} />
            <span className="text-sm text-muted-foreground">{k.mb}</span>
          </SettingRow>
        </div>
      </SectionCard>

      <SectionCard title={k.duplicatesTitle} description={k.duplicatesDescription}>
        <SettingRow label={k.sameTopic} hint={k.sameTopicHint}>
          <Input
            type="number"
            min={0.01}
            max={0.99}
            step={0.01}
            className="w-28"
            aria-invalid={!scoreOk}
            value={score ?? String(settings.duplicateScore)}
            onChange={(e) => {
              setScore(e.target.value)
              const v = Number(e.target.value)
              if (e.target.value.trim() && v > 0 && v < 1) update({ duplicateScore: v })
            }}
            onBlur={() => setScore(null)}
          />
        </SettingRow>
        {!scoreOk && <p className="mt-2 text-xs text-destructive">{k.scoreInvalid}</p>}
        <SettingRow label={k.atOnce} hint={k.atOnceHint}>
          <NumberSetting label={k.atOnce} value={settings.checksAtOnce} min={1} max={20} onSave={(n) => update({ checksAtOnce: n })} />
        </SettingRow>
      </SectionCard>

      <SectionCard title={k.spellingTitle} description={k.spellingDescription}>
        <div className="divide-y">
          <SettingRow label={k.spellcheck} hint={k.spellcheckHint}>
            <Switch checked={settings.spellcheck} onCheckedChange={(v) => update({ spellcheck: v })} />
          </SettingRow>
          <SettingRow label={k.grammar} hint={k.grammarHint}>
            <Switch checked={settings.grammar} onCheckedChange={(v) => update({ grammar: v })} />
          </SettingRow>
        </div>
      </SectionCard>

      {/* FR-KN-ED-8: personal dictionary */}
      <SectionCard title={k.dictionaryTitle} description={k.dictionaryDescription}>
        <div className="flex flex-wrap gap-2">
          {settings.personalDictionary.map((w) => (
            <span key={w} className="inline-flex items-center gap-1 rounded-full bg-tint-blue py-1 pr-1.5 pl-3 text-sm font-medium text-tint-blue-fg">
              {w}
              <button
                aria-label={k.removeWord(w)}
                className="rounded-full p-0.5 hover:bg-foreground/10"
                onClick={() => update({ personalDictionary: settings.personalDictionary.filter((x) => x !== w) })}
              >
                <X className="size-3.5" />
              </button>
            </span>
          ))}
          {settings.personalDictionary.length === 0 && <p className="text-sm text-muted-foreground">{k.noWords}</p>}
        </div>
        <form
          className="mt-4 flex max-w-sm gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            addWord()
          }}
        >
          <Input value={word} onChange={(e) => setWord(e.target.value)} placeholder={k.addWord} />
          <Button type="submit" variant="outline" disabled={!word.trim()}>
            <Plus /> {t.common.add}
          </Button>
        </form>
      </SectionCard>
    </div>
  )
}
