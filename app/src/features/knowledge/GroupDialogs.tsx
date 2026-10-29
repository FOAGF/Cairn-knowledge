import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import type { Group } from '@/api/knowledge'
import { useIsExternal } from '@/api/storage'
import { useT } from '@/i18n'
import { groupAndInside, groupOptions } from './groupTree'

/** Name prompt for creating or renaming a group (FR-KN-GR-1: one level only; external mode nests, FR-KN-EXT-2). */
export function GroupNameDialog({
  open,
  title,
  initial = '',
  initialDescription = '',
  onClose,
  onSubmit,
}: {
  open: boolean
  title: string
  initial?: string
  initialDescription?: string
  onClose: () => void
  onSubmit: (name: string, description: string) => void
}) {
  const external = useIsExternal()
  const t = useT()
  const d = t.knowledge.groupDialog
  const [name, setName] = useState(initial)
  const [description, setDescription] = useState(initialDescription)
  useEffect(() => {
    if (open) {
      setName(initial)
      setDescription(initialDescription)
    }
  }, [open, initial, initialDescription])
  const submit = () => {
    if (name.trim()) onSubmit(name.trim(), description.trim())
  }
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{external ? d.externalHint : d.internalHint}</DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <Input autoFocus value={name} placeholder={d.namePlaceholder} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && !e.nativeEvent.isComposing && submit()} />
          <div className="space-y-1.5">
            <Textarea rows={3} value={description} placeholder={d.descriptionPlaceholder} onChange={(e) => setDescription(e.target.value)} />
            <p className="text-xs text-muted-foreground">{d.descriptionHint}</p>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button disabled={!name.trim()} onClick={submit}>
            {t.common.save}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function ConfirmDialog({
  open,
  title,
  description,
  confirmLabel,
  onClose,
  onConfirm,
}: {
  open: boolean
  title: string
  description: string
  confirmLabel: string
  onClose: () => void
  onConfirm: () => void
}) {
  const t = useT()
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

const TOP = '__top'

/**
 * External mode: *Move to…* for a group (its folder), into another group of the same category or
 * directly into the category. Not into itself or a group inside it.
 */
export function GroupMoveDialog({
  group,
  groups,
  categoryName,
  onClose,
  onMove,
}: {
  group: Group | null
  /** The groups of the group's category. */
  groups: Group[]
  categoryName: string
  onClose: () => void
  /** `''`: directly in the category. */
  onMove: (parentId: string) => void
}) {
  return (
    <Dialog open={!!group} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md" data-testid="group-move-dialog">
        {group && <MoveBody group={group} groups={groups} categoryName={categoryName} onClose={onClose} onMove={onMove} />}
      </DialogContent>
    </Dialog>
  )
}

function MoveBody({ group, groups, categoryName, onClose, onMove }: { group: Group; groups: Group[]; categoryName: string; onClose: () => void; onMove: (parentId: string) => void }) {
  const current = group.parentId && groups.some((g) => g.id === group.parentId) ? group.parentId : TOP
  const [target, setTarget] = useState(current)
  const inside = groupAndInside(group, groups)
  const options = groupOptions(groups).filter((o) => !inside.has(o.group.id))
  const t = useT()
  const d = t.knowledge.groupDialog
  return (
    <>
      <DialogHeader>
        <DialogTitle>{d.moveTitle(group.name)}</DialogTitle>
        <DialogDescription>{d.moveDescription}</DialogDescription>
      </DialogHeader>
      <div className="space-y-1.5">
        <Label>{d.moveInto}</Label>
        <Select value={target} onValueChange={setTarget}>
          <SelectTrigger className="w-full" aria-label={d.moveInto}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={TOP}>{d.directlyIn(categoryName)}</SelectItem>
            {options.map(({ group: g, label }) => (
              <SelectItem key={g.id} value={g.id}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <DialogFooter>
        <Button variant="outline" onClick={onClose}>
          {t.common.cancel}
        </Button>
        <Button disabled={target === current} onClick={() => onMove(target === TOP ? '' : target)} data-testid="group-move-confirm">
          {t.common.move}
        </Button>
      </DialogFooter>
    </>
  )
}

export type GroupDialogState =
  /** `parentId`: a group to make it in (external mode). */
  | { kind: 'new'; categoryId: string; parentId?: string }
  | { kind: 'rename'; group: Group }
  | { kind: 'move'; group: Group }
  | { kind: 'delete'; group: Group }
  | { kind: 'suggest'; categoryId: string }
  | null
