import { BookOpen, Check, FilePlus2, FolderPlus, FolderOpen, Pencil, Upload, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { FoldButton, useFold } from '@/components/shared/Fold'
import { Link, useNavigate } from 'react-router'
import { SectionCard } from '@/components/shared/PageHeader'
import { TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import type { Category, TintColor } from '@/lib/types'
import { type PageSummary, useCreateGroup, useGroups, usePageList, useUpdateGroup } from '@/api/knowledge'
import { useIsExternal } from '@/api/storage'
import { groupPath } from '@/features/knowledge/groupTree'
import { useT } from '@/i18n'

const GROUP_TINTS: TintColor[] = ['purple', 'green', 'amber', 'blue', 'rose']

/** Small amber dot for pages that have not been checked yet. */
function PageLinkRow({ page }: { page: PageSummary }) {
  const t = useT()
  return (
    <li>
      <Link to={`/knowledge/${page.id}`} className="flex items-center gap-2 text-sm hover:text-primary">
        <span className="truncate">{page.title}</span>
        {page.checkStatus === 'not_checked' && (
          <span className="size-2 shrink-0 rounded-full bg-tint-amber-fg" title={t.categories.group.notCheckedYet} aria-label={t.categories.group.notCheckedYet} />
        )}
      </Link>
    </li>
  )
}

function GroupCard({
  name,
  description,
  pages,
  tint,
  to,
  onSaveDescription,
}: {
  name: string
  description?: string
  pages: PageSummary[]
  tint: TintColor
  to: string
  /** Omit for the Ungrouped card (it has no description). */
  onSaveDescription?: (d: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const sorted = [...pages].sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
  // Show the 3 latest pages; the rest folds (FR-PRJ-4).
  const pageFold = useFold(sorted, PAGE_LIMIT)
  const t = useT()
  const k = t.categories.group
  const save = () => {
    onSaveDescription?.(draft.trim())
    setEditing(false)
  }
  return (
    <div className="flex flex-col rounded-xl border bg-card p-4">
      <Link to={to} className="group flex items-center gap-3">
        <TintIcon icon={FolderOpen} tint={tint} />
        <div className="min-w-0 flex-1">
          <p className="truncate font-semibold group-hover:text-primary">{name}</p>
          <p className="text-xs text-muted-foreground">{k.pageCount(pages.length)}</p>
        </div>
      </Link>

      <div className="mt-3 min-h-9">
        {editing ? (
          <div className="space-y-2">
            <Textarea autoFocus rows={2} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={k.descriptionPlaceholder} className="text-sm" />
            <div className="flex justify-end gap-1">
              <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
                <X /> {t.common.cancel}
              </Button>
              <Button size="sm" onClick={save}>
                <Check /> {t.common.save}
              </Button>
            </div>
          </div>
        ) : onSaveDescription ? (
          <button
            className="group/d flex w-full items-start gap-1.5 text-left text-sm"
            onClick={() => {
              setDraft(description ?? '')
              setEditing(true)
            }}
          >
            <span className={description ? 'line-clamp-2 text-muted-foreground' : 'text-muted-foreground/70 italic'}>
              {description || k.addDescription}
            </span>
            <Pencil className="mt-0.5 size-3 shrink-0 text-muted-foreground opacity-0 group-hover/d:opacity-100" />
          </button>
        ) : (
          <p className="text-sm text-muted-foreground">{k.ungroupedNote}</p>
        )}
      </div>

      {sorted.length > 0 ? (
        <div className="mt-3 border-t pt-3">
          <ul className="space-y-1.5">
            {pageFold.visible.map((p) => (
              <PageLinkRow key={p.id} page={p} />
            ))}
          </ul>
          <FoldButton fold={pageFold} noun={k.pageFoldNoun(pageFold.hiddenCount)} className="mt-1 py-1 text-xs" />
        </div>
      ) : (
        <p className="mt-3 border-t pt-3 text-sm text-muted-foreground">{k.noPages}</p>
      )}
    </div>
  )
}

function NewGroupDialog({ open, onClose, categoryId }: { open: boolean; onClose: () => void; categoryId: string }) {
  const { mutate: createGroup } = useCreateGroup()
  const external = useIsExternal()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const t = useT()
  const close = () => {
    setName('')
    setDescription('')
    onClose()
  }
  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t.categories.knowledge.newGroup}</DialogTitle>
          <DialogDescription>{external ? t.knowledge.groupDialog.externalHint : t.knowledge.groupDialog.internalHint}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="gname">{t.common.name}</Label>
            <Input id="gname" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="gdesc">{t.common.description}</Label>
            <Textarea id="gdesc" rows={3} value={description} onChange={(e) => setDescription(e.target.value)} />
            <p className="text-xs text-muted-foreground">{t.categories.newGroupDialog.descriptionHint}</p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={close}>
            {t.common.cancel}
          </Button>
          <Button
            disabled={!name.trim()}
            onClick={() => {
              createGroup({ categoryId, name: name.trim(), description: description.trim() || null })
              close()
            }}
          >
            {t.categories.newGroupDialog.create}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Group cards and pages per card shown before folding. */
const GROUP_LIMIT = 3
const PAGE_LIMIT = 3

/** Knowledge of one category: group cards plus ungrouped pages. */
export function CategoryKnowledgeSection({ category }: { category: Category }) {
  const navigate = useNavigate()
  const groups = useGroups(category.id)
  const pageFilter = useMemo(() => ({ categoryIds: [category.id] }), [category.id])
  const pages = usePageList(pageFilter)
  const { mutate: updateGroup } = useUpdateGroup()
  const [newGroup, setNewGroup] = useState(false)
  const t = useT()
  const k = t.categories.knowledge

  const ungrouped = useMemo(() => pages.filter((p) => !p.groupId || !groups.some((g) => g.id === p.groupId)), [pages, groups])
  // Many groups fold automatically (FR-PRJ-4); Ungrouped is shown once everything is visible.
  const groupFold = useFold(groups, GROUP_LIMIT)
  const to = `/knowledge?category=${category.id}`

  return (
    <SectionCard
      icon={BookOpen}
      tint="purple"
      title={t.common.nav.knowledge}
      description={k.description(pages.length, groups.length)}
      className="min-w-0"
      action={
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="outline" size="sm" onClick={() => setNewGroup(true)}>
            <FolderPlus /> {k.newGroup}
          </Button>
          <Button variant="outline" size="sm" onClick={() => navigate('/assistant?attach=1')}>
            <Upload /> {k.fromFile}
          </Button>
          <Button size="sm" onClick={() => navigate('/knowledge/new')}>
            <FilePlus2 /> {t.common.newPage}
          </Button>
        </div>
      }
    >
      {groups.length === 0 && ungrouped.length === 0 ? (
        <p className="rounded-xl border border-dashed py-8 text-center text-sm text-muted-foreground">{k.empty}</p>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-1 2xl:grid-cols-2">
          {groupFold.visible.map((g, i) => (
            <GroupCard
              key={g.id}
              name={groupPath(g, groups)}
              description={g.description}
              pages={pages.filter((p) => p.groupId === g.id)}
              tint={GROUP_TINTS[i % GROUP_TINTS.length]}
              to={to}
              onSaveDescription={(d) => updateGroup({ id: g.id, patch: { description: d } })}
            />
          ))}
          {ungrouped.length > 0 && (groupFold.expanded || groupFold.hiddenCount === 0) && <GroupCard name={t.knowledge.ungrouped} pages={ungrouped} tint="slate" to={to} />}
        </div>
      )}
      {(groups.length > 0 || ungrouped.length > 0) && (
        <FoldButton
          fold={{ ...groupFold, hiddenCount: groupFold.hiddenCount + (groupFold.hiddenCount > 0 && ungrouped.length > 0 ? 1 : 0) }}
          noun={k.groupFoldNoun(groupFold.hiddenCount + (groupFold.hiddenCount > 0 && ungrouped.length > 0 ? 1 : 0))}
          className="mt-2"
        />
      )}
      <NewGroupDialog open={newGroup} onClose={() => setNewGroup(false)} categoryId={category.id} />
    </SectionCard>
  )
}
