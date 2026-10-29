import { Plus, RefreshCw, X } from 'lucide-react'
import { useState } from 'react'
import {
  DEFAULT_EFFORT_LEVELS,
  type EffortConfig,
  fetchModels,
  type ModelConfig,
  type Provider,
  type RemoteModel,
  type RequestFormat,
  useCreateModel,
  useUpdateModel,
} from '@/api/providers'
import { errorMessage } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { useT } from '@/i18n'
import { Field, allowedFormats, defaultFormat } from './parts'

/** What the form edits. Turned into an `EffortConfig` on save (FR-LLM-EF-4). */
interface Draft {
  label: string
  name: string
  format: RequestFormat
  vision: boolean
  /** FR-LLM-AS-12: the provider offers built-in web search. */
  webSearch: boolean
  /** Tokens as typed; '' = not set. */
  contextSize: string
  effortOn: boolean
  levels: string[]
  /** '' = not set: nothing is sent unless the chat picks a level (FR-LLM-EF-1). */
  defaultLevel: string
  /** false = Auto: Cairn maps the level to the provider's own parameter (FR-LLM-EF-2). */
  customOn: boolean
  custom: Record<string, string>
}

function toDraft(provider: Provider, model: ModelConfig | null): Draft {
  if (!model)
    return {
      label: '',
      name: '',
      format: defaultFormat(provider),
      vision: false,
      webSearch: false,
      contextSize: '',
      effortOn: false,
      levels: [...DEFAULT_EFFORT_LEVELS],
      defaultLevel: '',
      customOn: false,
      custom: {},
    }
  const e = model.effort
  return {
    label: model.label,
    name: model.name,
    format: model.format,
    vision: model.vision,
    webSearch: model.webSearch,
    contextSize: model.contextSize ? String(model.contextSize) : '',
    effortOn: e.enabled,
    levels: e.levels ?? [...DEFAULT_EFFORT_LEVELS],
    defaultLevel: e.defaultLevel ?? '',
    customOn: e.custom !== null,
    custom: e.custom ?? {},
  }
}

function toEffort(d: Draft): EffortConfig {
  const sameAsDefault = d.levels.length === DEFAULT_EFFORT_LEVELS.length && d.levels.every((l, i) => l === DEFAULT_EFFORT_LEVELS[i])
  const custom = d.customOn ? Object.fromEntries(d.levels.map((l) => [l, (d.custom[l] ?? '').trim()])) : null
  return {
    enabled: d.effortOn,
    levels: sameAsDefault ? null : d.levels,
    defaultLevel: d.defaultLevel && d.levels.includes(d.defaultLevel) ? d.defaultLevel : null,
    custom,
  }
}

/** The effort part of the form: enable, levels, default, and how it is sent (FR-LLM-EF-3, -4). */
function EffortEditor({ draft, set }: { draft: Draft; set: (c: Partial<Draft>) => void }) {
  const [newLevel, setNewLevel] = useState('')
  const t = useT()
  const ee = t.settings.effortEditor
  const addLevel = () => {
    const l = newLevel.trim()
    if (!l || draft.levels.some((x) => x.toLowerCase() === l.toLowerCase())) return
    set({ levels: [...draft.levels, l] })
    setNewLevel('')
  }
  const removeLevel = (l: string) =>
    set({ levels: draft.levels.filter((x) => x !== l), defaultLevel: draft.defaultLevel === l ? '' : draft.defaultLevel })
  return (
    <div className="rounded-xl border">
      <div className="flex items-center justify-between p-3">
        <div>
          <p className="text-sm font-medium">{ee.title}</p>
          <p className="text-xs text-muted-foreground">{ee.hint}</p>
        </div>
        <Switch checked={draft.effortOn} onCheckedChange={(v) => set({ effortOn: v })} aria-label={ee.title} />
      </div>
      {draft.effortOn && (
        <div className="space-y-4 border-t p-3">
          <Field label={ee.levels} hint={ee.levelsHint}>
            <div className="flex flex-wrap items-center gap-1.5">
              {draft.levels.map((l) => (
                <span key={l} className="inline-flex items-center gap-1 rounded-full bg-muted py-0.5 pr-1 pl-2.5 text-xs font-medium">
                  {l}
                  <button
                    type="button"
                    className="rounded-full p-0.5 text-muted-foreground hover:bg-background hover:text-foreground disabled:opacity-40"
                    aria-label={ee.removeLevel(l)}
                    disabled={draft.levels.length <= 1}
                    onClick={() => removeLevel(l)}
                  >
                    <X className="size-3" />
                  </button>
                </span>
              ))}
              <Input
                className="h-7 w-28 text-xs"
                placeholder={ee.addLevel}
                value={newLevel}
                onChange={(e) => setNewLevel(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
                    e.preventDefault()
                    addLevel()
                  }
                }}
              />
              <Button type="button" size="icon-sm" variant="ghost" aria-label={ee.addLevel} disabled={!newLevel.trim()} onClick={addLevel}>
                <Plus />
              </Button>
            </div>
          </Field>
          <Field label={ee.defaultLevel} hint={ee.defaultLevelHint}>
            <Select value={draft.defaultLevel || 'none'} onValueChange={(v) => set({ defaultLevel: v === 'none' ? '' : v })}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="none">{t.common.notSet}</SelectItem>
                {draft.levels.map((l) => <SelectItem key={l} value={l}>{l}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <Field label={ee.howSent} hint={draft.customOn ? undefined : ee.autoHint}>
            <Select value={draft.customOn ? 'custom' : 'auto'} onValueChange={(v) => set({ customOn: v === 'custom' })}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">{ee.auto}</SelectItem>
                <SelectItem value="custom">{ee.custom}</SelectItem>
              </SelectContent>
            </Select>
          </Field>
          {draft.customOn && (
            <div className="space-y-2">
              {draft.levels.map((l) => (
                <div key={l} className="flex items-center gap-3">
                  <span className="w-24 shrink-0 truncate text-sm">{l}</span>
                  <Input
                    value={draft.custom[l] ?? ''}
                    onChange={(e) => set({ custom: { ...draft.custom, [l]: e.target.value } })}
                    placeholder={ee.valuePlaceholder}
                    aria-label={ee.valueAria(l)}
                  />
                </div>
              ))}
              <p className="text-xs text-muted-foreground">
                {ee.customNote}
              </p>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

/** Add / edit a model of a provider. FR-LLM-PV-1, -7 (format per model), FR-LLM-EF-3, -4, UI-SET-2. */
export function ModelDialog({ provider, model, onClose }: { provider: Provider; model: ModelConfig | 'new' | null; onClose: () => void }) {
  const [draft, setDraft] = useState<Draft | null>(null)
  const [remote, setRemote] = useState<RemoteModel[] | null>(null)
  const [fetching, setFetching] = useState(false)
  const [fetchError, setFetchError] = useState('')
  const create = useCreateModel()
  const update = useUpdateModel()
  const t = useT()
  const md = t.settings.modelDialog
  // (Re)initialise the form whenever a different model is opened.
  const [lastKey, setLastKey] = useState<unknown>(null)
  if (model !== lastKey) {
    setLastKey(model)
    setDraft(model === null ? null : toDraft(provider, model === 'new' ? null : model))
    setRemote(null)
    setFetchError('')
  }
  if (!draft) return <Dialog open={false} />
  const set = (c: Partial<Draft>) => setDraft({ ...draft, ...c })

  const canList = provider.kind !== 'azure_foundry'
  const formats = allowedFormats(provider)
  const customMissing = draft.effortOn && draft.customOn && draft.levels.some((l) => !(draft.custom[l] ?? '').trim())
  const contextBad = draft.contextSize.trim() !== '' && !/^[1-9]\d{0,8}$/.test(draft.contextSize.trim())
  const valid = !!draft.name.trim() && !customMissing && !contextBad && !(draft.effortOn && draft.levels.length === 0)

  const doFetch = async () => {
    setFetching(true)
    setFetchError('')
    try {
      setRemote(await fetchModels(provider.id))
    } catch (e) {
      setFetchError(errorMessage(e))
    } finally {
      setFetching(false)
    }
  }
  const pick = (id: string) => {
    const r = remote?.find((x) => x.id === id)
    // The label follows the pick only while it is empty or still the old name.
    set({ name: id, label: !draft.label.trim() || draft.label === draft.name ? (r?.label ?? id) : draft.label })
  }

  const save = () => {
    const fields = { name: draft.name.trim(), label: draft.label.trim(), format: draft.format, vision: draft.vision, webSearch: draft.webSearch, effort: toEffort(draft) }
    const done = { onSuccess: onClose }
    const size = draft.contextSize.trim() ? Number(draft.contextSize.trim()) : null
    if (model === 'new') create.mutate({ providerId: provider.id, ...fields, contextSize: size }, done)
    // 0 clears the setting (ModelPatch).
    else if (model) update.mutate({ id: model.id, patch: { ...fields, contextSize: size ?? 0 } }, done)
  }
  const saving = create.isPending || update.isPending

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      {/* The fields scroll; the header and the Save bar stay in view. */}
      <DialogContent className="flex max-h-[90vh] flex-col sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{model === 'new' ? md.titleAdd : md.titleEdit}</DialogTitle>
          <DialogDescription>{provider.name}</DialogDescription>
        </DialogHeader>
        <div className="-mx-4 min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-1">
          <Field
            label={provider.kind === 'azure_foundry' ? md.deploymentName : md.modelName}
            hint={canList ? undefined : md.azureHint}
          >
            <div className="flex gap-2">
              <Input value={draft.name} onChange={(e) => set({ name: e.target.value })} placeholder={md.namePlaceholder} />
              {canList && (
                <Button type="button" variant="outline" disabled={fetching} onClick={doFetch}>
                  <RefreshCw className={fetching ? 'animate-spin' : ''} /> {md.fetch}
                </Button>
              )}
            </div>
            {fetchError && <p className="text-xs text-destructive">{fetchError}</p>}
            {remote && remote.length === 0 && <p className="text-xs text-muted-foreground">{md.noneListed}</p>}
            {remote && remote.length > 0 && (
              <Select value={remote.some((r) => r.id === draft.name) ? draft.name : ''} onValueChange={pick}>
                <SelectTrigger className="w-full"><SelectValue placeholder={md.pickOne(remote.length)} /></SelectTrigger>
                <SelectContent>
                  {remote.map((r) => <SelectItem key={r.id} value={r.id}>{r.label && r.label !== r.id ? `${r.label} (${r.id})` : r.id}</SelectItem>)}
                </SelectContent>
              </Select>
            )}
          </Field>
          <Field label={md.label} hint={md.labelHint}>
            <Input value={draft.label} onChange={(e) => set({ label: e.target.value })} placeholder={draft.name || md.labelPlaceholder} />
          </Field>
          <Field label={md.format}>
            <Select value={draft.format} onValueChange={(v) => set({ format: v as RequestFormat })}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                {formats.map((f) => <SelectItem key={f} value={f}>{t.settings.requestFormats[f]}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <div className="flex items-center justify-between rounded-xl border p-3">
            <div><p className="text-sm font-medium">{md.vision}</p><p className="text-xs text-muted-foreground">{md.visionHint}</p></div>
            <Switch checked={draft.vision} onCheckedChange={(v) => set({ vision: v })} aria-label={md.vision} />
          </div>
          <div className="flex items-center justify-between gap-4 rounded-xl border p-3">
            <div>
              <p className="text-sm font-medium">{md.webSearch}</p>
              <p className="text-xs text-muted-foreground">
                {provider.kind === 'deepseek' ? md.webSearchDeepSeek : md.webSearchOther}
              </p>
            </div>
            <Switch checked={draft.webSearch} onCheckedChange={(v) => set({ webSearch: v })} aria-label={md.webSearch} />
          </div>
          <Field label={md.contextSize} hint={md.contextHint}>
            <div className="flex items-center gap-2">
              <Input
                className="w-40"
                inputMode="numeric"
                value={draft.contextSize}
                onChange={(e) => set({ contextSize: e.target.value })}
                placeholder={md.contextPlaceholder}
                aria-label={md.contextAria}
                aria-invalid={contextBad}
              />
              <span className="text-xs text-muted-foreground">{md.tokens}</span>
            </div>
            {contextBad && <p className="text-xs text-destructive">{md.contextInvalid}</p>}
          </Field>
          <EffortEditor draft={draft} set={set} />
        </div>
        <DialogFooter className="shrink-0">
          <Button variant="outline" onClick={onClose}>{t.common.cancel}</Button>
          <Button disabled={!valid || saving} onClick={save}>{saving ? md.saving : t.common.save}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
