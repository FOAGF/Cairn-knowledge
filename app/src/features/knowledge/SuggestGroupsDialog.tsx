// *Suggest groups* for a category's ungrouped pages (FR-KN-GR-3): the model proposes a group per
// page, the user approves, changes or skips each one. A new group is created with its first approval.
// External mode (FR-KN-EXT-13): nested groups are named by their paths ("Parent / Child"), and a new
// path makes the groups it is missing, each inside the one before.
import { Check, Loader2, Sparkles, Square } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { Link } from 'react-router'
import { errorMessage } from '@/api/client'
import { type GroupProposal, type GroupSuggestions, suggestGroups } from '@/api/creation'
import { useCreateGroup, useGroups, useUpdatePage } from '@/api/knowledge'
import { cancelJob, isCancelled, isLlmUnavailable, newStreamId } from '@/api/llm'
import { ensureGroupPath, groupOptions, groupPath, isGroupPath } from './groupTree'
import { useCategoryList } from '@/api/categories'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'

type Pick = GroupProposal['group']
type Row = { proposal: GroupProposal; pick: Pick; state: 'open' | 'approved' | 'skipped'; changing: boolean }

const NONE = '__none'
const existingKey = (id: string) => `e:${id}`
const newKey = (name: string) => `n:${name}`

export function SuggestGroupsDialog({ categoryId, onClose }: { categoryId: string; onClose: () => void }) {
  const category = useCategoryList().find((p) => p.id === categoryId)
  const groups = useGroups(categoryId)
  const { mutateAsync: createGroup } = useCreateGroup()
  const { mutateAsync: updatePage } = useUpdatePage()
  const [jobId] = useState(newStreamId)
  const [result, setResult] = useState<GroupSuggestions | null>(null)
  const [rows, setRows] = useState<Row[]>([])
  const [error, setError] = useState<{ text: string; unavailable: boolean } | null>(null)
  const [attempt, setAttempt] = useState(0)
  // Groups created by approvals of this run, by lower-case name.
  const created = useRef(new Map<string, string>())
  const t = useT()
  const sg = t.knowledge.suggest

  useEffect(() => {
    let stale = false
    setResult(null)
    setError(null)
    suggestGroups(categoryId, jobId)
      .then((r) => {
        if (stale) return
        setResult(r)
        setRows(r.proposals.map((proposal) => ({ proposal, pick: proposal.group, state: 'open', changing: false })))
      })
      .catch((e) => {
        if (stale || isCancelled(e)) return
        setError({ text: errorMessage(e), unavailable: isLlmUnavailable(e) })
      })
    return () => {
      stale = true
    }
  }, [categoryId, jobId, attempt])

  const running = result === null && error === null
  // Closing while it works is *Stop*.
  const close = () => {
    if (running) void cancelJob(jobId)
    onClose()
  }
  const patch = (i: number, p: Partial<Row>) => setRows((rs) => rs.map((r, j) => (j === i ? { ...r, ...p } : r)))

  const descriptionOf = (name: string) => result?.newGroups.find((g) => g.name.toLowerCase() === name.toLowerCase())?.description ?? ''

  async function approve(i: number) {
    const row = rows[i]
    try {
      let groupId: string
      if (row.pick.kind === 'existing') groupId = row.pick.groupId
      else {
        const key = row.pick.name.toLowerCase()
        const have = created.current.get(key) ?? groups.find((g) => groupPath(g, groups).toLowerCase() === key)?.id
        if (have) groupId = have
        else {
          groupId = isGroupPath(row.pick.name)
            ? await ensureGroupPath(categoryId, row.pick.name, descriptionOf(row.pick.name))
            : (await createGroup({ categoryId, name: row.pick.name, description: descriptionOf(row.pick.name) || null })).id
          created.current.set(key, groupId)
        }
      }
      await updatePage({ id: row.proposal.pageId, patch: { groupId } })
      patch(i, { state: 'approved', changing: false })
    } catch {
      // the error is shown as a toast
    }
  }

  // The choices for "Change": the category's groups, the names proposed in this run, or no group.
  const choices = [
    ...groupOptions(groups).map(({ group: g, label }) => ({ key: existingKey(g.id), label, pick: { kind: 'existing', groupId: g.id, name: label } as Pick })),
    ...(result?.newGroups ?? [])
      .filter((n) => !groups.some((g) => groupPath(g, groups).toLowerCase() === n.name.toLowerCase()))
      .map((n) => ({ key: newKey(n.name), label: sg.newChoice(n.name), pick: { kind: 'new', name: n.name } as Pick })),
  ]
  const keyOf = (p: Pick) => (p.kind === 'existing' ? existingKey(p.groupId) : newKey(p.name))
  const open = rows.filter((r) => r.state === 'open').length

  return (
    <Dialog open onOpenChange={(o) => !o && close()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{sg.title(category?.name)}</DialogTitle>
          <DialogDescription>{sg.description}</DialogDescription>
        </DialogHeader>

        {running && (
          <div className="flex items-center justify-between gap-3 rounded-xl border p-4 text-sm text-muted-foreground">
            <span className="flex items-center gap-2"><Loader2 className="size-4 animate-spin" /> {sg.reading}</span>
            <Button variant="outline" size="sm" onClick={close}><Square /> {t.common.stop}</Button>
          </div>
        )}
        {error && (
          <div className="space-y-2 rounded-xl border p-4 text-sm">
            <p>{error.unavailable ? sg.unavailable(error.text) : error.text}</p>
            {error.unavailable ? (
              <Button variant="outline" size="sm" asChild><Link to="/settings">{sg.openSettings}</Link></Button>
            ) : (
              <Button variant="outline" size="sm" onClick={() => setAttempt((n) => n + 1)}>{t.common.tryAgain}</Button>
            )}
          </div>
        )}
        {result && rows.length === 0 && <p className="rounded-xl border p-4 text-sm text-muted-foreground">{sg.noSuggestions}</p>}
        {rows.length > 0 && (
          <ul className="space-y-2">
            {rows.map((r, i) => (
              <li key={r.proposal.pageId} className="rounded-xl border p-3">
                <p className="truncate text-sm font-medium">{r.proposal.pageTitle}</p>
                {r.state === 'open' && !r.changing && (
                  <p className="mt-0.5 text-sm text-muted-foreground">
                    {r.pick.kind === 'existing'
                      ? sg.group(<span className="font-medium text-foreground">{r.pick.name}</span>)
                      : sg.newGroup(<span className="font-medium text-foreground">{r.pick.name}</span>, descriptionOf(r.pick.name))}
                  </p>
                )}
                {r.state === 'open' && r.changing && (
                  <Select value={keyOf(r.pick)} onValueChange={(v) => v === NONE ? patch(i, { state: 'skipped', changing: false }) : patch(i, { pick: choices.find((c) => c.key === v)?.pick ?? r.pick, changing: false })}>
                    <SelectTrigger className="mt-1.5 w-full" size="sm"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      {choices.map((c) => <SelectItem key={c.key} value={c.key}>{c.label}</SelectItem>)}
                      <SelectItem value={NONE}>{sg.noGroupSkip}</SelectItem>
                    </SelectContent>
                  </Select>
                )}
                {r.state === 'approved' && <p className="mt-0.5 flex items-center gap-1 text-sm text-muted-foreground"><Check className="size-3.5" /> {sg.movedTo(r.pick.name)}</p>}
                {r.state === 'skipped' && <p className="mt-0.5 text-sm text-muted-foreground">{t.common.skipped}</p>}
                {r.state === 'open' && (
                  <div className="mt-2 flex gap-1.5">
                    <Button size="xs" onClick={() => void approve(i)}>{sg.approve}</Button>
                    <Button size="xs" variant="outline" onClick={() => patch(i, { changing: !r.changing })}>{t.common.change}</Button>
                    <Button size="xs" variant="ghost" onClick={() => patch(i, { state: 'skipped', changing: false })}>{sg.skip}</Button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        )}
        <DialogFooter>
          {rows.length > 0 && open === 0 && <span className="mr-auto flex items-center gap-1.5 text-sm text-muted-foreground"><Sparkles className="size-4" /> {sg.allHandled}</span>}
          <Button variant="outline" onClick={close}>{open > 0 ? t.common.close : sg.done}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
