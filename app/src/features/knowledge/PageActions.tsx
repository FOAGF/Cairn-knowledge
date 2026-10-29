// Page actions shown in the page header, next to Edit (UI-KN-4): export as .md, find similar
// pages, archive / unarchive, delete. Icon buttons with tooltips keep them one click away.
import { Archive, ArchiveRestore, Download, ScanSearch, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { type Page, useArchivePage, useDeletePage } from '@/api/knowledge'
import { useIsExternal } from '@/api/storage'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { useT } from '@/i18n'
import { ExportDialog } from './ExportDialog'
import { ConfirmDialog } from './GroupDialogs'

function Action({ label, children }: { label: string; children: React.ReactElement }) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

export function PageActions({ page }: { page: Page }) {
  const navigate = useNavigate()
  const { mutate: archivePage } = useArchivePage()
  const { mutate: deletePage } = useDeletePage()
  const [exporting, setExporting] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const external = useIsExternal()
  const t = useT()
  const a = t.knowledge.actions
  const archiveLabel = page.archived ? a.unarchive : t.common.archive

  return (
    <div className="flex items-center gap-1">
      <Action label={a.exportMd}>
        <Button variant="outline" size="icon" aria-label={a.exportMd} onClick={() => setExporting(true)}>
          <Download />
        </Button>
      </Action>
      <Action label={a.findSimilar}>
        <Button variant="outline" size="icon" aria-label={a.findSimilar} asChild>
          <Link to={`/knowledge/new?check=${page.id}`}>
            <ScanSearch />
          </Link>
        </Button>
      </Action>
      <Action label={archiveLabel}>
        <Button
          variant="outline"
          size="icon"
          aria-label={archiveLabel}
          onClick={() =>
            archivePage(
              { id: page.id, archived: !page.archived },
              { onSuccess: () => toast.success(page.archived ? a.restored : a.archived) },
            )
          }
        >
          {page.archived ? <ArchiveRestore /> : <Archive />}
        </Button>
      </Action>
      <Action label={a.deletePage}>
        <Button
          variant="outline"
          size="icon"
          aria-label={a.deletePage}
          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
          onClick={() => setConfirmDelete(true)}
        >
          <Trash2 />
        </Button>
      </Action>

      <ExportDialog page={page} open={exporting} onClose={() => setExporting(false)} />
      <ConfirmDialog
        open={confirmDelete}
        title={a.deleteTitle}
        // FR-KN-EXT-10: in external mode the file goes to the system trash.
        description={external ? a.deleteExternal(page.title) : a.deleteInternal(page.title)}
        confirmLabel={a.deletePage}
        onClose={() => setConfirmDelete(false)}
        onConfirm={() => {
          setConfirmDelete(false)
          deletePage(page.id, {
            onSuccess: () => {
              toast.success(a.deleted)
              navigate('/knowledge')
            },
          })
        }}
      />
    </div>
  )
}
