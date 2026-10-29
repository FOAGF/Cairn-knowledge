import { ChevronDown, ChevronUp } from 'lucide-react'
import { useState } from 'react'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'

/** Shows only the first `limit` items until expanded. */
export function useFold<T>(items: T[], limit: number) {
  const [expanded, setExpanded] = useState(false)
  return {
    visible: expanded ? items : items.slice(0, limit),
    expanded,
    toggle: () => setExpanded((e) => !e),
    hiddenCount: Math.max(0, items.length - limit),
  }
}

/** "Show 4 more tasks" / "Show less" toggle. Renders nothing when nothing is folded. */
export function FoldButton({
  fold,
  noun,
  className,
}: {
  fold: { expanded: boolean; toggle: () => void; hiddenCount: number }
  /** Plural noun in the interface language, e.g. "tasks". */
  noun: string
  className?: string
}) {
  const t = useT()
  if (fold.hiddenCount === 0) return null
  return (
    <button
      type="button"
      onClick={fold.toggle}
      className={cn('flex w-full items-center justify-center gap-1.5 rounded-lg py-2 text-sm font-medium text-primary hover:bg-accent', className)}
    >
      {fold.expanded ? <ChevronUp className="size-4" /> : <ChevronDown className="size-4" />}
      {fold.expanded ? t.common.showLess : t.shared.fold.showMore(fold.hiddenCount, noun)}
    </button>
  )
}
