// The bar for pages selected in the tree (FR-KN-UI-3): archive, restore, move, tags, mark as
// checked, delete. Every action goes through the listed confirmation with its countdown
// (components/shared/Bulk.tsx). In external mode a move that would give two files one name in a
// folder is refused as a whole (FR-KN-EXT-10): Rust plans it first, and the clashes lock the button.
import { useQuery } from '@tanstack/react-query'
import { Archive, ArchiveRestore, CheckCheck, FolderInput, Tag, Tags, Trash2, TriangleAlert } from 'lucide-react'
import { useState } from 'react'
import { commands, type NameClash, type PageBulkChange } from '@/api/bindings'
import { useCategoryList } from '@/api/categories'
import { errorMessage } from '@/api/client'
import { useGroups, usePageTags, type PageSummary } from '@/api/knowledge'
import { useIsExternal } from '@/api/storage'
import { BulkBar, BulkConfirmDialog, NameChips, NamePicker, type BulkItem } from '@/components/shared/Bulk'
import { CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { groupOptions, groupPath } from './groupTree'

/** The file names a move would use twice, listed in the confirmation (FR-KN-EXT-10). */
function ClashList({ clashes }: { clashes: NameClash[] }) {
  const b = useT().knowledge.bulk
  return (
    <div className="space-y-2 rounded-xl border border-destructive/40 bg-destructive/5 p-3 text-sm" role="alert" data-testid="bulk-clashes">
      <p className="flex items-center gap-2 font-medium text-destructive">
        <TriangleAlert className="size-4 shrink-0" /> {b.clashTitle}
      </p>
      <p className="text-xs text-muted-foreground">{b.clashHint}</p>
      <ul className="max-h-40 space-y-1.5 overflow-y-auto">
        {clashes.map((c) => (
          <li key={`${c.folder}/${c.name}`} data-testid="bulk-clash">
            <p className="font-mono text-xs break-all">
              {c.folder ? `${c.folder}/` : ''}
              {c.name}
            </p>
            <p className="text-xs text-muted-foreground">
              {c.titles.length > 1 ? c.titles.join(', ') : b.clashWithFile(c.titles[0] ?? '')}
            </p>
          </li>
        ))}
      </ul>
    </div>
  )
}

type Action = 'archive' | 'restore' | 'move' | 'add_tag' | 'remove_tag' | 'mark_checked' | 'delete'
const NO_GROUP = '__none'

export function PageBulkBar({ pages, onClear }: { pages: PageSummary[]; onClear: () => void }) {
  const categories = useCategoryList()
  const groups = useGroups()
  const allTags = usePageTags()
  const external = useIsExternal()
  const [action, setAction] = useState<Action | null>(null)
  const [tag, setTag] = useState('')
  const [target, setTarget] = useState({ categoryId: '', groupId: NO_GROUP })
  const t = useT()
  const b = t.knowledge.bulk
  const open = (a: Action) => {
    setTag('')
    // A move starts from the category all the pages share, if they share one.
    const shared = pages.every((p) => p.categoryId === pages[0]?.categoryId) ? pages[0]?.categoryId : ''
    setTarget({ categoryId: shared ?? '', groupId: NO_GROUP })
    setAction(a)
  }
  const ids = pages.map((p) => p.id)
  const n = pages.length
  const where = (p: PageSummary) => {
    const c = categories.find((x) => x.id === p.categoryId)?.name ?? ''
    const g = groups.find((x) => x.id === p.groupId)
    return g ? `${c} › ${groupPath(g, groups)}` : c
  }
  const items = (detail: (p: PageSummary) => React.ReactNode = where): BulkItem[] => pages.map((p) => ({ id: p.id, label: p.title, detail: detail(p) }))
  const update = (change: PageBulkChange) => () => commands.updatePages(ids, change)
  const tagsOnPages = [...new Set(pages.flatMap((p) => p.tags))].sort((a, b) => a.localeCompare(b))
  const anyArchived = pages.some((p) => p.archived)
  const anyActive = pages.some((p) => !p.archived)
  const anyUnchecked = pages.some((p) => p.checkStatus === 'not_checked')
  const targetCategory = categories.find((c) => c.id === target.categoryId)
  const targetGroups = groups.filter((g) => g.categoryId === target.categoryId)
  const targetGroup = targetGroups.find((g) => g.id === target.groupId)
  const targetName = targetCategory ? (targetGroup ? `${targetCategory.name} › ${groupPath(targetGroup, targetGroups)}` : targetCategory.name) : ''
  const moveChange: PageBulkChange = { kind: 'move', categoryId: target.categoryId, groupId: target.groupId === NO_GROUP ? null : target.groupId }
  // External mode: the clashes this move would make, asked again when the target or the pages change.
  const plan = useQuery({
    queryKey: ['knowledge', 'bulkPlan', ids, moveChange],
    queryFn: () => commands.planPageBulkChange(ids, moveChange),
    enabled: external && action === 'move' && !!targetCategory,
    staleTime: 0,
    gcTime: 0,
  })
  let blocked: React.ReactNode
  if (external && action === 'move' && targetCategory) {
    if (plan.isError) blocked = <p role="alert" className="text-sm text-destructive">{b.planFailed(errorMessage(plan.error))}</p>
    else if (plan.data?.length) blocked = <ClashList clashes={plan.data} />
  }

  const dialog = (() => {
    switch (action) {
      case 'archive':
        return {
          title: b.archiveTitle(n),
          description: b.archiveDescription,
          items: items(),
          confirmLabel: t.common.archive,
          run: update({ kind: 'archive' }),
          doneMessage: b.archived,
        }
      case 'restore':
        return {
          title: b.restoreTitle(n),
          description: b.restoreDescription,
          items: items(),
          confirmLabel: t.common.restore,
          run: update({ kind: 'restore' }),
          doneMessage: b.restored,
        }
      case 'move':
        return {
          title: b.moveTitle(n),
          description: external ? b.moveExternal : b.moveInternal,
          items: items(),
          confirmLabel: t.common.move,
          // External mode: only once Rust has checked the file names.
          ready: !!targetCategory && (!external || plan.isSuccess),
          blocked,
          children: (
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label>{t.common.category}</Label>
                <Select value={target.categoryId} onValueChange={(v) => setTarget({ categoryId: v, groupId: NO_GROUP })}>
                  <SelectTrigger className="w-full" aria-label={b.targetCategory}>
                    <SelectValue placeholder={b.chooseCategory} />
                  </SelectTrigger>
                  <SelectContent>
                    {categories
                      .filter((c) => c.status === 'active')
                      .map((c) => (
                        <SelectItem key={c.id} value={c.id}>
                          <CategoryDot color={c.color} className="size-2" /> {c.name}
                        </SelectItem>
                      ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1.5">
                <Label>{t.common.group}</Label>
                <Select value={target.groupId} onValueChange={(v) => setTarget((x) => ({ ...x, groupId: v }))} disabled={!targetCategory}>
                  <SelectTrigger className="w-full" aria-label={b.targetGroup}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NO_GROUP}>{b.noGroup}</SelectItem>
                    {groupOptions(targetGroups).map(({ group: g, label }) => (
                      <SelectItem key={g.id} value={g.id}>
                        {label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
          ),
          run: update(moveChange),
          doneMessage: (d: number) => b.moved(d, targetName),
        }
      case 'add_tag':
        return {
          title: b.addTagTitle(n),
          description: b.addTagDescription,
          items: items((p) => <NameChips names={p.tags} />),
          confirmLabel: b.addTag,
          ready: tag.trim() !== '',
          children: <NamePicker typed noun="tag" names={allTags} value={tag} onChange={setTag} />,
          run: update({ kind: 'add_tag', tag: tag.trim() }),
          doneMessage: (d: number) => b.tagAdded(tag.trim(), d),
        }
      case 'remove_tag':
        return {
          title: b.removeTagTitle(n),
          description: b.removeTagDescription,
          items: items((p) => <NameChips names={p.tags} />),
          confirmLabel: b.removeTag,
          ready: tag !== '',
          children: <NamePicker noun="tag" names={tagsOnPages} value={tag} onChange={setTag} />,
          run: update({ kind: 'remove_tag', tag }),
          doneMessage: (d: number) => b.tagRemoved(tag, d),
        }
      case 'mark_checked':
        return {
          title: b.markCheckedTitle(n),
          description: b.markCheckedDescription,
          items: items((p) => (p.checkStatus === 'not_checked' ? b.notChecked : b.checked)),
          confirmLabel: b.markChecked,
          run: update({ kind: 'mark_checked' }),
          doneMessage: b.markedChecked,
        }
      case 'delete':
        return {
          title: b.deleteTitle(n),
          description: external ? b.deleteExternal : b.deleteInternal,
          items: items(),
          confirmLabel: b.deleteConfirm(n),
          destructive: true,
          run: () => commands.deletePages(ids),
          doneMessage: b.deleted,
          onDone: onClear,
        }
      default:
        return null
    }
  })()

  return (
    <>
      <BulkBar selected={n} onClear={onClear}>
        {anyActive && (
          <Button variant="ghost" size="sm" onClick={() => open('archive')}>
            <Archive /> {t.common.archive}
          </Button>
        )}
        {anyArchived && (
          <Button variant="ghost" size="sm" onClick={() => open('restore')}>
            <ArchiveRestore /> {t.common.restore}
          </Button>
        )}
        <Button variant="ghost" size="sm" onClick={() => open('move')}>
          <FolderInput /> {b.move}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => open('add_tag')}>
          <Tag /> {b.addTag}
        </Button>
        <Button variant="ghost" size="sm" disabled={tagsOnPages.length === 0} onClick={() => open('remove_tag')}>
          <Tags /> {b.removeTag}
        </Button>
        {anyUnchecked && (
          <Button variant="ghost" size="sm" onClick={() => open('mark_checked')}>
            <CheckCheck /> {b.markChecked}
          </Button>
        )}
        <Button variant="ghost" size="sm" className="text-destructive hover:text-destructive" onClick={() => open('delete')}>
          <Trash2 /> {t.common.delete}
        </Button>
      </BulkBar>
      {dialog && <BulkConfirmDialog open onClose={() => setAction(null)} {...dialog} />}
    </>
  )
}
