// Knowledge screens: tree + overview + page view (docs/requirements.md §5).
// Serves both /knowledge and /knowledge/:pageId.
import { BookOpen, ChevronDown, FileDown, FilePlus2, FileUp, FolderInput, FolderOutput, ScanSearch } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { PageHeader } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useCreateGroup, useDeleteGroup, useGroups, usePageList, useUpdateGroup, type PageFilter } from '@/api/knowledge'
import { useCategoryList } from '@/api/categories'
import { useSearch } from '@/api/search'
import { useIsExternal } from '@/api/storage'
import { useT } from '@/i18n'
import { ConfirmDialog, GroupMoveDialog, GroupNameDialog, type GroupDialogState } from './GroupDialogs'
import { KnowledgeOverview } from './KnowledgeOverview'
import { KnowledgeTree, type PageSelection } from './KnowledgeTree'
import { PageBulkBar } from './PageBulkBar'
import { ExportFolderDialog, ImportFolderDialog } from './MarkdownFolderDialogs'
import { SuggestGroupsDialog } from './SuggestGroupsDialog'
import { PageView } from './PageView'
import { toPageFilter, type PageFilters } from './utils'
import { cn } from '@/lib/utils'

const NOT_CHECKED: PageFilter = { checkStatus: 'not_checked' }

export default function KnowledgePage() {
  const { pageId } = useParams()
  const [searchParams, setSearchParams] = useSearchParams()
  const { mutate: createGroup } = useCreateGroup()
  const { mutate: updateGroup } = useUpdateGroup()
  const { mutate: deleteGroup } = useDeleteGroup()
  const [local, setLocal] = useState({ query: '', tag: '', notChecked: false, showArchived: false })
  const [dialog, setDialog] = useState<GroupDialogState>(null)
  const [folderDialog, setFolderDialog] = useState<'import' | 'export' | null>(null)
  const external = useIsExternal()
  const groups = useGroups()
  const categories = useCategoryList()
  // The group a new group goes into (external mode), and the group being moved with its category's groups.
  const newParent = dialog?.kind === 'new' && dialog.parentId ? groups.find((g) => g.id === dialog.parentId) : undefined
  const moving = dialog?.kind === 'move' ? dialog.group : null
  const t = useT()
  const k = t.knowledge

  // The category filter lives in the URL (`?category=<id>`), because other screens link to it.
  const filters: PageFilters = { ...local, categoryId: searchParams.get('category') ?? '' }
  const setFilters = (f: Partial<PageFilters>) => {
    const { categoryId, ...rest } = f
    if (Object.keys(rest).length) setLocal((l) => ({ ...l, ...rest }))
    if (categoryId !== undefined) {
      const next = new URLSearchParams(searchParams)
      if (categoryId) next.set('category', categoryId)
      else next.delete('category')
      setSearchParams(next, { replace: true })
    }
  }

  // Filters (category, tag, not checked, archived) are applied in Rust (FR-KN-UI-1)...
  const { categoryId, tag, notChecked, showArchived } = filters
  const listFilter = useMemo<PageFilter>(() => toPageFilter({ query: '', categoryId, tag, notChecked, showArchived }), [categoryId, tag, notChecked, showArchived])
  const listed = usePageList(listFilter)
  // ...and the text search (FR-KN-UI-2) is a keyword search whose hits are intersected with them.
  const searchQuery = useMemo(
    () => ({ text: filters.query, kinds: ['page' as const], categoryIds: categoryId ? [categoryId] : null, includeArchived: showArchived, limit: 1000 }),
    [filters.query, categoryId, showArchived],
  )
  const { hits } = useSearch(searchQuery)
  const searching = filters.query.trim().length > 0
  const snippets = useMemo(() => new Map(hits.map((h) => [h.id, h.snippet])), [hits])
  const visible = useMemo(() => (searching ? listed.filter((p) => snippets.has(p.id)) : listed), [searching, listed, snippets])
  const uncheckedCount = usePageList(NOT_CHECKED).length

  // FR-KN-UI-3: pages ticked while *Select* is on. A page that leaves the tree (a filter,
  // archiving, deleting) leaves the selection too, so no hidden page is ever changed.
  const [selecting, setSelectingState] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(() => new Set())
  const [listedPages, setListedPages] = useState(visible)
  if (listedPages !== visible) {
    setListedPages(visible)
    const shown = new Set(visible.map((p) => p.id))
    if ([...selected].some((id) => !shown.has(id))) setSelected(new Set([...selected].filter((id) => shown.has(id))))
  }
  const ticked = useMemo(() => (selecting ? visible.filter((p) => selected.has(p.id)) : []), [selecting, visible, selected])
  const selection: PageSelection = {
    selected,
    set: (ids, on) =>
      setSelected((s) => {
        const next = new Set(s)
        for (const id of ids) {
          if (on) next.add(id)
          else next.delete(id)
        }
        return next
      }),
  }
  const setSelecting = (on: boolean) => {
    setSelectingState(on)
    if (!on) setSelected(new Set())
  }

  return (
    // Room under the page for the selection bar.
    <div className={cn(ticked.length > 0 && 'pb-20')}>
      <PageHeader
        icon={BookOpen}
        tint="purple"
        title={t.common.nav.knowledge}
        description={k.screen.description}
        actions={
          <>
            {uncheckedCount > 0 && (
              <Button variant="outline" asChild>
                <Link to="/knowledge/new?check=unchecked">
                  <ScanSearch /> {k.screen.checkAllUnchecked(uncheckedCount)}
                </Link>
              </Button>
            )}
            {/* FR-KN-MD-1..4: Markdown folders in and out; not in external mode, where pages are files already (FR-KN-EXT-13). */}
            {!external && (
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button variant="outline">
                    <FileDown /> Markdown <ChevronDown className="opacity-60" />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem onClick={() => setFolderDialog('import')}>
                    <FolderInput /> {k.screen.importFromFolder}
                  </DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setFolderDialog('export')}>
                    <FolderOutput /> {k.screen.exportToFolder}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
            <Button variant="outline" asChild>
              <Link to="/assistant?attach=1">
                <FileUp /> {k.screen.createFromFile}
              </Link>
            </Button>
            <Button asChild>
              <Link to="/knowledge/new">
                <FilePlus2 /> {t.common.newPage}
              </Link>
            </Button>
          </>
        }
      />

      <div className="grid items-start gap-6 lg:grid-cols-[300px_1fr]">
        <KnowledgeTree
          filters={filters}
          setFilters={setFilters}
          pages={visible}
          snippets={snippets}
          activeId={pageId}
          setDialog={setDialog}
          selecting={selecting}
          setSelecting={setSelecting}
          selection={selection}
        />
        <div className="min-w-0">{pageId ? <PageView key={pageId} pageId={pageId} /> : <KnowledgeOverview pages={visible} />}</div>
      </div>

      {ticked.length > 0 && <PageBulkBar pages={ticked} onClear={() => setSelected(new Set())} />}
      <ImportFolderDialog open={folderDialog === 'import'} onClose={() => setFolderDialog(null)} defaultCategoryId={categoryId || undefined} />
      <ExportFolderDialog open={folderDialog === 'export'} onClose={() => setFolderDialog(null)} />
      {dialog?.kind === 'suggest' && <SuggestGroupsDialog key={dialog.categoryId} categoryId={dialog.categoryId} onClose={() => setDialog(null)} />}
      <GroupNameDialog
        open={dialog?.kind === 'new'}
        title={newParent ? k.screen.newGroupIn(newParent.name) : k.newGroup}
        onClose={() => setDialog(null)}
        onSubmit={(name, description) => {
          if (dialog?.kind === 'new')
            createGroup({ categoryId: dialog.categoryId, name, description: description || null, ...(dialog.parentId ? { parentId: dialog.parentId } : {}) })
          setDialog(null)
        }}
      />
      <GroupMoveDialog
        group={moving}
        groups={moving ? groups.filter((g) => g.categoryId === moving.categoryId) : []}
        categoryName={categories.find((c) => c.id === moving?.categoryId)?.name ?? k.screen.theCategory}
        onClose={() => setDialog(null)}
        onMove={(parentId) => {
          if (moving) updateGroup({ id: moving.id, patch: { parentId } })
          setDialog(null)
        }}
      />
      <GroupNameDialog
        open={dialog?.kind === 'rename'}
        title={k.screen.editGroup}
        initial={dialog?.kind === 'rename' ? dialog.group.name : ''}
        initialDescription={dialog?.kind === 'rename' ? dialog.group.description : ''}
        onClose={() => setDialog(null)}
        onSubmit={(name, description) => {
          if (dialog?.kind === 'rename') updateGroup({ id: dialog.group.id, patch: { name, description } })
          setDialog(null)
        }}
      />
      <ConfirmDialog
        open={dialog?.kind === 'delete'}
        title={k.screen.deleteGroupTitle(dialog?.kind === 'delete' ? dialog.group.name : '')}
        description={external ? k.screen.deleteGroupExternal : k.screen.deleteGroupInternal}
        confirmLabel={k.deleteGroup}
        onClose={() => setDialog(null)}
        onConfirm={() => {
          if (dialog?.kind === 'delete') deleteGroup(dialog.group.id)
          setDialog(null)
        }}
      />
    </div>
  )
}
