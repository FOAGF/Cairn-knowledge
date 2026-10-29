// The composer's `#` / `[[` suggestion list (FR-LLM-AS-14, D-81); `references.tsx` drives it.
import { FileText } from 'lucide-react'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'

export type Suggestion = { kind: 'task'; key: string; title: string } | { kind: 'page'; id: string; title: string }

export function SuggestionList({
  kind,
  items,
  active,
  onPick,
  onHover,
}: {
  kind: 'task' | 'page'
  items: Suggestion[]
  active: number
  onPick: (s: Suggestion) => void
  onHover: (i: number) => void
}) {
  const r = useT().assistant.references
  return (
    <div
      role="listbox"
      aria-label={kind === 'task' ? r.tasks : r.pages}
      className="absolute bottom-full left-3 z-50 mb-2 max-h-72 w-[28rem] max-w-[calc(100%-1.5rem)] overflow-y-auto rounded-xl border bg-popover p-1 text-popover-foreground shadow-md"
    >
      <p className="px-2 pt-1 pb-1.5 text-xs text-muted-foreground">{kind === 'task' ? r.tasks : r.pages}</p>
      {items.length === 0 && <p className="px-2 pb-2 text-sm text-muted-foreground">{kind === 'task' ? r.noTasks : r.noPages}</p>}
      {items.map((s, i) => (
        <button
          key={s.kind === 'task' ? s.key : s.id}
          type="button"
          role="option"
          aria-selected={i === active}
          // Keep the caret in the box.
          onMouseDown={(e) => e.preventDefault()}
          onClick={() => onPick(s)}
          onMouseEnter={() => onHover(i)}
          className={cn('flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm', i === active && 'bg-accent')}
        >
          {s.kind === 'task' ? (
            <span className="shrink-0 font-mono text-xs text-muted-foreground">{s.key}</span>
          ) : (
            <FileText className="size-3.5 shrink-0 text-tint-purple-fg" />
          )}
          <span className="truncate">{s.title}</span>
        </button>
      ))}
    </div>
  )
}
