// The bar for selected tasks in the List view (FR-TASK-UI-7): status, priority, move, labels, delete.
// Every action goes through the listed confirmation with its countdown (components/shared/Bulk.tsx).
import { ArrowRightLeft, ChevronDown, Flag, Tag, Tags, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { commands, type Category, type TaskBulkChange } from '@/api/bindings'
import type { Priority, Task, TaskStatus } from '@/api/tasks'
import { BulkBar, BulkConfirmDialog, NameChips, NamePicker, type BulkItem } from '@/components/shared/Bulk'
import { StatusPill } from '@/components/shared/task-status'
import { CategoryDot, PriorityIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { useT } from '@/i18n'
import { priorityLabel, PRIORITY_ORDER, statusLabel, STATUS_ORDER } from '@/lib/labels'

type Action =
  | { kind: 'status'; status: TaskStatus }
  | { kind: 'priority'; priority: Priority }
  | { kind: 'category'; category: Category }
  | { kind: 'add_label' }
  | { kind: 'remove_label' }
  | { kind: 'delete' }

type Menu = 'status' | 'priority' | 'move'

export function TaskBulkBar({
  tasks,
  categories,
  allLabels,
  onClear,
}: {
  /** The selected tasks, in list order. */
  tasks: Task[]
  categories: Category[]
  allLabels: string[]
  onClear: () => void
}) {
  const [action, setAction] = useState<Action | null>(null)
  const [label, setLabel] = useState('')
  // One menu open at a time: opening a second one closes the first (a press on the other trigger
  // opens it before the first menu sees the press outside, so each menu can't do this alone).
  const [menu, setMenu] = useState<Menu | null>(null)
  // `t` is a task below.
  const m = useT()
  const b = m.tasks.bulk
  const menuProps = (m: Menu) => ({
    open: menu === m,
    onOpenChange: (o: boolean) => setMenu((cur) => (o ? m : cur === m ? null : cur)),
  })
  const open = (a: Action) => {
    setLabel('')
    setMenu(null)
    setAction(a)
  }
  const ids = tasks.map((t) => t.id)
  const n = tasks.length
  const category = (id: string) => categories.find((c) => c.id === id)
  const inCategory = (t: Task) => {
    const c = category(t.categoryId)
    return c ? (
      <>
        <CategoryDot color={c.color} className="size-2" />
        {c.name}
      </>
    ) : null
  }
  const labelsOnTasks = [...new Set(tasks.flatMap((t) => t.labels))].sort((a, b) => a.localeCompare(b))
  const item = (t: Task, detail: React.ReactNode): BulkItem => ({ id: t.id, lead: t.key, label: t.title, detail })
  const update = (change: TaskBulkChange) => () => commands.updateTasks(ids, change)
  const same = b.same

  const dialog = (() => {
    switch (action?.kind) {
      case 'status':
        return {
          title: b.statusTitle(n, statusLabel(action.status)),
          description: b.statusDescription(<StatusPill status={action.status} className="align-middle" />, same),
          items: tasks.map((t) => item(t, <StatusPill status={t.status} />)),
          confirmLabel: b.setStatus(statusLabel(action.status)),
          run: update({ kind: 'status', status: action.status }),
          doneMessage: (d: number) => b.statusDone(d, statusLabel(action.status)),
        }
      case 'priority':
        return {
          title: b.priorityTitle(n, priorityLabel(action.priority)),
          description: b.priorityDescription(<PriorityIcon priority={action.priority} withLabel className="align-middle text-sm" />, same),
          items: tasks.map((t) => item(t, <PriorityIcon priority={t.priority} withLabel />)),
          confirmLabel: b.setPriority(priorityLabel(action.priority)),
          run: update({ kind: 'priority', priority: action.priority }),
          doneMessage: (d: number) => b.priorityDone(d, priorityLabel(action.priority)),
        }
      case 'category':
        return {
          title: b.moveTitle(n, action.category.name),
          description: b.moveDescription(action.category.name),
          items: tasks.map((t) => item(t, inCategory(t))),
          confirmLabel: m.common.move,
          run: update({ kind: 'category', categoryId: action.category.id }),
          doneMessage: (d: number) => b.moveDone(d, action.category.name),
        }
      case 'add_label':
        return {
          title: b.addLabelTitle(n),
          description: b.addLabelDescription,
          items: tasks.map((t) => item(t, <NameChips names={t.labels} />)),
          confirmLabel: b.addLabel,
          ready: label.trim() !== '',
          children: <NamePicker typed noun="label" names={allLabels} value={label} onChange={setLabel} />,
          run: update({ kind: 'add_label', label: label.trim() }),
          doneMessage: (d: number) => b.addLabelDone(label.trim(), d),
        }
      case 'remove_label':
        return {
          title: b.removeLabelTitle(n),
          description: b.removeLabelDescription,
          items: tasks.map((t) => item(t, <NameChips names={t.labels} />)),
          confirmLabel: b.removeLabel,
          ready: label !== '',
          children: <NamePicker noun="label" names={labelsOnTasks} value={label} onChange={setLabel} />,
          run: update({ kind: 'remove_label', label }),
          doneMessage: (d: number) => b.removeLabelDone(label, d),
        }
      case 'delete':
        return {
          title: b.deleteTitle(n),
          description: b.deleteDescription,
          items: tasks.map((t) => item(t, inCategory(t))),
          confirmLabel: b.deleteConfirm(n),
          destructive: true,
          run: () => commands.deleteTasks(ids),
          doneMessage: (d: number) => b.deleteDone(d),
          onDone: onClear,
        }
      default:
        return null
    }
  })()

  return (
    <>
      <BulkBar selected={n} onClear={onClear}>
        <DropdownMenu {...menuProps('status')}>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm">
              {m.common.status} <ChevronDown className="opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="center" side="top" className="min-w-40">
            {STATUS_ORDER.map((s) => (
              <DropdownMenuItem key={s} onSelect={() => open({ kind: 'status', status: s })}>
                <StatusPill status={s} />
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu {...menuProps('priority')}>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm">
              <Flag /> {m.common.priority} <ChevronDown className="opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="center" side="top" className="min-w-36">
            {PRIORITY_ORDER.map((p) => (
              <DropdownMenuItem key={p} onSelect={() => open({ kind: 'priority', priority: p })}>
                <PriorityIcon priority={p} withLabel className="text-sm" />
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <DropdownMenu {...menuProps('move')}>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm">
              <ArrowRightLeft /> {m.common.move} <ChevronDown className="opacity-60" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="center" side="top">
            {categories
              .filter((c) => c.status === 'active')
              .map((c) => (
                <DropdownMenuItem key={c.id} onSelect={() => open({ kind: 'category', category: c })}>
                  <CategoryDot color={c.color} className="size-2" /> {c.name}
                </DropdownMenuItem>
              ))}
          </DropdownMenuContent>
        </DropdownMenu>
        <Button variant="ghost" size="sm" onClick={() => open({ kind: 'add_label' })}>
          <Tag /> {b.addLabel}
        </Button>
        <Button variant="ghost" size="sm" disabled={labelsOnTasks.length === 0} onClick={() => open({ kind: 'remove_label' })}>
          <Tags /> {b.removeLabel}
        </Button>
        <Button variant="ghost" size="sm" className="text-destructive hover:text-destructive" onClick={() => open({ kind: 'delete' })}>
          <Trash2 /> {m.common.delete}
        </Button>
      </BulkBar>
      {dialog && <BulkConfirmDialog open onClose={() => setAction(null)} {...dialog} />}
    </>
  )
}
