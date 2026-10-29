import { Check, Eye, KeyRound, Pencil, Plug, Plus, Settings2, Trash2, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import {
  type AuthMode,
  type AzureSignin,
  azureStyle,
  checkAzureSignin,
  type ModelConfig,
  type Provider,
  type ProviderKind,
  useCreateProvider,
  useDeleteModel,
  useDeleteProvider,
  useModelList,
  useProviders,
  useRemoveProviderKey,
  useSetProviderKey,
  useUpdateProvider,
} from '@/api/providers'
import { EmptyState, SectionCard } from '@/components/shared/PageHeader'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { useT } from '@/i18n'
import { ModelDialog } from './ModelDialog'
import { AUTH_CHOICES, effortSummary, Field } from './parts'
import { TestDialog } from './TestDialog'

/**
 * What the form shows per provider kind (FR-LLM-PV-1..4): placeholder and first sign-in choice (the
 * hint is `settings.providerDialog.kindHints`). Its order is the order of the kind picker.
 */
const KIND_HINTS: Record<ProviderKind, { placeholder: string; auth: AuthMode }> = {
  azure_foundry: { placeholder: 'https://<resource>.services.ai.azure.com/openai/v1', auth: 'azure_cli' },
  anthropic: { placeholder: 'https://api.anthropic.com', auth: 'api_key' },
  openai_compatible: { placeholder: 'https://api.openai.com/v1', auth: 'api_key' },
  local: { placeholder: 'http://localhost:11434/v1', auth: 'none' },
  deepseek: { placeholder: 'https://api.deepseek.com', auth: 'api_key' },
}

const KIND_TINT = { azure_foundry: 'blue', anthropic: 'amber', openai_compatible: 'blue', local: 'green', deepseek: 'purple' } as const

/** FR-LLM-PV-1..5: add a provider, or edit one (the kind stays fixed). */
function ProviderDialog({ provider, open, onOpenChange }: { provider?: Provider; open: boolean; onOpenChange: (o: boolean) => void }) {
  const create = useCreateProvider()
  const update = useUpdateProvider()
  const [kind, setKind] = useState<ProviderKind>(provider?.kind ?? 'azure_foundry')
  const [name, setName] = useState(provider?.name ?? '')
  const [endpoint, setEndpoint] = useState(provider?.endpoint ?? '')
  const [apiVersion, setApiVersion] = useState(provider?.apiVersion ?? '')
  const [auth, setAuth] = useState<AuthMode>(provider?.auth ?? KIND_HINTS.azure_foundry.auth)
  const t = useT()
  const pd = t.settings.providerDialog
  const hints = KIND_HINTS[kind]
  const style = kind === 'azure_foundry' && endpoint.trim() ? azureStyle(endpoint) : null
  const needsEndpoint = kind === 'azure_foundry' || kind === 'openai_compatible'
  const valid = !!name.trim() && (!needsEndpoint || !!endpoint.trim())

  const pickKind = (k: ProviderKind) => {
    setKind(k)
    setAuth(KIND_HINTS[k].auth)
    setApiVersion('')
  }
  const save = () => {
    const done = { onSuccess: () => onOpenChange(false) }
    const fields = { name: name.trim(), endpoint: endpoint.trim(), auth, apiVersion: style === 'classic' ? apiVersion.trim() : '' }
    if (provider) update.mutate({ id: provider.id, patch: fields }, done)
    else create.mutate({ kind, ...fields }, { onSuccess: () => { setName(''); setEndpoint(''); onOpenChange(false) } })
  }
  const saving = create.isPending || update.isPending

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{provider ? pd.titleEdit : pd.titleAdd}</DialogTitle>
          <DialogDescription>{pd.description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <Field label={pd.kind}>
            <Select value={kind} onValueChange={(v) => pickKind(v as ProviderKind)} disabled={!!provider}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                {(Object.keys(KIND_HINTS) as ProviderKind[]).map((k) => <SelectItem key={k} value={k}>{t.settings.providerKinds[k]}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          <Field label={t.common.name}>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder={pd.namePlaceholder} />
          </Field>
          <Field label={needsEndpoint ? pd.endpoint : pd.endpointOptional} hint={pd.kindHints[kind]}>
            <Input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder={hints.placeholder} />
            {style && <p className="text-xs font-medium text-tint-blue-fg">{pd.style[style]}</p>}
          </Field>
          {style === 'classic' && (
            <Field label={pd.apiVersion} hint={pd.apiVersionHint}>
              <Input value={apiVersion} onChange={(e) => setApiVersion(e.target.value)} placeholder="2025-04-01-preview" />
            </Field>
          )}
          <Field label={pd.authentication}>
            <Select value={auth} onValueChange={(v) => setAuth(v as AuthMode)}>
              <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                {AUTH_CHOICES[kind].map((a) => <SelectItem key={a} value={a}>{t.settings.authModes[a]}</SelectItem>)}
              </SelectContent>
            </Select>
          </Field>
          {!provider && auth === 'api_key' && <p className="text-xs text-muted-foreground">{pd.keyLater}</p>}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t.common.cancel}</Button>
          <Button disabled={!valid || saving} onClick={save}>{provider ? t.common.save : pd.add}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Authentication block, depends on the auth type. FR-LLM-PV-1a, NFR-SEC-1. The key is never shown. */
function AuthSection({ provider }: { provider: Provider }) {
  const setKey = useSetProviderKey()
  const removeKey = useRemoveProviderKey()
  const [key, setKeyText] = useState('')
  const [editing, setEditing] = useState(false)
  const [signin, setSignin] = useState<null | 'checking' | AzureSignin | { error: string }>(null)
  const t = useT()
  const a = t.settings.auth

  if (provider.auth === 'none') return <p className="text-sm text-muted-foreground">{a.none}</p>

  if (provider.auth === 'azure_cli') {
    const check = async () => {
      setSignin('checking')
      try {
        setSignin(await checkAzureSignin())
      } catch (e) {
        setSignin({ error: errorMessage(e) })
      }
    }
    const result = typeof signin === 'object' && signin !== null ? signin : null
    return (
      <div className="flex flex-wrap items-center gap-3">
        <p className="flex items-start gap-2 text-sm">
          {result && 'error' in result ? (
            <><TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" /> <span className="text-destructive">{result.error}</span></>
          ) : result ? (
            result.signedIn ? (
              <><Check className="mt-0.5 size-4 shrink-0 text-tint-green-fg" /> <span>{a.signedIn(result.account ? <span className="font-medium">{result.account}</span> : null, result.detail)}</span></>
            ) : (
              <><TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" /> <span>{a.notSignedIn(result.detail)}</span></>
            )
          ) : (
            <span className="text-muted-foreground">{a.notChecked}</span>
          )}
        </p>
        <Button variant="outline" size="sm" disabled={signin === 'checking'} onClick={check}>
          {signin === 'checking' ? a.checking : a.check}
        </Button>
      </div>
    )
  }

  const showInput = editing || !provider.hasKey
  const saveKey = () =>
    setKey.mutate(
      { id: provider.id, key },
      {
        onSuccess: () => {
          setKeyText('')
          setEditing(false)
          toast.success(a.keyStored)
        },
      },
    )
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        {showInput ? (
          <>
            <Input type="password" className="w-72" placeholder={a.keyPlaceholder} value={key} onChange={(e) => setKeyText(e.target.value)} autoComplete="off" aria-label={a.keyAria} />
            <Button size="sm" disabled={!key.trim() || setKey.isPending} onClick={saveKey}>
              <KeyRound /> {provider.hasKey ? a.replaceKey : a.setKey}
            </Button>
            {editing && <Button size="sm" variant="ghost" onClick={() => { setEditing(false); setKeyText('') }}>{t.common.cancel}</Button>}
          </>
        ) : (
          <>
            <Input readOnly disabled className="w-72 tracking-widest" value="••••••••••••••••••••" aria-label={a.keySavedAria} />
            <Button size="sm" variant="outline" onClick={() => setEditing(true)}>{a.replace}</Button>
            <Button size="sm" variant="destructive" disabled={removeKey.isPending} onClick={() => removeKey.mutate(provider.id)}>{t.common.remove}</Button>
          </>
        )}
      </div>
      <p className="text-xs text-muted-foreground">{a.storedNote}</p>
    </div>
  )
}

function ModelsTable({ provider, models, off }: { provider: Provider; models: ModelConfig[]; off: boolean }) {
  const deleteModel = useDeleteModel()
  const [editing, setEditing] = useState<ModelConfig | 'new' | null>(null)
  const [testing, setTesting] = useState<ModelConfig | null>(null)
  const t = useT()
  const mt = t.settings.modelsTable
  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">{mt.title}</p>
        <Button size="sm" variant="ghost" onClick={() => setEditing('new')}><Plus /> {mt.add}</Button>
      </div>
      {models.length === 0 ? (
        <p className="rounded-xl border border-dashed p-4 text-center text-sm text-muted-foreground">{mt.empty}</p>
      ) : (
        <div className={/* i18n-ignore: class names */ `overflow-hidden rounded-xl border ${off ? 'grayscale' : ''}`}>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{mt.label}</TableHead>
                <TableHead>{provider.kind === 'azure_foundry' ? mt.deployment : mt.modelName}</TableHead>
                <TableHead>{mt.format}</TableHead>
                <TableHead>{mt.effort}</TableHead>
                <TableHead className="text-center">{mt.vision}</TableHead>
                <TableHead className="w-28" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {models.map((m) => (
                <TableRow key={m.id}>
                  <TableCell className="font-medium">{m.label}</TableCell>
                  <TableCell className="font-mono text-xs text-muted-foreground">{m.name}</TableCell>
                  <TableCell>{t.settings.requestFormats[m.format]}</TableCell>
                  <TableCell className="text-xs">{m.effort.enabled ? effortSummary(m) : <span className="text-muted-foreground">–</span>}</TableCell>
                  <TableCell className="text-center">{m.vision ? <Eye className="mx-auto size-4 text-tint-green-fg" /> : <span className="text-muted-foreground">–</span>}</TableCell>
                  <TableCell>
                    <div className="flex justify-end gap-0.5">
                      <Button variant="ghost" size="icon-sm" aria-label={mt.testAria(m.label)} title={mt.testTitle} disabled={off} onClick={() => setTesting(m)}><Plug /></Button>
                      <Button variant="ghost" size="icon-sm" aria-label={mt.editAria(m.label)} onClick={() => setEditing(m)}><Pencil /></Button>
                      <Button variant="ghost" size="icon-sm" aria-label={mt.deleteAria(m.label)} disabled={deleteModel.isPending} onClick={() => deleteModel.mutate(m.id)}><Trash2 /></Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
      <ModelDialog provider={provider} model={editing} onClose={() => setEditing(null)} />
      <TestDialog model={testing} onClose={() => setTesting(null)} />
    </div>
  )
}

function ProviderCard({ provider }: { provider: Provider }) {
  const allModels = useModelList()
  const update = useUpdateProvider()
  const remove = useDeleteProvider()
  const off = !provider.enabled
  const mine = allModels.filter((m) => m.providerId === provider.id)
  const [editOpen, setEditOpen] = useState(false)
  const [confirmDel, setConfirmDel] = useState(false)
  const t = useT()
  const pc = t.settings.providerCard

  return (
    <SectionCard
      icon={Plug}
      tint={KIND_TINT[provider.kind]}
      title={provider.name}
      description={provider.endpoint || (provider.kind === 'anthropic' ? pc.claudeDefault : '')}
      action={
        <div className="flex items-center gap-3">
          {off && <Badge variant="outline" className="text-muted-foreground">{pc.disabled}</Badge>}
          <Badge variant="secondary" className={off ? 'opacity-60' : ''}>{t.settings.providerKinds[provider.kind]}</Badge>
          <label className="flex items-center gap-2 text-sm font-medium">
            {pc.enabled}
            <Switch checked={!off} disabled={update.isPending} onCheckedChange={(v) => update.mutate({ id: provider.id, patch: { enabled: v } })} aria-label={pc.enableAria(provider.name)} />
          </label>
          <Button variant="ghost" size="icon-sm" aria-label={pc.editAria} onClick={() => setEditOpen(true)}><Settings2 /></Button>
          <Button variant="ghost" size="icon-sm" aria-label={pc.deleteAria} onClick={() => setConfirmDel(true)}><Trash2 /></Button>
        </div>
      }
    >
      {off && (
        <p className="mb-4 rounded-xl border border-dashed bg-muted/40 p-3 text-xs text-muted-foreground">
          {pc.offNote}
        </p>
      )}
      <div className={`space-y-5 ${off ? 'opacity-60' : ''}`}>
        <div>
          <p className="mb-2 text-xs font-medium tracking-wide text-muted-foreground uppercase">{pc.authentication(t.settings.authModes[provider.auth])}</p>
          <AuthSection provider={provider} />
        </div>
        <ModelsTable provider={provider} models={mine} off={off} />
      </div>
      {/* Keyed so the form restarts from the saved values each time it opens. */}
      {editOpen && <ProviderDialog key={provider.updatedAt} provider={provider} open onOpenChange={setEditOpen} />}
      <Dialog open={confirmDel} onOpenChange={setConfirmDel}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{pc.deleteTitle(provider.name)}</DialogTitle>
            <DialogDescription>{pc.deleteBody(mine.length)}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmDel(false)}>{t.common.cancel}</Button>
            <Button variant="destructive" disabled={remove.isPending} onClick={() => remove.mutate(provider.id, { onSuccess: () => setConfirmDel(false) })}>{t.common.delete}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </SectionCard>
  )
}

export function ProvidersTab() {
  const { data: providers, isPending, error } = useProviders()
  const [adding, setAdding] = useState(false)
  const m = useT().settings.providers
  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">{m.intro}</p>
        <Button onClick={() => setAdding(true)}><Plus /> {m.add}</Button>
      </div>
      {error && <p className="text-sm text-destructive">{errorMessage(error)}</p>}
      {!isPending && providers?.length === 0 && (
        <EmptyState icon={Plug} title={m.emptyTitle}>
          <p>{m.emptyBody}</p>
          <Button className="mt-4" onClick={() => setAdding(true)}><Plus /> {m.add}</Button>
        </EmptyState>
      )}
      {providers?.map((p) => <ProviderCard key={p.id} provider={p} />)}
      {/* Mounted only while open, so each new provider starts from an empty form. */}
      {adding && <ProviderDialog open onOpenChange={setAdding} />}
    </div>
  )
}
