import { cn } from '@/lib/utils'

/** Three stacked, slightly offset stones: a cairn. */
export function CairnLogo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={cn('size-8', className)} aria-hidden>
      <rect x="3" y="21" width="26" height="8" rx="4" className="fill-primary" />
      <rect x="7.5" y="13" width="18" height="6.5" rx="3.25" className="fill-primary/70" />
      <rect x="10" y="5.5" width="11" height="6" rx="3" className="fill-primary/45" />
    </svg>
  )
}
