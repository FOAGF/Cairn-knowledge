import { Check, Minus, TriangleAlert } from 'lucide-react'
import { useEffect, useState } from 'react'
import { errorMessage } from '@/api/client'
import { type ModelConfig, type TestCheck, testModel, useUpdateModel } from '@/api/providers'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { getT, useT } from '@/i18n'

/** `effort:high` becomes "Effort: high". */
function checkLabel(name: string): string {
  const t = getT().settings.testDialog
  if (name === 'basic') return t.checks.basic
  if (name === 'stream') return t.checks.stream
  if (name === 'image') return t.checks.image
  if (name === 'web') return t.checks.web
  if (name.startsWith('effort:')) return t.effort(name.slice('effort:'.length))
  return name
}

function CheckRow({ check }: { check: TestCheck }) {
  const t = useT()
  const [Icon, color] =
    check.ok === true ? [Check, 'text-tint-green-fg'] : check.ok === false ? [TriangleAlert, 'text-destructive'] : [Minus, 'text-muted-foreground']
  return (
    <li className="flex items-start gap-2.5 py-2">
      <Icon className={`mt-0.5 size-4 shrink-0 ${color}`} aria-label={check.ok === true ? t.settings.testDialog.passed : check.ok === false ? t.common.failed : t.common.skipped} />
      <div className="min-w-0">
        <p className="text-sm font-medium">{checkLabel(check.name)}</p>
        <p className="text-xs break-words text-muted-foreground">{check.detail}</p>
      </div>
    </li>
  )
}

/** *Test connection* for one model (UI-SET-2, FR-LLM-EF-4): runs when opened, shows each check. */
export function TestDialog({ model, onClose }: { model: ModelConfig | null; onClose: () => void }) {
  // The answer is stored with the run it belongs to; an answer from an older run counts as "running".
  const [answer, setAnswer] = useState<{ run: string; result: { checks: TestCheck[] } | { error: string } } | null>(null)
  const [run, setRun] = useState(0)
  const modelId = model?.id
  const runKey = `${modelId}:${run}`
  const state = answer?.run === runKey ? answer.result : 'running'
  const update = useUpdateModel()
  const t = useT()
  const td = t.settings.testDialog
  // The tick as corrected here (the `model` prop is the one the dialog was opened with).
  const [fixed, setFixed] = useState<{ id: string; value: boolean } | null>(null)
  const ticked = model && fixed?.id === model.id ? fixed.value : model?.webSearch
  // FR-LLM-AS-12: the web check says whether the provider really searches; offer to fix a tick that disagrees.
  const web = typeof state === 'object' && 'checks' in state ? state.checks.find((c) => c.name === 'web') : undefined
  const mismatch = model && web && web.ok !== null && web.ok !== ticked ? web.ok : undefined

  useEffect(() => {
    if (!modelId) return
    let cancelled = false
    testModel(modelId).then(
      (r) => !cancelled && setAnswer({ run: runKey, result: { checks: r.checks } }),
      (e) => !cancelled && setAnswer({ run: runKey, result: { error: errorMessage(e) } }),
    )
    return () => {
      cancelled = true
    }
  }, [modelId, runKey])

  return (
    <Dialog open={!!model} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{td.title}</DialogTitle>
          <DialogDescription>{model?.label}</DialogDescription>
        </DialogHeader>
        {state === 'running' && <p className="py-4 text-sm text-muted-foreground">{td.testing}</p>}
        {typeof state === 'object' && 'error' in state && (
          <p className="flex items-start gap-2 rounded-lg bg-destructive/10 px-3 py-2 text-sm text-destructive">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" /> {state.error}
          </p>
        )}
        {typeof state === 'object' && 'checks' in state && <ul className="divide-y">{state.checks.map((c) => <CheckRow key={c.name} check={c} />)}</ul>}
        {model && mismatch !== undefined && (
          <div className="flex flex-wrap items-center gap-2 rounded-lg bg-tint-amber px-3 py-2 text-sm text-tint-amber-fg">
            <p className="min-w-0 flex-1">
              {mismatch ? td.searchesNotTicked : td.didNotSearch}
            </p>
            <Button size="sm" variant="outline" disabled={update.isPending} onClick={() => update.mutate({ id: model.id, patch: { webSearch: mismatch } }, { onSuccess: () => setFixed({ id: model.id, value: mismatch }) })}>
              {mismatch ? td.tick : td.untick}
            </Button>
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" disabled={state === 'running'} onClick={() => setRun((n) => n + 1)}>{td.runAgain}</Button>
          <Button onClick={onClose}>{t.common.close}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
