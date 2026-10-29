// FR-KN-EXT-15: in external mode a page's title is its file name, which follows Windows' rules on
// every system. Under a title box it shows the name the page will get, when that differs.
import { useIsExternal } from '@/api/storage'
import { useT } from '@/i18n'
import { cleanTitle } from '@/lib/fileNames'
import { cn } from '@/lib/utils'

export function SavedAs({ title, className }: { title: string; className?: string }) {
  const external = useIsExternal()
  const t = useT()
  const typed = title.trim()
  if (!external || !typed) return null
  const clean = cleanTitle(typed)
  if (clean === typed) return null
  return (
    <p
      className={cn('text-xs text-muted-foreground', className)}
      data-testid="saved-as"
      title={t.shared.savedAs.rules}
    >
      {t.shared.savedAs.label(<span className="font-medium text-foreground">{clean}</span>)}
    </p>
  )
}
