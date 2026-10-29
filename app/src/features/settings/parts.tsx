import { type ReactNode, useState } from 'react'
import type { AuthMode, ModelConfig, Provider, ProviderKind, RequestFormat } from '@/api/providers'
import { azureStyle } from '@/api/providers'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { cn } from '@/lib/utils'

/** One setting: label + explanation on the left, control on the right. */
export function SettingRow({
  label,
  hint,
  children,
  className,
}: {
  label: ReactNode
  hint?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('flex flex-wrap items-center justify-between gap-x-6 gap-y-2 py-3.5 first:pt-0 last:pb-0', className)}>
      <div className="min-w-0 flex-1 basis-64">
        <p className="text-sm font-medium">{label}</p>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  )
}

/** The sign-in choices each kind allows (FR-LLM-PV-1..4). Same rules as `auth_allowed` in Rust. */
export const AUTH_CHOICES: Record<ProviderKind, AuthMode[]> = {
  azure_foundry: ['azure_cli', 'api_key'],
  anthropic: ['api_key'],
  openai_compatible: ['api_key', 'none'],
  local: ['none', 'api_key'],
  deepseek: ['api_key'],
}

/** A form field: label above the control. */
export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}

/** Which request formats fit a provider (FR-LLM-PV-7). Same rules as `check_format` in Rust. */
export function allowedFormats(provider: Pick<Provider, 'kind' | 'endpoint'>): RequestFormat[] {
  const claude = provider.kind === 'anthropic' || (provider.kind === 'azure_foundry' && azureStyle(provider.endpoint) === 'anthropic')
  const all: RequestFormat[] = ['chat_completions', 'responses', 'anthropic_messages']
  // D-69: Cairn's DeepSeek web search runs on Chat Completions.
  if (provider.kind === 'deepseek') return ['chat_completions']
  return all.filter((f) => (f === 'anthropic_messages' ? claude : f === 'responses' ? provider.kind !== 'anthropic' : true))
}

/** The format a new model of this provider starts with. */
export function defaultFormat(provider: Pick<Provider, 'kind' | 'endpoint'>): RequestFormat {
  if (provider.kind === 'anthropic') return 'anthropic_messages'
  if (provider.kind === 'azure_foundry') return azureStyle(provider.endpoint) === 'anthropic' ? 'anthropic_messages' : 'responses'
  return 'chat_completions'
}

/** "low · medium · high", or a dash when effort is off. */
export function effortSummary(model: ModelConfig): string {
  if (!model.effort.enabled) return ''
  return (model.effort.levels ?? ['low', 'medium', 'high']).join(' · ')
}

/** Runs a fake progress bar from 0 to 100, then calls `done`. */
export function runFakeProgress(set: (n: number) => void, done: () => void, ms = 2200) {
  let n = 0
  const timer = setInterval(() => {
    n = Math.min(100, n + 4 + Math.random() * 8)
    set(Math.round(n))
    if (n >= 100) {
      clearInterval(timer)
      done()
    }
  }, ms / 20)
  return () => clearInterval(timer)
}

/** A whole number between `min` and `max`, saved as soon as it is valid. */
export function NumberSetting({ value, min, max, onSave, label }: { value: number; min: number; max: number; onSave: (n: number) => void; label: string }) {
  const [text, setText] = useState<string | null>(null)
  const n = Number(text)
  const bad = text !== null && !(Number.isInteger(n) && n >= min && n <= max)
  return (
    <Input
      type="number"
      min={min}
      max={max}
      className="w-28"
      aria-label={label}
      aria-invalid={bad}
      value={text ?? value}
      onChange={(e) => {
        setText(e.target.value)
        const v = Number(e.target.value)
        if (e.target.value !== '' && Number.isInteger(v) && v >= min && v <= max) onSave(v)
      }}
      onBlur={() => setText(null)}
    />
  )
}
