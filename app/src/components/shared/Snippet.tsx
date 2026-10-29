import { cn } from '@/lib/utils'

/** A search snippet (FR-KN-UI-2): text with the matches wrapped in «», shown highlighted. */
export function Snippet({ text, className }: { text: string; className?: string }) {
  const parts = text.split(/«([^»]*)»/)
  return (
    <span className={cn('break-words', className)}>
      {parts.map((part, i) =>
        i % 2 === 1 ? (
          <mark key={i} className="rounded bg-tint-amber px-0.5 text-tint-amber-fg">
            {part}
          </mark>
        ) : (
          part
        ),
      )}
    </span>
  )
}
