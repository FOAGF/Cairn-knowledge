// Left panel: search + filters + Category → Group → Page tree (FR-KN-UI-1, FR-KN-UI-2). *Select*
// adds checkboxes for changing many pages at once (FR-KN-UI-3): a category, group or "Ungrouped"
// ticks the pages shown under it. In external mode groups hold groups (FR-KN-EXT-2): the tree
// nests them, and a group's tick covers the groups inside it.
import { Check, CheckSquare, ChevronDown, ChevronRight, FileText, FolderInput, FolderOpen, FolderPlus, MoreHorizontal, Pencil, Search, Sparkles, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Link, useLocation } from 'react-router'
import { CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Snippet } from '@/components/shared/Snippet'
import { useGroups, usePageTags, type Group, type PageSummary } from '@/api/knowledge'
import { useIsExternal } from '@/api/storage'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import type { GroupDialogState } from './GroupDialogs'
import { childGroups, groupAndInside } from './groupTree'
import type { PageFilters } from './utils'
import { useCategoryList } from '@/api/categories'

/** Pages ticked in the tree, while *Select* is on. */
export type PageSelection = {
  selected: Set<string>
  set: (ids: string[], on: boolean) => void
}

/** A checkbox for several pages: ticked when all are, a dash when some are. */
function TickMany({ ids, selection, label }: { ids: string[]; selection: PageSelection; label: string }) {
  const n = ids.filter((id) => selection.selected.has(id)).length
  const state = n === 0 ? false : n === ids.length ? true : 'indeterminate'
  return <Checkbox checked={state} disabled={ids.length === 0} aria-label={label} onCheckedChange={() => selection.set(ids, state !== true)} className="ml-1.5" />
}

function PageRow({ page, active, search, snippet, selection }: { page: PageSummary; active: boolean; search: string; snippet?: string; selection?: PageSelection }) {
  const ticked = !!selection?.selected.has(page.id)
  const t = useT()
  const className = cn(
    'flex w-full items-center gap-2 rounded-lg py-1.5 pr-2 pl-2 text-left text-sm transition-colors hover:bg-muted',
    active && !selection && 'bg-accent font-medium text-accent-foreground hover:bg-accent',
    ticked && 'bg-tint-blue/50 hover:bg-tint-blue/70',
    page.archived && 'opacity-60',
  )
  const content = (
    <>
      {selection ? (
        // Only a picture of a checkbox: the whole row is the control (no button inside a button).
        <span aria-hidden className={cn('flex size-4 shrink-0 items-center justify-center rounded-[4px] border border-input', ticked && 'border-primary bg-primary text-primary-foreground')}>
          {ticked && <Check className="size-3.5" />}
        </span>
      ) : (
        <FileText className="mt-0.5 size-3.5 shrink-0 self-start text-muted-foreground" />
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate">{page.title}</span>
        {snippet && <Snippet text={snippet} className="line-clamp-2 block text-xs font-normal text-muted-foreground" />}
      </span>
      {page.checkStatus === 'not_checked' && <span title={t.knowledge.notChecked} className="size-2 shrink-0 rounded-full bg-tint-amber-fg" />}
    </>
  )
  // While selecting, the whole row ticks the page.
  if (selection)
    return (
      <button type="button" role="checkbox" aria-checked={ticked} data-page-row={page.id} className={className} onClick={() => selection.set([page.id], !ticked)}>
        {content}
      </button>
    )
  return (
    <Link to={`/knowledge/${page.id}${search}`} className={className}>
      {content}
    </Link>
  )
}

export function KnowledgeTree({
  filters,
  setFilters,
  pages,
  snippets,
  activeId,
  setDialog,
  selecting,
  setSelecting,
  selection,
}: {
  filters: PageFilters
  setFilters: (f: Partial<PageFilters>) => void
  /** Pages that already match the filters. */
  pages: PageSummary[]
  /** Search snippets by page id, while a text search is active. */
  snippets: Map<string, string>
  activeId?: string
  setDialog: (d: GroupDialogState) => void
  selecting: boolean
  setSelecting: (on: boolean) => void
  selection: PageSelection
}) {
  const sel = selecting ? selection : undefined
  const categories = useCategoryList()
  const groups = useGroups()
  const allTags = usePageTags()
  const external = useIsExternal()
  const { search } = useLocation()
  const k = useT().knowledge
  const tr = k.tree
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const toggle = (id: string) =>
    setCollapsed((c) => {
      const n = new Set(c)
      if (!n.delete(id)) n.add(id)
      return n
    })

  const filtering = !!(filters.query || filters.tag || filters.notChecked)
  const shownCategories = categories.filter((p) => p.status === 'active' && (!filters.categoryId || p.id === filters.categoryId))

  return (
    <aside className="flex flex-col gap-3 rounded-2xl border bg-card p-3 shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={filters.query}
          onChange={(e) => setFilters({ query: e.target.value })}
          placeholder={tr.searchPlaceholder}
          className="h-9 rounded-xl pl-9"
        />
      </div>
      <div className="grid grid-cols-2 gap-2">
        <Select value={filters.categoryId || 'all'} onValueChange={(v) => setFilters({ categoryId: v === 'all' ? '' : v })}>
          <SelectTrigger className="w-full" aria-label={tr.filterByCategory}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{tr.allCategories}</SelectItem>
            {categories.map((p) => (
              <SelectItem key={p.id} value={p.id}>
                {p.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={filters.tag || 'all'} onValueChange={(v) => setFilters({ tag: v === 'all' ? '' : v })}>
          <SelectTrigger className="w-full" aria-label={tr.filterByTag}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{tr.allTags}</SelectItem>
            {allTags.map((t) => (
              <SelectItem key={t} value={t}>
                {t}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="flex items-start justify-between gap-2 px-1 text-sm">
        <div className="flex flex-col gap-1.5 whitespace-nowrap">
          <label className="flex items-center gap-2 text-muted-foreground">
            <Checkbox checked={filters.notChecked} onCheckedChange={(c) => setFilters({ notChecked: c === true })} />
            {tr.notCheckedOnly}
          </label>
          <label className="flex items-center gap-2 text-muted-foreground">
            <Checkbox checked={filters.showArchived} onCheckedChange={(c) => setFilters({ showArchived: c === true })} />
            {tr.showArchived}
          </label>
        </div>
        <Button
          variant={selecting ? 'secondary' : 'default'}
          size="xs"
          aria-pressed={selecting}
          title={tr.selectHint}
          onClick={() => setSelecting(!selecting)}
        >
          <CheckSquare /> {selecting ? tr.stopSelecting : tr.select}
        </Button>
      </div>

      <div className="-mx-1 max-h-[calc(100vh-24rem)] min-h-40 space-y-1 overflow-y-auto border-t px-1 pt-3">
        {shownCategories.map((category) => {
          const projGroups = groups.filter((g) => g.categoryId === category.id)
          const projPages = pages.filter((p) => p.categoryId === category.id)
          if (filtering && projPages.length === 0) return null
          const open = !collapsed.has(category.id)
          const ungrouped = projPages.filter((p) => !p.groupId || !projGroups.some((g) => g.id === p.groupId))
          return (
            <div key={category.id}>
              <div className="group/p flex items-center gap-1">
                {sel && <TickMany ids={projPages.map((p) => p.id)} selection={sel} label={tr.selectPagesOf(category.name)} />}
                <button onClick={() => toggle(category.id)} className="flex min-w-0 flex-1 items-center gap-1.5 rounded-lg px-1.5 py-1.5 text-left text-sm font-semibold hover:bg-muted">
                  {open ? <ChevronDown className="size-4 text-muted-foreground" /> : <ChevronRight className="size-4 text-muted-foreground" />}
                  <CategoryDot color={category.color} />
                  <span className="truncate">{category.name}</span>
                  <span className="ml-auto text-xs font-normal text-muted-foreground">{projPages.length}</span>
                </button>
                <Button variant="ghost" size="icon-sm" title={k.newGroup} aria-label={tr.newGroupIn(category.name)} onClick={() => setDialog({ kind: 'new', categoryId: category.id })}>
                  <FolderPlus />
                </Button>
                {ungrouped.length > 0 && (
                  <Button variant="ghost" size="icon-sm" title={tr.suggestGroupsHint} aria-label={tr.suggestGroupsIn(category.name)} onClick={() => setDialog({ kind: 'suggest', categoryId: category.id })}>
                    <Sparkles />
                  </Button>
                )}
              </div>
              {open && (
                <div className="ml-3 border-l pl-2">
                  {childGroups(projGroups, null).map((g) => (
                    <GroupNode key={g.id} group={g} groups={projGroups} snippets={snippets} pages={projPages} hideEmpty={filtering} collapsed={collapsed} toggle={toggle} activeId={activeId} search={search} setDialog={setDialog} selection={sel} external={external} />
                  ))}
                  {ungrouped.length > 0 && (
                    <div className="mt-1">
                      <div className="flex items-center">
                        {sel && <TickMany ids={ungrouped.map((p) => p.id)} selection={sel} label={tr.selectUngroupedOf(category.name)} />}
                        <p className="px-2 py-1 text-xs font-medium text-muted-foreground">{k.ungrouped}</p>
                      </div>
                      {ungrouped.map((p) => (
                        <PageRow key={p.id} page={p} active={p.id === activeId} search={search} snippet={snippets.get(p.id)} selection={sel} />
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          )
        })}
        {pages.length === 0 && <p className="px-2 py-6 text-center text-sm text-muted-foreground">{tr.noPagesMatch}</p>}
      </div>
    </aside>
  )
}

function GroupNode({
  group,
  groups,
  pages,
  snippets,
  hideEmpty,
  collapsed,
  toggle,
  activeId,
  search,
  setDialog,
  selection,
  external,
}: {
  group: Group
  /** The groups of its category. */
  groups: Group[]
  /** The pages of its category. */
  pages: PageSummary[]
  snippets: Map<string, string>
  hideEmpty: boolean
  collapsed: Set<string>
  toggle: (id: string) => void
  activeId?: string
  search: string
  setDialog: (d: GroupDialogState) => void
  selection?: PageSelection
  external: boolean
}) {
  const k = useT().knowledge
  const tr = k.tree
  const inner = childGroups(groups, group.id)
  const own = pages.filter((p) => p.groupId === group.id)
  // The pages in it, at any depth: what its tick and its count cover.
  const inside = inner.length ? pages.filter((p) => !!p.groupId && groupAndInside(group, groups).has(p.groupId)) : own
  if (hideEmpty && inside.length === 0) return null
  const closed = collapsed.has(group.id)
  return (
    <div>
      <div className="group/g flex items-center">
        {selection && <TickMany ids={inside.map((p) => p.id)} selection={selection} label={tr.selectPagesOf(group.name)} />}
        <button onClick={() => toggle(group.id)} title={group.description || undefined} className="flex min-w-0 flex-1 items-center gap-1.5 rounded-lg px-1.5 py-1.5 text-left text-sm font-medium hover:bg-muted">
          {closed ? <ChevronRight className="size-3.5 text-muted-foreground" /> : <ChevronDown className="size-3.5 text-muted-foreground" />}
          <FolderOpen className="size-4 shrink-0 text-tint-amber-fg" />
          <span className="truncate">{group.name}</span>
          <span className="ml-auto text-xs font-normal text-muted-foreground">{inside.length}</span>
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-xs" className="opacity-0 group-hover/g:opacity-100 data-[state=open]:opacity-100" aria-label={tr.actionsFor(group.name)}>
              <MoreHorizontal />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={() => setDialog({ kind: 'rename', group })}>
              <Pencil /> {tr.editNameDescription}
            </DropdownMenuItem>
            {external && (
              <>
                <DropdownMenuItem onSelect={() => setDialog({ kind: 'new', categoryId: group.categoryId, parentId: group.id })}>
                  <FolderPlus /> {tr.newGroupInside}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setDialog({ kind: 'move', group })}>
                  <FolderInput /> {tr.moveTo}
                </DropdownMenuItem>
              </>
            )}
            <DropdownMenuItem variant="destructive" onSelect={() => setDialog({ kind: 'delete', group })}>
              <Trash2 /> {k.deleteGroup}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {!closed && (
        <div className="ml-3 border-l pl-1.5">
          {inner.map((g) => (
            <GroupNode key={g.id} group={g} groups={groups} pages={pages} snippets={snippets} hideEmpty={hideEmpty} collapsed={collapsed} toggle={toggle} activeId={activeId} search={search} setDialog={setDialog} selection={selection} external={external} />
          ))}
          {own.map((p) => (
            <PageRow key={p.id} page={p} active={p.id === activeId} search={search} snippet={snippets.get(p.id)} selection={selection} />
          ))}
          {own.length === 0 && inner.length === 0 && <p className="px-2 py-1 text-xs text-muted-foreground">{tr.emptyGroup}</p>}
        </div>
      )}
    </div>
  )
}
