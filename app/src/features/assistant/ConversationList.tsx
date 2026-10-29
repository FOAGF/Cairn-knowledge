import { Archive, ArchiveRestore, ChevronRight, FileUp, MessageSquare, MoreHorizontal, Pencil, Plus, Search, Trash2 } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { ConfirmDialog } from '@/features/knowledge/GroupDialogs'
import { useT } from '@/i18n'
import { timeAgo, timeUntil } from '@/lib/format'
import { cn } from '@/lib/utils'
import { archiveConversation, type Conversation, restoreConversation, useConversationList, useDeleteConversation, useUpdateConversation } from '@/api/conversations'

/** Saved conversations: search, rename, archive (FR-LLM-AS-5). Archived ones sit in a collapsed
 *  section with the time left; only they can be deleted (FR-LLM-AS-13). */
export function ConversationList({ activeId }: { activeId?: string }) {
  const navigate = useNavigate()
  const conversations = useConversationList()
  const update = useUpdateConversation()
  const del = useDeleteConversation()
  const updateConversation = (id: string, patch: { title: string }) => update.mutate({ id, patch })
  const [query, setQuery] = useState('')
  const [renaming, setRenaming] = useState<{ id: string; title: string } | null>(null)
  const [deleting, setDeleting] = useState<Conversation | null>(null)
  const [archiveOpen, setArchiveOpen] = useState(false)
  const t = useT()
  const tc = t.assistant.conversations

  const { active, archived } = useMemo(() => {
    const q = query.trim().toLowerCase()
    const found = conversations.filter((c) => !q || c.title.toLowerCase().includes(q))
    return {
      active: found.filter((c) => !c.archivedAt).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)),
      archived: found.filter((c) => c.archivedAt).sort((a, b) => (b.archivedAt ?? '').localeCompare(a.archivedAt ?? '')),
    }
  }, [conversations, query])
  // The open conversation is always in view, also when it is archived.
  const showArchived = archiveOpen || archived.some((c) => c.id === activeId)

  const archive = (c: Conversation) =>
    void archiveConversation(c.id).then(
      () =>
        toast.success(tc.archived, {
          description: tc.archivedDescription,
          action: { label: tc.undo, onClick: () => void restoreConversation(c.id).catch((e) => toast.error(errorMessage(e))) },
        }),
      (e) => toast.error(errorMessage(e)),
    )
  const restore = (c: Conversation) => void restoreConversation(c.id).catch((e) => toast.error(errorMessage(e)))

  const row = (c: Conversation) => (
    <li key={c.id}>
      <div
        className={cn(
          'group flex items-center gap-1 rounded-xl pr-1 transition-colors hover:bg-muted',
          c.id === activeId && 'bg-tint-blue text-tint-blue-fg hover:bg-tint-blue',
        )}
      >
        <button onClick={() => navigate(`/assistant/${c.id}`)} className="flex min-w-0 flex-1 items-start gap-2.5 px-3 py-2.5 text-left">
          {c.archivedAt ? <Archive className="mt-0.5 size-4 shrink-0 opacity-60" /> : <MessageSquare className="mt-0.5 size-4 shrink-0 opacity-70" />}
          <span className="min-w-0">
            <span className={cn('block truncate text-sm font-medium', c.archivedAt && 'opacity-80')}>{c.title}</span>
            <span className={cn('block text-xs', c.id === activeId ? 'opacity-70' : 'text-muted-foreground')}>
              {c.expiresAt ? tc.deletedIn(timeUntil(c.expiresAt)) : timeAgo(c.updatedAt)}
            </span>
          </span>
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" className="opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100" aria-label={tc.menu}>
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {c.archivedAt ? (
              <>
                <DropdownMenuItem onClick={() => restore(c)}>
                  <ArchiveRestore /> {t.common.restore}
                </DropdownMenuItem>
                <DropdownMenuItem variant="destructive" onClick={() => setDeleting(c)}>
                  <Trash2 /> {tc.deleteNow}
                </DropdownMenuItem>
              </>
            ) : (
              <>
                <DropdownMenuItem onClick={() => setRenaming({ id: c.id, title: c.title })}>
                  <Pencil /> {t.common.rename}
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => archive(c)}>
                  <Archive /> {t.common.archive}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  )

  return (
    <aside className="flex min-h-0 flex-col rounded-2xl border bg-card p-3 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      <Button className="w-full" onClick={() => navigate('/assistant')}>
        <Plus /> {tc.new}
      </Button>
      {/* FR-KN-UP-7: the same entry as Knowledge › Create from file. Each click gets its own value, so it
          also works while the Assistant is already open. */}
      <Button variant="outline" className="mt-2 w-full" onClick={() => navigate(`/assistant?attach=${Date.now()}`)}>
        <FileUp /> {tc.fromFile}
      </Button>
      <div className="relative my-3">
        <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={tc.search} className="pl-9" />
      </div>
      <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1">
        <ul className="space-y-1">
          {active.map(row)}
          {active.length === 0 && (
            <li className="px-3 py-6 text-center text-sm text-muted-foreground">{query.trim() ? tc.noneFound : tc.noneActive}</li>
          )}
        </ul>
        {archived.length > 0 && (
          <div className="mt-3 border-t pt-2">
            <button
              type="button"
              onClick={() => setArchiveOpen(!showArchived)}
              aria-expanded={showArchived}
              className="flex w-full items-center gap-1.5 rounded-lg px-2 py-1.5 text-xs font-medium text-muted-foreground hover:bg-muted"
            >
              <ChevronRight className={cn('size-3.5 transition-transform', showArchived && 'rotate-90')} />
              {tc.archivedSection(archived.length)}
            </button>
            {showArchived && <ul className="mt-1 space-y-1">{archived.map(row)}</ul>}
          </div>
        )}
      </div>

      <ConfirmDialog
        open={!!deleting}
        title={tc.deleteTitle}
        description={tc.deleteDescription(deleting?.title ?? '')}
        confirmLabel={tc.deleteConfirm}
        onClose={() => setDeleting(null)}
        onConfirm={() => {
          const c = deleting
          setDeleting(null)
          if (c) del.mutate(c.id, { onSuccess: () => c.id === activeId && navigate('/assistant') })
        }}
      />

      <Dialog open={!!renaming} onOpenChange={(o) => !o && setRenaming(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{tc.renameTitle}</DialogTitle>
          </DialogHeader>
          <Input
            autoFocus
            value={renaming?.title ?? ''}
            onChange={(e) => setRenaming((r) => r && { ...r, title: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && renaming?.title.trim()) {
                updateConversation(renaming.id, { title: renaming.title.trim() })
                setRenaming(null)
              }
            }}
          />
          <DialogFooter>
            <Button variant="outline" onClick={() => setRenaming(null)}>
              {t.common.cancel}
            </Button>
            <Button
              disabled={!renaming?.title.trim()}
              onClick={() => {
                if (renaming) updateConversation(renaming.id, { title: renaming.title.trim() })
                setRenaming(null)
              }}
            >
              {t.common.save}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </aside>
  )
}
