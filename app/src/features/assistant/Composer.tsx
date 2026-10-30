import { useQueryClient } from '@tanstack/react-query'
import { effortLevels, useModelList, useProviderList } from '@/api/providers'
import { useSettings } from '@/api/settings'
import { useHistoryUsage, widenHistory } from '@/api/assistant'
import { errorMessage } from '@/api/client'
import { Brain, ChevronDown, FolderKanban, Globe, Info, Paperclip, SendHorizontal, TriangleAlert, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { toast } from 'sonner'
import { cn } from '@/lib/utils'
import { Link } from 'react-router'
import { CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectGroup, SelectItem, SelectLabel, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useT } from '@/i18n'
import type { Effort } from '@/lib/types'
import { FileChip } from './FileChip'
import { attachmentKind } from './fileDraft'
import { useReferences } from './references'
import { blanksOf } from './suggestions'
import { useCategoryList } from '@/api/categories'

/** `unset` = let the model's default level apply (FR-LLM-EF-1). */
export type EffortChoice = 'unset' | Effort

interface Props {
  /** The open conversation, for the history left (FR-LLM-AS-10). */
  conversationId?: string
  /** 🌐 for the next message (FR-LLM-AS-12). `webAllowed` is false when the message can't be a web search (a file, a draft being refined). */
  web: boolean
  onWebChange: (on: boolean) => void
  webAllowed: boolean
  modelId: string
  onModelChange: (id: string) => void
  effort: EffortChoice
  onEffortChange: (e: EffortChoice) => void
  scope: string[]
  onScopeChange: (ids: string[]) => void
  disabled?: boolean
  /** No enabled provider at all: sending is blocked. */
  noProvider?: boolean
  /** Name of the disabled provider the conversation's model belonged to. */
  disabledProviderName?: string
  /** Attached file (create a page from it, FR-KN-UP-1..3). */
  file: File | null
  onFileChange: (f: File | null) => void
  onBrowse: () => void
  fileError: string
  onSend: (text: string, file: File | null) => void
  /** A template picked on the empty conversation (D-64): fills the box and selects its first blank. A new object per click. */
  template?: { text: string }
}

export function Composer({
  conversationId,
  web,
  onWebChange,
  webAllowed,
  modelId,
  onModelChange,
  effort,
  onEffortChange,
  scope,
  onScopeChange,
  disabled,
  noProvider,
  disabledProviderName,
  file,
  onFileChange,
  onBrowse,
  fileError,
  onSend,
  template,
}: Props) {
  const models = useModelList()
  const providers = useProviderList()
  const categories = useCategoryList()
  const [text, setText] = useState('')
  const area = useRef<HTMLTextAreaElement>(null)
  const t = useT()
  const c = t.assistant.composer
  // FR-LLM-AS-14: `#` and `[[` suggest tasks and pages.
  const refs = useReferences(text, setText, area)
  // D-64: the blanks of the template in the box. Tab selects the next one, and sending waits until
  // they are filled in. Only the template's own blanks count, never brackets the user typed.
  const [blanks, setBlanks] = useState<string[]>([])
  const open = blanks.filter((b) => text.includes(b))
  /** Selects the first of `list` that starts at or after `from` (else the first in the text). */
  const selectBlank = (value: string, list: string[], from: number) => {
    const el = area.current
    const at = (start: number) =>
      list
        .map((b) => ({ b, i: value.indexOf(b, start) }))
        .filter((x) => x.i >= 0)
        .sort((x, y) => x.i - y.i)[0]
    const hit = at(from) ?? at(0)
    if (!el || !hit) return null
    el.focus()
    el.setSelectionRange(hit.i, hit.i + hit.b.length)
    return hit.b
  }
  useEffect(() => {
    if (!template) return
    const list = blanksOf(template.text)
    setText(template.text)
    setBlanks(list)
    // After the new text is in the box.
    requestAnimationFrame(() => selectBlank(template.text, list, 0))
    // Runs once per picked template; selectBlank only reads the box.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [template])
  const model = models.find((m) => m.id === modelId)
  const levels = effortLevels(model)
  const activeCategories = categories.filter((p) => p.status === 'active')

  const settings = useSettings()
  const large = !!file && file.size > settings.largeFileWarningMB * 1024 * 1024 // FR-KN-UP-2: warning only
  const queryClient = useQueryClient()
  const usage = useHistoryUsage(conversationId, modelId || null).data
  const webSupported = !!model?.webSearch
  const webOn = web && webSupported && webAllowed
  const webDisabled = !webSupported || !webAllowed
  const widen = () => {
    if (!conversationId) return
    widenHistory(conversationId, modelId || null).then(
      () => void queryClient.invalidateQueries({ queryKey: ['conversations', 'history'] }),
      (e) => toast.error(errorMessage(e)),
    )
  }
  const canSend = (!!text.trim() || !!file) && !disabled && !noProvider

  const send = () => {
    if (!canSend) return
    if (open.length) {
      const blank = selectBlank(text, open, 0)
      toast.info(c.fillIn(blank ?? open[0]), { description: c.fillInHint })
      return
    }
    onSend(refs.stored(text).trim(), file)
    setText('')
    setBlanks([])
  }
  const toggle = (id: string) => onScopeChange(scope.includes(id) ? scope.filter((x) => x !== id) : [...scope, id])

  return (
    <div>
      <div className="relative rounded-2xl border bg-card shadow-[0_1px_3px_rgba(16,24,40,0.05)] focus-within:ring-2 focus-within:ring-ring/40">
        {refs.list}
        {file && (
          <div className="px-3 pt-3">
            <FileChip name={file.name} size={file.size} kind={attachmentKind(file.name)} large={large} onRemove={() => onFileChange(null)} />
          </div>
        )}
        <Textarea
          ref={area}
          value={text}
          onChange={(e) => {
            setText(e.target.value)
            refs.follow(e.target.value, e.target.selectionStart)
          }}
          onSelect={(e) => refs.follow(e.currentTarget.value, e.currentTarget.selectionStart)}
          onBlur={refs.close}
          onKeyDown={(e) => {
            if (refs.onKeyDown(e)) return
            // Tab moves to the next blank of a template while one is left (D-64); otherwise it moves focus as usual.
            if (e.key === 'Tab' && !e.shiftKey && open.length) {
              e.preventDefault()
              selectBlank(text, open, e.currentTarget.selectionEnd)
              return
            }
            // Enter sends, Shift+Enter = new line. Ignore Enter while an IME (Chinese input) is composing.
            if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault()
              send()
            }
          }}
          rows={2}
          placeholder={file ? c.placeholderWithFile : c.placeholder}
          className="min-h-[3.5rem] resize-none border-0 bg-transparent px-4 pt-3 shadow-none focus-visible:ring-0 dark:bg-transparent"
        />
        <div className="flex flex-wrap items-center gap-2 px-3 pb-3">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button type="button" variant="outline" size="icon-sm" className="rounded-full" onClick={onBrowse} aria-label={c.attach}>
                <Paperclip />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{c.attachHint}</TooltipContent>
          </Tooltip>
          {/* FR-LLM-ST-2: pick the model without opening Settings */}
          <Select value={modelId} onValueChange={onModelChange}>
            <SelectTrigger size="sm" className="w-auto gap-2 rounded-full">
              <SelectValue placeholder={c.model} />
            </SelectTrigger>
            <SelectContent>
              {providers
                .filter((p) => p.enabled)
                .map((p) => {
                  const ms = models.filter((m) => m.providerId === p.id)
                  if (!ms.length) return null
                  return (
                    <SelectGroup key={p.id}>
                      <SelectLabel>{p.name}</SelectLabel>
                      {ms.map((m) => (
                        <SelectItem key={m.id} value={m.id}>
                          {m.label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  )
                })}
            </SelectContent>
          </Select>

          {/* FR-LLM-EF-1..3: only for models that support effort; "Not set" sends nothing */}
          {levels.length > 0 && (
            <div className="flex items-center gap-1">
              <Select value={levels.includes(effort) ? effort : 'unset'} onValueChange={(v) => onEffortChange(v as EffortChoice)}>
                <SelectTrigger size="sm" className="w-auto gap-2 rounded-full">
                  <Brain className="size-3.5 text-muted-foreground" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="unset">
                    {model?.effort.defaultLevel ? c.effortDefault(model.effort.defaultLevel) : c.effortNotSet}
                  </SelectItem>
                  {levels.map((l) => (
                    <SelectItem key={l} value={l}>{c.effort(l)}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Info className="size-3.5 cursor-help text-muted-foreground" />
                </TooltipTrigger>
                <TooltipContent className="max-w-64">{c.effortHint}</TooltipContent>
              </Tooltip>
            </div>
          )}

          {/* FR-LLM-AS-4: category scope, empty = everything */}
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="outline" size="sm" className="rounded-full">
                <FolderKanban /> {scope.length === 0 ? c.allCategories : c.categories(scope.length)}
                <ChevronDown className="opacity-60" />
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-64 p-2">
              <p className="px-2 pt-1 pb-2 text-xs text-muted-foreground">{c.scopeHint}</p>
              {activeCategories.map((p) => (
                <label key={p.id} className="flex cursor-pointer items-center gap-2.5 rounded-lg px-2 py-1.5 text-sm hover:bg-muted">
                  <Checkbox checked={scope.includes(p.id)} onCheckedChange={() => toggle(p.id)} />
                  <CategoryDot color={p.color} />
                  {p.name}
                </label>
              ))}
            </PopoverContent>
          </Popover>
          {scope.map((id) => {
            const p = categories.find((x) => x.id === id)
            if (!p) return null
            return (
              <span
                key={id}
                className="inline-flex items-center gap-1.5 rounded-full bg-tint-slate py-1 pr-1.5 pl-2.5 text-xs font-medium text-tint-slate-fg"
              >
                <CategoryDot color={p.color} className="size-2" />
                {p.name}
                <button onClick={() => toggle(id)} aria-label={c.remove(p.name)} className="rounded-full p-0.5 hover:bg-foreground/10">
                  <X className="size-3" />
                </button>
              </span>
            )
          })}

          {/* FR-LLM-AS-12: always shown, usable only for models ticked "Web search"; one message. The span
              carries the tooltip, since a disabled button gets no hover. */}
          <Tooltip>
            <TooltipTrigger asChild>
              <span className="inline-flex" tabIndex={webDisabled ? 0 : undefined}>
                <Button
                  type="button"
                  variant={webOn ? 'default' : 'outline'}
                  size="icon-sm"
                  className="rounded-full"
                  aria-pressed={webOn}
                  aria-label={c.web}
                  disabled={webDisabled}
                  onClick={() => onWebChange(!web)}
                >
                  <Globe />
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-64">
              {!webSupported ? c.webUnsupported : webAllowed ? c.webHint : c.webUnavailable}
            </TooltipContent>
          </Tooltip>

          <Button
            size="icon"
            className="ml-auto rounded-full"
            onClick={send}
            disabled={!canSend}
            aria-label={c.send}
          >
            <SendHorizontal />
          </Button>
        </div>
      </div>
      {webOn && (
        <p className="mt-2 flex items-start gap-1.5 rounded-lg bg-tint-amber px-3 py-2 text-xs text-tint-amber-fg">
          <Globe className="mt-0.5 size-3.5 shrink-0" />
          {c.webOn}
        </p>
      )}
      {usage && (
        <div className={cn('mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 px-2 text-xs', usage.percentLeft < 25 ? 'text-tint-amber-fg' : 'text-muted-foreground')}>
          <span>{c.historyLeft(Math.round(usage.percentLeft))}</span>
          {usage.canWiden && (
            <Button type="button" size="xs" variant="outline" onClick={widen}>{c.widen}</Button>
          )}
          {usage.atModelLimit && <span>{c.oldestDropped}</span>}
        </div>
      )}
      {fileError && (
        <p role="alert" className="mt-2 flex items-start gap-1.5 px-2 text-xs text-destructive">
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" /> {fileError}
        </p>
      )}
      {noProvider ? (
        <p className="mt-2 flex items-center gap-1.5 px-2 text-xs text-tint-amber-fg">
          <TriangleAlert className="size-3.5 shrink-0" />
          {c.noProvider((label) => (
            <Link to="/settings/providers" className="underline underline-offset-2">
              {label}
            </Link>
          ))}
        </p>
      ) : disabledProviderName ? (
        <p className="mt-2 px-2 text-xs text-muted-foreground">
          {c.fallback(disabledProviderName, model?.label ?? c.anotherModel)}
        </p>
      ) : null}
    </div>
  )
}
