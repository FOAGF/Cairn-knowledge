// The proposed-change card under an assistant message (FR-LLM-AS-3, UI-AS-2): a row per change
// with a tick, *Confirm* / *Edit…* / *Reject*, then each row's result ("Created OPS-48" with a
// link); a page change shows its diff. Nothing is saved before *Confirm*. *Edit…* opens the normal
// form, filled in: the new-task dialog; a comment or task change edited in the card, which
// *Confirm* then saves; a page change as the page's unsaved changes in its editor.
// Owned by the M6 actions agent; MessageView renders it for `message.proposal`.
import { ArrowRight, Check, ChevronDown, FilePen, ListChecks, Loader2, MessageSquare, Minus, Pencil, Plus, TriangleAlert, X } from 'lucide-react'
import { useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { confirmProposal, type LinkTo, type Proposal, type ProposalRow, type ProposedChange, rejectProposal, setProposalRow } from '@/api/assistant'
import { commands, type TaskStatus } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { type ChatMessage, updateMessage } from '@/api/conversations'
import { useCategoryList } from '@/api/categories'
import { writeDraft } from '@/api/drafts'
import { useTask } from '@/api/tasks'
import { MarkdownField } from '@/components/editor/MarkdownField'
import { PriorityIcon, CategoryDot, StatusBadge, TaskTypeIcon, TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { type Messages, useT } from '@/i18n'
import { PRIORITY_ORDER, STATUS_ORDER, TINT_CLASS, TYPE_ORDER, typeLabel } from '@/lib/labels'
import type { Priority, TaskType } from '@/lib/types'
import { cn } from '@/lib/utils'
import { useAppStore } from '@/store'
import { askBeforeEditing, holdPageEdit, pendingPath } from '../creation/pending'
import { TermSuggestions } from '../glossary/TermSuggestions'
import { NewTaskDialog } from '../tasks/NewTaskDialog'
import { DatePicker, LabelInput } from '../tasks/taskUi'
import { PageDiff } from './PageDiff'

type Change<Op extends ProposedChange['op']> = Extract<ProposedChange, { op: Op }>

function summary(c: ProposedChange, tp: Messages['assistant']['proposal']): string {
  switch (c.op) {
    case 'create_task':
      return tp.createTask
    case 'update_task':
      return tp.updateTask(c.key)
    case 'comment':
      return tp.comment(c.key)
    case 'update_page':
      return tp.changePage(c.title)
  }
}

/** The task key in "Created OPS-48", for the link after Confirm. */
function createdKey(result: string | null): string | undefined {
  return /^Created (\S+)/.exec(result ?? '')?.[1]
}

function FieldList({ children }: { children: React.ReactNode }) {
  return <dl className="mt-2 grid grid-cols-[5.5rem_1fr] items-baseline gap-x-3 gap-y-1.5 text-sm">{children}</dl>
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
    </>
  )
}

/** old → new. */
function Arrow({ from, to }: { from: React.ReactNode; to: React.ReactNode }) {
  return (
    <span className="inline-flex flex-wrap items-center gap-1.5">
      <span className="text-muted-foreground">{from}</span>
      <ArrowRight className="size-3.5 text-muted-foreground" />
      <span className="font-medium">{to}</span>
    </span>
  )
}

function Chips({ items }: { items: string[] }) {
  const m = useT()
  if (!items.length) return <span className="text-muted-foreground">{m.common.none}</span>
  return (
    <span className="flex flex-wrap gap-1.5">
      {items.map((t) => (
        <span key={t} className={cn('rounded-full px-2.5 py-0.5 text-xs font-medium', TINT_CLASS.blue)}>
          {t}
        </span>
      ))}
    </span>
  )
}

function LinkList({ links }: { links: LinkTo[] }) {
  const t = useT()
  return (
    <ul className="space-y-0.5">
      {links.map((l) => (
        <li key={`${l.kind}-${l.targetId}`}>
          <span className="text-muted-foreground">{t.assistant.proposal.links[l.kind]} </span>
          {l.targetName}
        </li>
      ))}
    </ul>
  )
}

function CreateFields({ c, editable, onCategory }: { c: Change<'create_task'>; editable: boolean; onCategory: (categoryId: string) => void }) {
  const categories = useCategoryList()
  const category = categories.find((p) => p.id === c.categoryId)
  const t = useT()
  return (
    <FieldList>
      <Field label={t.common.category}>
        {editable ? (
          <Select value={c.categoryId} onValueChange={onCategory}>
            <SelectTrigger size="sm" className="w-auto gap-2 rounded-full" aria-label={t.assistant.changeCategory}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {categories.filter((p) => p.status === 'active').map((p) => (
                <SelectItem key={p.id} value={p.id}>
                  <CategoryDot color={p.color} className="size-2" />
                  {p.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          category?.name
        )}
      </Field>
      <Field label={t.common.title}>
        <span className="font-medium">{c.title}</span>
      </Field>
      <Field label={t.common.type}>
        <TaskTypeIcon type={c.taskType} withLabel />
      </Field>
      <Field label={t.common.priority}>
        <PriorityIcon priority={c.priority} withLabel />
      </Field>
      <Field label={t.assistant.proposal.due}>{c.due ?? <span className="text-muted-foreground">{t.common.none}</span>}</Field>
      <Field label={t.common.labels}>
        <Chips items={c.labels} />
      </Field>
      {c.description && (
        <Field label={t.common.description}>
          <span className="line-clamp-4 whitespace-pre-line">{c.description}</span>
        </Field>
      )}
      {c.links.length > 0 && (
        <Field label={t.assistant.proposal.linksField}>
          <LinkList links={c.links} />
        </Field>
      )}
    </FieldList>
  )
}

/** The changed fields as old → new, with the task's current values read live. */
function UpdateFields({ c }: { c: Change<'update_task'> }) {
  const { data: task } = useTask(c.taskId)
  const t = useT()
  const none = <span>{t.common.none}</span>
  return (
    <>
      <p className="mt-0.5 truncate text-sm text-muted-foreground">{c.title}</p>
      <FieldList>
        {c.status && (
          <Field label={t.common.status}>
            <Arrow from={task ? <StatusBadge status={task.status} /> : '…'} to={<StatusBadge status={c.status} />} />
          </Field>
        )}
        {c.priority && (
          <Field label={t.common.priority}>
            <Arrow from={task ? <PriorityIcon priority={task.priority} withLabel /> : '…'} to={<PriorityIcon priority={c.priority} withLabel />} />
          </Field>
        )}
        {c.taskType && (
          <Field label={t.common.type}>
            <Arrow from={task ? typeLabel(task.type) : '…'} to={typeLabel(c.taskType)} />
          </Field>
        )}
        {(c.due || c.clearDue) && (
          <Field label={t.assistant.proposal.due}>
            <Arrow from={task ? (task.dueDate ?? none) : '…'} to={c.due ?? t.common.noDueDate} />
          </Field>
        )}
        {c.labels && (
          <Field label={t.common.labels}>
            <Arrow from={task ? <Chips items={task.labels} /> : '…'} to={<Chips items={c.labels} />} />
          </Field>
        )}
        {c.description !== null && (
          <Field label={t.common.description}>
            <span className="line-clamp-4 whitespace-pre-line">{c.description || t.assistant.proposal.cleared}</span>
          </Field>
        )}
        {c.links.length > 0 && (
          <Field label={t.assistant.proposal.addLinks}>
            <LinkList links={c.links} />
          </Field>
        )}
      </FieldList>
    </>
  )
}

/** *Done* (keeps the edit in the card for *Confirm*) and *Cancel*. */
function EditButtons({ onDone, onCancel, disabled }: { onDone: () => void; onCancel: () => void; disabled?: boolean }) {
  const t = useT()
  return (
    <div className="mt-2 flex gap-2">
      <Button size="sm" disabled={disabled} onClick={onDone}>
        <Check /> {t.assistant.proposal.editDone}
      </Button>
      <Button size="sm" variant="ghost" onClick={onCancel}>
        {t.common.cancel}
      </Button>
    </div>
  )
}

/** A proposed comment's text, edited in the card. */
function CommentEditor({ c, onDone, onCancel }: { c: Change<'comment'>; onDone: (c: Change<'comment'>) => void; onCancel: () => void }) {
  const [text, setText] = useState(c.text)
  return (
    <div className="mt-1.5">
      <MarkdownField small value={text} onChange={setText} autoFocus minHeight="4rem" />
      <EditButtons disabled={!text.trim()} onDone={() => onDone({ ...c, text: text.trim() })} onCancel={onCancel} />
    </div>
  )
}

/** A proposed task change edited in the card: the fields it changes, with the task screen's controls. */
function UpdateEditor({ c, onDone, onCancel }: { c: Change<'update_task'>; onDone: (c: Change<'update_task'>) => void; onCancel: () => void }) {
  const [d, setD] = useState(c)
  const t = useT()
  return (
    <>
      <p className="mt-0.5 truncate text-sm text-muted-foreground">{c.title}</p>
      <FieldList>
        {c.status && (
          <Field label={t.common.status}>
            <Select value={d.status ?? c.status} onValueChange={(v) => setD({ ...d, status: v as TaskStatus })}>
              <SelectTrigger size="sm" className="w-auto gap-2">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {STATUS_ORDER.map((st) => (
                  <SelectItem key={st} value={st}>
                    <StatusBadge status={st} />
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
        )}
        {c.priority && (
          <Field label={t.common.priority}>
            <Select value={d.priority ?? c.priority} onValueChange={(v) => setD({ ...d, priority: v as Priority })}>
              <SelectTrigger size="sm" className="w-auto gap-2">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PRIORITY_ORDER.map((p) => (
                  <SelectItem key={p} value={p}>
                    <PriorityIcon priority={p} withLabel />
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
        )}
        {c.taskType && (
          <Field label={t.common.type}>
            <Select value={d.taskType ?? c.taskType} onValueChange={(v) => setD({ ...d, taskType: v as TaskType })}>
              <SelectTrigger size="sm" className="w-auto gap-2">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TYPE_ORDER.map((ty) => (
                  <SelectItem key={ty} value={ty}>
                    <TaskTypeIcon type={ty} withLabel />
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
        )}
        {(c.due || c.clearDue) && (
          <Field label={t.assistant.proposal.due}>
            <DatePicker value={d.due} onChange={(v) => setD({ ...d, due: v ?? null, clearDue: !v })} placeholder={t.common.noDueDate} />
          </Field>
        )}
        {c.labels && (
          <Field label={t.common.labels}>
            <LabelInput value={d.labels ?? []} onChange={(labels) => setD({ ...d, labels })} />
          </Field>
        )}
        {c.description !== null && (
          <Field label={t.common.description}>
            <MarkdownField small value={d.description ?? ''} onChange={(description) => setD({ ...d, description })} minHeight="4rem" />
          </Field>
        )}
        {c.links.length > 0 && (
          <Field label={t.assistant.proposal.addLinks}>
            <LinkList links={c.links} />
          </Field>
        )}
      </FieldList>
      <EditButtons onDone={() => onDone(d)} onCancel={onCancel} />
    </>
  )
}

function PageChange({ c }: { c: Change<'update_page'> }) {
  const [open, setOpen] = useState(false)
  const t = useT()
  return (
    <div className="mt-1">
      <p className="text-sm text-muted-foreground">{c.change}</p>
      <button type="button" onClick={() => setOpen(!open)} className="mt-1.5 inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">
        <ChevronDown className={cn('size-3.5 transition-transform', open && 'rotate-180')} />
        {open ? t.assistant.proposal.hideChanges : t.assistant.proposal.showChanges}
      </button>
      {open && (
        <div className="mt-2">
          <PageDiff oldBody={c.oldBody} newBody={c.newBody} />
        </div>
      )}
    </div>
  )
}

/** After Confirm: the result with a link, or why the row failed. */
function Result({ row }: { row: ProposalRow }) {
  const { change: c } = row
  const t = useT()
  if (row.state === 'failed') {
    return (
      <p className="mt-2 flex items-start gap-1.5 text-sm text-destructive">
        <TriangleAlert className="mt-0.5 size-4 shrink-0" />
        <span>{row.result ?? t.assistant.proposal.failed}</span>
      </p>
    )
  }
  if (row.state === 'rejected') return <p className="mt-2 text-sm text-muted-foreground">{t.assistant.proposal.rejectedRow}</p>
  if (row.state !== 'done') return null
  const key = c.op === 'create_task' ? createdKey(row.result) : c.op === 'update_page' ? undefined : c.key
  const to = c.op === 'update_page' ? (row.resultId ? `/knowledge/${row.resultId}` : undefined) : key ? `/tasks/${key}` : undefined
  const label = row.result ?? t.assistant.proposal.done
  return (
    <p className="mt-2 flex items-center gap-1.5 text-sm font-medium text-tint-green-fg">
      <Check className="size-4" />
      {to ? (
        <Link to={to} className="underline">
          {label}
        </Link>
      ) : (
        label
      )}
    </p>
  )
}

function RowView({
  message,
  index,
  row,
  open,
  patchChange,
}: {
  message: ChatMessage
  index: number
  row: ProposalRow
  /** The card still waits for *Confirm* or *Reject*. */
  open: boolean
  patchChange: (index: number, change: ProposedChange) => void
}) {
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const tp = useT().assistant.proposal
  const c = row.change
  const pending = open && row.state === 'pending'
  const off = row.state === 'rejected' || (pending && !row.ticked)

  // A page change becomes the page's unsaved changes (UI-KN-6a), so the page opens straight into
  // editing with the proposed text. It is saved there, so Confirm must not save it again.
  const editPage = async (c: Change<'update_page'>) => {
    try {
      // Opening the same proposal again continues the user's edits instead of replacing them.
      if (useAppStore.getState().pageEdit?.pageId === c.pageId) {
        void setProposalRow(message.id, index, false)
        return navigate(`/knowledge/${c.pageId}`)
      }
      const page = await commands.getPage(c.pageId)
      if (page.updatedAt !== c.baseUpdatedAt) return void toast.error(tp.pageChanged)
      const choice = await askBeforeEditing(page.id)
      if (choice === 'continue') return navigate(pendingPath() ?? '/knowledge')
      if (choice !== 'start') return
      await writeDraft('page', page.id, JSON.stringify({ title: page.title, body: c.newBody, tags: page.tags }))
      holdPageEdit(page.id, page.title)
      void setProposalRow(message.id, index, false)
      navigate(`/knowledge/${page.id}`)
    } catch (e) {
      toast.error(errorMessage(e))
    }
  }
  const edit = () => (c.op === 'update_page' ? void editPage(c) : setEditing(true))
  const done = (change: ProposedChange) => {
    patchChange(index, change)
    setEditing(false)
  }
  // The new-task dialog edits a new task; the others are edited in the card.
  const inCard = editing && c.op !== 'create_task'
  const Icon = c.op === 'comment' ? MessageSquare : c.op === 'update_page' ? FilePen : c.op === 'create_task' ? Plus : Pencil

  return (
    <li className={cn('flex gap-3 px-4 py-3', off && 'opacity-60')}>
      <div className="mt-0.5 w-4 shrink-0">
        {pending ? (
          <Checkbox checked={row.ticked} onCheckedChange={(v) => void setProposalRow(message.id, index, v === true)} aria-label={tp.apply(summary(c, tp))} />
        ) : row.state === 'done' ? (
          <Check className="size-4 text-tint-green-fg" />
        ) : row.state === 'failed' ? (
          <X className="size-4 text-destructive" />
        ) : (
          <Minus className="size-4 text-muted-foreground" />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <Icon className="size-4 shrink-0 text-muted-foreground" />
          <p className="min-w-0 flex-1 truncate text-sm font-semibold">{summary(c, tp)}</p>
          {pending && !inCard && (
            <Button size="sm" variant="ghost" className="-my-1 h-7" onClick={edit}>
              <Pencil /> {tp.edit}
            </Button>
          )}
        </div>
        {c.op === 'create_task' && <CreateFields c={c} editable={pending} onCategory={(categoryId) => patchChange(index, { ...c, categoryId })} />}
        {c.op === 'update_task' && (inCard && pending ? <UpdateEditor c={c} onDone={done} onCancel={() => setEditing(false)} /> : <UpdateFields c={c} />)}
        {c.op === 'comment' &&
          (inCard && pending ? (
            <CommentEditor c={c} onDone={done} onCancel={() => setEditing(false)} />
          ) : (
            <p className="mt-1.5 whitespace-pre-line rounded-lg border-l-2 bg-muted/40 px-3 py-1.5 text-sm">{c.text}</p>
          ))}
        {c.op === 'update_page' && <PageChange c={c} />}
        <Result row={row} />
      </div>
      {c.op === 'create_task' && (
        <NewTaskDialog
          open={editing}
          onOpenChange={setEditing}
          initial={{
            categoryId: c.categoryId,
            title: c.title,
            description: c.description,
            type: c.taskType,
            priority: c.priority,
            labels: c.labels,
            dueDate: c.due ?? undefined,
          }}
          // Made by hand, so Confirm must not make it again.
          onCreated={() => void setProposalRow(message.id, index, false)}
        />
      )}
    </li>
  )
}

export function ProposalCard({ message }: { message: ChatMessage }) {
  const proposal = message.proposal
  const [busy, setBusy] = useState(false)
  // Terms resolved before the refreshed card arrives (several can go in a row).
  const droppedNow = useRef<string[]>([])
  const [dropped, setDropped] = useState<string[]>([])
  const tp = useT().assistant.proposal
  if (!proposal) return null

  const open = proposal.rows.some((r) => r.state === 'pending')
  const ticked = proposal.rows.filter((r) => r.state === 'pending' && r.ticked).length
  const allRejected = proposal.rows.every((r) => r.state === 'rejected')
  const patchProposal = (p: Partial<Proposal>) => updateMessage(message.id, { proposal: { ...proposal, ...p } })
  const patchChange = (index: number, change: ProposedChange) =>
    void patchProposal({ rows: proposal.rows.map((r, i) => (i === index ? { ...r, change } : r)) })

  const run = (action: (id: string) => Promise<ChatMessage>) => {
    setBusy(true)
    action(message.id)
      .catch((e: { message?: string }) => toast.error(e.message ?? tp.error))
      .finally(() => setBusy(false))
  }

  const termResolved = (term: string) => {
    droppedNow.current = [...droppedNow.current, term]
    setDropped(droppedNow.current)
    void patchProposal({ terms: proposal.terms.filter((t) => !droppedNow.current.includes(t.term)) })
  }
  const terms = proposal.terms.filter((t) => !dropped.includes(t.term))
  const n = proposal.rows.length

  return (
    <div className="mt-3 w-full max-w-2xl overflow-hidden rounded-2xl border bg-card shadow-[0_1px_2px_rgba(16,24,40,0.04)]">
      <div className="flex items-center gap-2.5 border-b bg-tint-blue/50 px-4 py-2.5">
        <TintIcon icon={ListChecks} tint="blue" size="sm" />
        <div className="min-w-0 flex-1">
          <p className="text-sm font-semibold">{tp.title(n)}</p>
          <p className="text-xs text-muted-foreground">{open ? tp.notSaved : allRejected ? tp.rejected : tp.confirmed}</p>
        </div>
      </div>

      {proposal.reply && proposal.reply !== message.text && <p className="border-b px-4 py-2.5 text-sm">{proposal.reply}</p>}

      <ul className="divide-y">
        {proposal.rows.map((row, i) => (
          <RowView key={i} message={message} index={i} row={row} open={open} patchChange={patchChange} />
        ))}
      </ul>

      {/* FR-KN-GL-7: "N possible glossary terms" for the new tasks */}
      {!allRejected && terms.length > 0 && proposal.termsCategoryId && (
        <div className="border-t px-4 py-3">
          <TermSuggestions categoryId={proposal.termsCategoryId} terms={terms} onResolved={termResolved} />
        </div>
      )}

      {open && (
        <div className="flex flex-wrap items-center gap-2 border-t px-4 py-2.5">
          <Button size="sm" disabled={busy || ticked === 0} onClick={() => run(confirmProposal)}>
            {busy ? <Loader2 className="animate-spin" /> : <Check />} {tp.confirm(ticked > 0 && n > 1 ? ticked : null)}
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => run(rejectProposal)}>
            <X /> {tp.reject}
          </Button>
        </div>
      )}
    </div>
  )
}
