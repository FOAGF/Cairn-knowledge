import { Info } from 'lucide-react'
import type { MeaningStatus } from '@/api/search'
import { getT, useLang } from '@/i18n'
import { cn } from '@/lib/utils'

/**
 * FR-RAG-IX-6: says when search by meaning covers only part of the data, or can't be used at all.
 * Nothing for `not_installed` (a lite package without the search model, D-82): not a fault.
 */
export function meaningNotice(meaning: MeaningStatus | undefined): { text: string; title?: string } | null {
  if (!meaning) return null
  const t = getT().shared.meaning
  if (meaning.state === 'partial')
    return { text: t.partial(meaning.itemsReady, meaning.items) }
  if (meaning.state === 'unavailable')
    return { text: t.unavailable, title: meaning.detail ?? undefined }
  return null
}

/** The warning as a small line; renders nothing when everything is fine. */
export function MeaningNotice({ meaning, className }: { meaning: MeaningStatus | undefined; className?: string }) {
  useLang()
  const notice = meaningNotice(meaning)
  if (!notice) return null
  return (
    <p title={notice.title} className={cn('flex items-center gap-1.5 text-xs text-muted-foreground', className)}>
      <Info className="size-3.5 shrink-0" />
      {notice.text}
    </p>
  )
}
