// "Restore unsaved changes from <time>?" (FR-KN-ED-10). Shown while an editor has a pending draft.
import type { Draft } from '@/api/drafts'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { dateTime, timeAgo } from '@/lib/format'

/** The text of a task or comment draft (`{ text }`), or null if it cannot be read. */
export function draftText(draft: Draft): string | null {
  try {
    const v = JSON.parse(draft.content) as { text?: unknown }
    return typeof v.text === 'string' ? v.text : null
  } catch {
    return null
  }
}

export function DraftRestorePrompt({ draft, onRestore, onDiscard }: { draft: Draft; onRestore: () => void; onDiscard: () => void }) {
  const t = useT()
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-lg bg-tint-amber px-3 py-2 text-sm text-tint-amber-fg" role="status">
      <span title={dateTime(draft.updatedAt)}>{t.editor.restoreDraft(timeAgo(draft.updatedAt))}</span>
      <span className="ml-auto flex gap-2">
        <Button size="xs" variant="outline" onClick={onDiscard}>
          {t.common.discard}
        </Button>
        <Button size="xs" onClick={onRestore}>
          {t.common.restore}
        </Button>
      </span>
    </div>
  )
}
