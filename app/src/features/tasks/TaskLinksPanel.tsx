// Task links (FR-TASK-LN-1..2), linked knowledge pages (FR-TASK-LN-3 / DM-5) and "Mentioned in" (DM-MN-4).
import { FileText, Link2, MessageSquare, Plus, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { SectionCard } from '@/components/shared/PageHeader'
import { CategoryDot } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { STATUS_TINT, TINT_TEXT } from '@/lib/labels'
import { ALL_TASKS, type Task, type TaskLink, useAddTaskLink, useRemoveTaskLink, useTaskLinks, useTaskList, useTasks } from '@/api/tasks'
import { type PageFilter, useBacklinks, useLinkTaskPage, usePageList, useTaskPageLinks, useUnlinkTaskPage } from '@/api/knowledge'
import { cn } from '@/lib/utils'
import { useCategoryList } from '@/api/categories'

/** A linked page may have been archived since; it should still show. */
const WITH_ARCHIVED: PageFilter = { includeArchived: true }

function TaskRow({ task, onRemove }: { task: Task; onRemove: () => void }) {
  const t = useT()
  const closed = task.status === 'done' || task.status === 'wont_do'
  return (
    <li className="group flex items-center gap-2 rounded-lg px-1.5 py-1 hover:bg-muted/50">
      <span className={cn('size-2 shrink-0 rounded-full', TINT_TEXT[STATUS_TINT[task.status]])} style={{ backgroundColor: 'currentcolor' }} />
      <Link to={`/tasks/${task.key}`} className="min-w-0 flex-1 truncate text-sm hover:text-primary">
        <span className="mr-1.5 font-mono text-xs text-muted-foreground">{task.key}</span>
        <span className={closed ? 'text-muted-foreground line-through' : ''}>{task.title}</span>
      </Link>
      <Button variant="ghost" size="icon-xs" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100" aria-label={t.tasks.links.remove(task.key)} onClick={onRemove}>
        <X />
      </Button>
    </li>
  )
}

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-1 text-xs font-medium text-muted-foreground">{title}</p>
      <ul>{children}</ul>
    </div>
  )
}

type LinkKind = 'relates' | 'parent-of' | 'subtask-of'

function AddLinkDialog({ task, open, onOpenChange }: { task: Task; open: boolean; onOpenChange: (o: boolean) => void }) {
  const links = useTaskLinks(task.id)
  const categories = useCategoryList()
  const addTaskLink = useAddTaskLink()
  const [kind, setKind] = useState<LinkKind>('relates')
  const [q, setQ] = useState('')
  const [picked, setPicked] = useState<string | null>(null)
  const m = useT()

  // Rust searches key, title and description; closed tasks can be linked too.
  const text = q.trim()
  const filter = useMemo(() => ({ text, limit: 60, includeClosed: true }), [text])
  const found = useTasks(filter).data

  const results = useMemo(() => {
    const linked = new Set(links.flatMap((l) => [l.fromTaskId, l.toTaskId]))
    return (found ?? []).filter((t) => t.id !== task.id && !linked.has(t.id)).slice(0, 40)
  }, [found, links, task.id])

  const save = () => {
    if (!picked) return
    const [type, from, to] =
      kind === 'relates' ? (['relates', task.id, picked] as const) : kind === 'parent-of' ? (['parent', task.id, picked] as const) : (['parent', picked, task.id] as const)
    // A refused link (FR-TASK-LN-4) comes back as a toast; keep the dialog open then.
    addTaskLink.mutate(
      { type, fromTaskId: from, toTaskId: to },
      {
        onSuccess: () => {
          onOpenChange(false)
          setPicked(null)
          setQ('')
        },
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{m.tasks.links.add}</DialogTitle>
          <DialogDescription>{m.tasks.links.addDescription(task.key)}</DialogDescription>
        </DialogHeader>
        <Select value={kind} onValueChange={(v) => setKind(v as LinkKind)}>
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="relates">{m.tasks.links.relatesTo}</SelectItem>
            <SelectItem value="parent-of">{m.tasks.links.parentOf}</SelectItem>
            <SelectItem value="subtask-of">{m.tasks.links.subtaskOf}</SelectItem>
          </SelectContent>
        </Select>
        <Input autoFocus placeholder={m.tasks.links.search} value={q} onChange={(e) => setQ(e.target.value)} />
        <ul className="max-h-64 space-y-0.5 overflow-y-auto">
          {results.map((t) => (
            <li key={t.id}>
              <button
                onClick={() => setPicked(t.id)}
                className={cn('flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm hover:bg-muted', picked === t.id && 'bg-accent text-accent-foreground')}
              >
                <span className="font-mono text-xs text-muted-foreground">{t.key}</span>
                <span className="min-w-0 flex-1 truncate">{t.title}</span>
                <CategoryDot color={categories.find((p) => p.id === t.categoryId)?.color ?? 'slate'} className="size-2" />
              </button>
            </li>
          ))}
          {results.length === 0 && <li className="p-3 text-center text-sm text-muted-foreground">{m.tasks.links.noTasks}</li>}
        </ul>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {m.common.cancel}
          </Button>
          <Button disabled={!picked} onClick={save}>
            {m.tasks.links.add}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function AddPageDialog({ task, open, onOpenChange }: { task: Task; open: boolean; onOpenChange: (o: boolean) => void }) {
  const pages = usePageList()
  const pageLinks = useTaskPageLinks({ taskId: task.id })
  const { mutate: linkTaskPage } = useLinkTaskPage()
  const [q, setQ] = useState('')
  const t = useT()
  const results = useMemo(() => {
    const linked = new Set(pageLinks.map((l) => l.pageId))
    const needle = q.trim().toLowerCase()
    return pages.filter((p) => !linked.has(p.id) && (!needle || p.title.toLowerCase().includes(needle))).slice(0, 40)
  }, [pages, pageLinks, q])
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t.tasks.links.pageTitle}</DialogTitle>
          <DialogDescription>{t.tasks.links.pageDescription(task.key)}</DialogDescription>
        </DialogHeader>
        <Input autoFocus placeholder={t.tasks.links.searchPages} value={q} onChange={(e) => setQ(e.target.value)} />
        <ul className="max-h-72 space-y-0.5 overflow-y-auto">
          {results.map((p) => (
            <li key={p.id}>
              <button
                className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm hover:bg-muted"
                onClick={() => {
                  linkTaskPage({ taskId: task.id, pageId: p.id })
                  onOpenChange(false)
                }}
              >
                <FileText className="size-4 shrink-0 text-tint-purple-fg" />
                <span className="truncate">{p.title}</span>
              </button>
            </li>
          ))}
          {results.length === 0 && <li className="p-3 text-center text-sm text-muted-foreground">{t.tasks.links.noPages}</li>}
        </ul>
      </DialogContent>
    </Dialog>
  )
}

export function TaskLinksPanel({ task }: { task: Task }) {
  const all = useTaskList(ALL_TASKS)
  const links = useTaskLinks(task.id)
  const removeTaskLink = useRemoveTaskLink()
  const [open, setOpen] = useState(false)
  const m = useT().tasks.links

  const byIdMap = useMemo(() => new Map(all.map((t) => [t.id, t])), [all])
  const byId = (id: string) => byIdMap.get(id)
  const groups = useMemo(() => {
    const parent = links.filter((l) => l.type === 'parent' && l.toTaskId === task.id)
    const subs = links.filter((l) => l.type === 'parent' && l.fromTaskId === task.id)
    const rel = links.filter((l) => l.type === 'relates')
    return { parent, subs, rel }
  }, [links, task.id])
  const other = (l: TaskLink) => byId(l.fromTaskId === task.id ? l.toTaskId : l.fromTaskId)
  const render = (list: TaskLink[], pick: (l: TaskLink) => Task | undefined) =>
    list.map((l) => {
      const t = pick(l)
      return t ? <TaskRow key={l.id} task={t} onRemove={() => removeTaskLink.mutate(l.id)} /> : null
    })
  const empty = !groups.parent.length && !groups.subs.length && !groups.rel.length

  return (
    <SectionCard
      title={m.taskLinks}
      icon={Link2}
      tint="blue"
      action={
        <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
          <Plus /> {m.add}
        </Button>
      }
    >
      <div className="space-y-3">
        {empty && <p className="text-sm text-muted-foreground">{m.noLinkedTasks}</p>}
        {groups.parent.length > 0 && <Group title={m.parent}>{render(groups.parent, (l) => byId(l.fromTaskId))}</Group>}
        {groups.subs.length > 0 && <Group title={m.subtasks}>{render(groups.subs, (l) => byId(l.toTaskId))}</Group>}
        {groups.rel.length > 0 && <Group title={m.relatesTo}>{render(groups.rel, other)}</Group>}
      </div>
      <AddLinkDialog task={task} open={open} onOpenChange={setOpen} />
    </SectionCard>
  )
}

export function LinkedPagesPanel({ task }: { task: Task }) {
  const pages = usePageList(WITH_ARCHIVED)
  const mine = useTaskPageLinks({ taskId: task.id })
  const { mutate: unlinkTaskPage } = useUnlinkTaskPage()
  const [open, setOpen] = useState(false)
  const m = useT().tasks.links
  return (
    <SectionCard
      title={m.knowledgePages}
      icon={FileText}
      tint="purple"
      action={
        <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
          <Plus /> {m.linkPage}
        </Button>
      }
    >
      {mine.length === 0 && <p className="text-sm text-muted-foreground">{m.noLinkedPages}</p>}
      <ul>
        {mine.map((l) => {
          const p = pages.find((x) => x.id === l.pageId)
          if (!p) return null
          return (
            <li key={l.id} className="group flex items-center gap-2 rounded-lg px-1.5 py-1 hover:bg-muted/50">
              <Link to={`/knowledge/${p.id}`} className="min-w-0 flex-1 truncate text-sm hover:text-primary">
                {p.title}
              </Link>
              <Button variant="ghost" size="icon-xs" className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100" aria-label={m.unlink(p.title)} onClick={() => unlinkTaskPage(l.id)}>
                <X />
              </Button>
            </li>
          )
        })}
      </ul>
      <AddPageDialog task={task} open={open} onOpenChange={setOpen} />
    </SectionCard>
  )
}

/** Backlinks: pages, tasks and comments whose text mentions `#KEY` (DM-MN-4). */
export function MentionedInPanel({ task }: { task: Task }) {
  const backlinks = useBacklinks('task', task.id)
  const m = useT().tasks.links
  if (backlinks.length === 0) return null
  return (
    <SectionCard title={m.mentionedIn} icon={MessageSquare} tint="amber">
      <ul>
        {backlinks.map((b) => {
          const Icon = b.source === 'page' ? FileText : b.source === 'comment' ? MessageSquare : Link2
          return (
            <li key={`${b.source}:${b.sourceId}`}>
              <Link to={b.pageId ? `/knowledge/${b.pageId}` : `/tasks/${b.taskId}`} className="flex items-center gap-2 rounded-lg px-1.5 py-1 text-sm hover:bg-muted/50 hover:text-primary">
                <Icon className="size-4 shrink-0 text-muted-foreground" />
                <span className="truncate">{b.label}</span>
                {b.source === 'comment' && <span className="shrink-0 text-xs text-muted-foreground">{m.comment}</span>}
              </Link>
            </li>
          )
        })}
      </ul>
    </SectionCard>
  )
}
