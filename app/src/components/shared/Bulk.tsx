// Changing many tasks or pages at once (FR-TASK-UI-7, FR-KN-UI-3, D-70): the bar floating at the
// bottom while something is selected, and the confirmation every action goes through. There is no
// undo, so the confirmation lists every item and its button unlocks only after a 5-second countdown.
import { CheckSquare, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import type { BulkResult } from '@/api/bindings'
import { MoreTagsBadge, TagChip } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'

export type { BulkResult }

/** Seconds before the confirm button can be pressed. */
const UNLOCK_SECONDS = 5

export function BulkBar({ selected, onClear, children }: { selected: number; onClear: () => void; children: React.ReactNode }) {
  const t = useT()
  const b = t.shared.bulk
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-5 z-20 flex justify-center px-6">
      <div role="toolbar" aria-label={b.toolbar} className="pointer-events-auto flex max-w-full flex-wrap items-center gap-1.5 rounded-2xl border bg-card/95 py-2 pr-2 pl-4 shadow-lg backdrop-blur">
        <CheckSquare className="size-4 shrink-0 text-muted-foreground" />
        <span className="mr-1 text-sm font-medium whitespace-nowrap" data-testid="bulk-count">
          {b.selected(selected)}
        </span>
        <span className="mx-1 h-5 w-px bg-border" />
        {children}
        <Button variant="ghost" size="icon-sm" onClick={onClear} aria-label={b.clear} title={b.clear}>
          <X />
        </Button>
      </div>
    </div>
  )
}

/** One line of the confirmation's list: `lead` in a fixed-width font (a task key), `detail` at the
 *  right (muted text, or a pill such as the current status). */
export type BulkItem = { id: string; label: string; lead?: string; detail?: React.ReactNode }

export function BulkConfirmDialog({
  open,
  onClose,
  ...body
}: {
  open: boolean
  onClose: () => void
} & BodyProps) {
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {/* The body mounts on each opening, so the countdown starts again. */}
      <DialogContent className="sm:max-w-xl">{open && <ConfirmBody onClose={onClose} {...body} />}</DialogContent>
    </Dialog>
  )
}

type BodyProps = {
  title: string
  description: React.ReactNode
  items: BulkItem[]
  confirmLabel: string
  destructive?: boolean
  /** False while the change still needs input (a label, a category). */
  ready?: boolean
  /**
   * Why the change can't be made as it stands (external mode's file-name clashes, FR-KN-EXT-10):
   * shown under the inputs, and the button stays locked whatever the countdown says.
   */
  blocked?: React.ReactNode
  /** Inputs for the change, shown above the list. */
  children?: React.ReactNode
  run: () => Promise<BulkResult>
  /** The notice when nothing was skipped, e.g. "3 tasks set to Done". */
  doneMessage: (done: number) => string
  /** After a run, whether or not some items were skipped. */
  onDone?: (result: BulkResult) => void
}

function ConfirmBody({ title, description, items, confirmLabel, destructive, ready = true, blocked, children, run, doneMessage, onDone, onClose }: BodyProps & { onClose: () => void }) {
  const left = useCountdown(UNLOCK_SECONDS)
  const [running, setRunning] = useState(false)
  const [result, setResult] = useState<BulkResult | null>(null)
  const t = useT()
  const b = t.shared.bulk
  const titles = new Map(items.map((i) => [i.id, i.lead ? `${i.lead} ${i.label}` : i.label]))

  const confirm = () => {
    setRunning(true)
    run().then(
      (r) => {
        onDone?.(r)
        if (r.skipped.length === 0) {
          toast.success(doneMessage(r.done))
          onClose()
        } else {
          setResult(r)
        }
      },
      (e) => {
        toast.error(errorMessage(e))
        setRunning(false)
      },
    )
  }

  if (result)
    return (
      <>
        <DialogHeader>
          <DialogTitle>{b.skippedTitle(result.skipped.length)}</DialogTitle>
          <DialogDescription>{b.skippedDescription(result.done > 0 ? doneMessage(result.done) : null)}</DialogDescription>
        </DialogHeader>
        <ul className="max-h-72 space-y-1 overflow-y-auto rounded-xl border p-2" data-testid="bulk-skipped">
          {result.skipped.map((s) => (
            <li key={s.id} className="rounded-lg px-2 py-1.5 text-sm">
              <p className="truncate font-medium">{s.title || titles.get(s.id) || b.gone}</p>
              <p className="text-xs text-muted-foreground">{s.reason}</p>
            </li>
          ))}
        </ul>
        <DialogFooter>
          <Button onClick={onClose}>{t.common.close}</Button>
        </DialogFooter>
      </>
    )

  return (
    <>
      <DialogHeader>
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription>{description}</DialogDescription>
      </DialogHeader>
      {children}
      {blocked}
      <ul className="max-h-72 divide-y overflow-y-auto rounded-xl border px-1" aria-label={b.items} data-testid="bulk-items">
        {items.map((i) => (
          <li key={i.id} className="flex min-w-0 items-center gap-3 px-2 py-1.5 text-sm">
            {i.lead && <span className="w-20 shrink-0 font-mono text-xs text-muted-foreground">{i.lead}</span>}
            <span className="min-w-0 flex-1 truncate">{i.label}</span>
            {i.detail && <span className="flex max-w-[45%] shrink-0 items-center gap-1.5 truncate text-xs text-muted-foreground">{i.detail}</span>}
          </li>
        ))}
      </ul>
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          {t.common.cancel}
        </Button>
        <Button
          variant={destructive ? 'destructive' : 'default'}
          disabled={left > 0 || !ready || !!blocked || running}
          onClick={confirm}
          className={cn(left > 0 && 'tabular-nums')}
          data-testid="bulk-confirm"
        >
          {running ? b.working : left > 0 ? b.countdown(confirmLabel, left) : confirmLabel}
        </Button>
      </DialogFooter>
    </>
  )
}

/**
 * Picks a label or tag as coloured chips (styled, unlike the browser's own suggestion list).
 * With `typed`, a box above lets a new name be written; the chips then follow what is typed.
 */
export function NamePicker({
  names,
  value,
  onChange,
  typed,
  noun,
}: {
  names: string[]
  value: string
  onChange: (v: string) => void
  /** Allow a name that isn't in `names` (adding); without it only a chip can be picked (removing). */
  typed?: boolean
  noun: 'label' | 'tag'
}) {
  const q = value.trim().toLowerCase()
  const p = useT().shared.namePicker[noun]
  const shown = typed ? names.filter((n) => !q || n.toLowerCase().includes(q)) : names
  return (
    <div className="space-y-2">
      {typed && <Input autoFocus value={value} onChange={(e) => onChange(e.target.value)} placeholder={p.placeholder} aria-label={p.toAdd} />}
      {shown.length > 0 && (
        <div>
          <p className="mb-1.5 text-xs text-muted-foreground">{typed ? p.existing : p.toRemove}</p>
          <div className="flex max-h-28 flex-wrap gap-1.5 overflow-y-auto p-0.5" role="listbox" aria-label={p.list}>
            {shown.map((n) => {
              const picked = n.toLowerCase() === q
              return (
                <button key={n} type="button" role="option" aria-selected={picked} onClick={() => onChange(n)} className="rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  <TagChip tag={n} className={cn('cursor-pointer hover:opacity-80', picked && 'ring-2 ring-primary ring-offset-1 ring-offset-background')} />
                </button>
              )
            })}
          </div>
        </div>
      )}
      {typed && q && !names.some((n) => n.toLowerCase() === q) && (
        <p className="text-xs text-muted-foreground">{p.isNew(value.trim())}</p>
      )}
    </div>
  )
}

/** Labels or tags in a confirmation row: two chips, the rest as "+N". */
export function NameChips({ names }: { names: string[] }) {
  return (
    <>
      {names.slice(0, 2).map((n) => (
        <TagChip key={n} tag={n} className="min-w-0" />
      ))}
      <MoreTagsBadge hidden={names.slice(2)} />
    </>
  )
}

/** Counts down once a second from `seconds` to 0. */
function useCountdown(seconds: number) {
  const [left, setLeft] = useState(seconds)
  useEffect(() => {
    if (left <= 0) return
    const t = window.setTimeout(() => setLeft(left - 1), 1000)
    return () => window.clearTimeout(t)
  }, [left])
  return left
}
