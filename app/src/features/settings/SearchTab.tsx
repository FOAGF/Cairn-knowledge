import { useSettings, useUpdateSettings } from '@/api/settings'
import { Cloud, Cpu, Database, Gauge } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { SectionCard } from '@/components/shared/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Progress } from '@/components/ui/progress'
import { Switch } from '@/components/ui/switch'
import { rebuildSearchIndex, useIndexProgress, useSidecarStatus } from '@/api/search'
import { errorMessage } from '@/api/client'
import { useSearchModelStatus } from '@/api/searchModel'
import { useT } from '@/i18n'
import { SettingRow } from './parts'
import { SearchModelRow } from './SearchModelRow'

/** Settings › Search: the search model (added later on a lite package, FR-RAG-EM-3), how far the index is, and the idle timeout (FR-RAG-IX-5, UI-SET-3). */
export function SearchTab() {
  const settings = useSettings()
  const update = useUpdateSettings()
  const progress = useIndexProgress()
  const sidecars = useSidecarStatus()
  const [rebuilding, setRebuilding] = useState(false)
  const s = useT().settings.search
  // Typed as text, so "0." and "" can be typed; only a value between 0 and 1 is saved.
  const [relevance, setRelevance] = useState<string | null>(null)
  const relevanceOk = relevance === null || (relevance !== '' && Number(relevance) >= 0 && Number(relevance) <= 1)

  // FR-RAG-IX-2: re-cut the pieces; then the indexer embeds only the ones that changed.
  const rebuild = () => {
    setRebuilding(true)
    rebuildSearchIndex()
      .then(() => toast.success(s.rebuilt))
      .catch((e) => toast.error(errorMessage(e)))
      .finally(() => setRebuilding(false))
  }

  const status = progress?.status
  const percent = status && status.items > 0 ? (status.itemsReady / status.items) * 100 : 100
  const embed = sidecars?.embed
  const notInstalled = useSearchModelStatus()?.install === 'notInstalled'

  return (
    <div className="space-y-6">
      {/* FR-RAG-EM-1, EM-1a, EM-3 (D-82) */}
      <SectionCard icon={Cpu} tint="purple" title={s.modelTitle} description={s.modelDescription}>
        <div className="divide-y">
          <SearchModelRow />
          {/* Without a model (a lite package, D-82) the worker never starts. */}
          {!notInstalled && (
            <SettingRow
              label={s.workerLabel}
              hint={embed?.state === 'failed' && embed.detail ? embed.detail : s.workerHint}
            >
              {embed && (
                <Badge variant={embed.state === 'failed' ? 'destructive' : 'secondary'} className="rounded-full">
                  {s.worker[embed.state]}
                </Badge>
              )}
            </SettingRow>
          )}
          <SettingRow label={s.idle} hint={s.idleHint}>
            <Input type="number" min={1} className="w-24" value={settings.embeddingIdleTimeoutMin} onChange={(e) => update({ embeddingIdleTimeoutMin: Math.max(1, Number(e.target.value) || 5) })} />
            <span className="text-sm text-muted-foreground">{s.minutes}</span>
          </SettingRow>
        </div>
      </SectionCard>

      {/* FR-LLM-AS-1, UI-SET-3 */}
      <SectionCard icon={Gauge} tint="green" title={s.assistantTitle} description={s.assistantDescription}>
        <SettingRow label={s.relevance} hint={s.relevanceHint}>
          <Input
            type="number"
            min={0}
            max={1}
            step={0.05}
            className="w-24"
            aria-label={s.relevance}
            aria-invalid={!relevanceOk}
            value={relevance ?? settings.assistantMinRelevance ?? 0.4}
            onChange={(e) => {
              setRelevance(e.target.value)
              const v = Number(e.target.value)
              if (e.target.value !== '' && v >= 0 && v <= 1) update({ assistantMinRelevance: v })
            }}
            onBlur={() => setRelevance(null)}
          />
        </SettingRow>
      </SectionCard>

      <SectionCard icon={Database} tint="blue" title={s.indexTitle} description={s.indexDescription}>
        {status ? (
          <div className="space-y-2">
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <p className="text-sm">
                {s.searchable(<span className="text-2xl font-semibold tabular-nums">{status.itemsReady}</span>, status.items)}
              </p>
              <p className="text-xs text-muted-foreground tabular-nums">
                {s.pieces(status.chunksReady, status.chunks)}
                {progress?.running && s.indexing}
              </p>
            </div>
            {/* No bar without a model: nothing is being indexed (FR-RAG-IX-6, D-82). */}
            {!notInstalled && (progress?.running || status.itemsReady < status.items) && <Progress value={percent} className="h-2" />}
            {progress?.error && (
              <p role="alert" className="text-sm text-destructive">
                {s.stopped(progress.error)}
              </p>
            )}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">{s.loading}</p>
        )}
        <div className="mt-4 flex items-center gap-4">
          <Button variant="outline" onClick={rebuild} disabled={rebuilding}>
            {rebuilding ? s.rebuilding : s.rebuild}
          </Button>
          <p className="text-xs text-muted-foreground">{s.alsoAfterImport}</p>
        </div>
      </SectionCard>

      {/* FR-RAG-EM-1b (Could) */}
      <SectionCard icon={Cloud} tint="slate" title={s.cloudTitle} description={s.cloudDescription}>
        <SettingRow label={<span className="flex items-center gap-2">{s.cloud} <Badge variant="secondary">{s.later}</Badge></span>} hint={s.cloudHint}>
          <Switch disabled checked={false} />
        </SettingRow>
      </SectionCard>
    </div>
  )
}
