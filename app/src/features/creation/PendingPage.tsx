// The pending page's tab in the top bar and the question before it is replaced (UI-KN-6a); the
// pending page itself is kept by pending.ts.
import { FilePenLine } from 'lucide-react'
import { useEffect } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { isApiError } from '@/api/client'
import { usePage } from '@/api/knowledge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { cn } from '@/lib/utils'
import { useAppStore } from '@/store'
import { getT, useT } from '@/i18n'
import { dropPageEdit, type PageEdit, pendingPath, releasePageEdit, useOnPendingPage } from './pending'

/** The question before the pending page is replaced. Mounted once in the app shell. */
export function PendingPageDialog() {
  const ask = useAppStore((s) => s.pendingAsk)
  const t = useT()
  // A saved page's changes are "them"; a new page is "it".
  const edit = !!ask?.edit
  const title = ask?.title || t.creation.pending.untitled
  const next = ask?.next === 'edit' ? 'edit' : 'new'
  return (
    <Dialog open={!!ask} onOpenChange={(o) => !o && ask?.resolve(null)}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t.creation.pending.dialogTitle}</DialogTitle>
          <DialogDescription>{t.creation.pending.description(title, edit, next)}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => ask?.resolve('replace')}>
            {t.creation.pending.discardAnd(edit, next)}
          </Button>
          <Button onClick={() => ask?.resolve('continue')}>{edit ? t.creation.pending.continueEditing : t.creation.pending.continueIt}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** The page with unsaved changes was deleted: its tab and changes go, with a notice. */
function useDropWhenDeleted(edit: PageEdit | null) {
  const { error } = usePage(edit?.pageId)
  const gone = !!edit && isApiError(error) && error.kind === 'not_found'
  useEffect(() => {
    if (!gone || !edit) return
    releasePageEdit(edit.pageId)
    dropPageEdit(edit)
    const p = getT().creation.pending
    toast(p.deleted(edit.title || p.untitled))
  }, [gone, edit])
}

/** The top bar's tab for the pending page. It has no ×: the page is saved or discarded on its own screen. */
export function PendingPageTab() {
  const draft = useAppStore((s) => s.draft)
  const pageEdit = useAppStore((s) => (s.draft ? null : s.pageEdit))
  const waiting = useAppStore((s) => s.draftQueue.length)
  const path = useAppStore(pendingPath)
  const active = useOnPendingPage()
  const t = useT()
  useDropWhenDeleted(pageEdit)
  if (!path) return null
  const title = draft ? draft.title.trim() || t.creation.pending.tabNew : pageEdit?.title.trim() || t.creation.pending.untitled
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Link
          to={path}
          className={cn(
            'relative flex h-10 min-w-0 items-center gap-2 rounded-xl px-3.5 text-sm font-medium transition-colors',
            active
              ? 'bg-accent text-accent-foreground after:absolute after:inset-x-3 after:-bottom-[13px] after:h-0.5 after:rounded-full after:bg-primary'
              : 'text-foreground/80 hover:bg-muted hover:text-foreground',
          )}
        >
          <FilePenLine className="size-[18px] shrink-0" />
          <span className="max-w-40 truncate">{title}</span>
          {draft && waiting > 0 && <span className="shrink-0 text-xs text-muted-foreground">+{waiting}</span>}
          <span className="size-2 shrink-0 rounded-full bg-tint-amber-fg" aria-label={t.creation.pending.notSaved} />
        </Link>
      </TooltipTrigger>
      <TooltipContent>
        {t.creation.pending.tooltip(!!draft, waiting)}
      </TooltipContent>
    </Tooltip>
  )
}
