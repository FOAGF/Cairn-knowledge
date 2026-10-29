// The "Draft page" card in an assistant message (§5.3 – §5.5). Nothing is saved from here:
// "Review" hands the draft to the normal save pipeline. The card is stored with its
// message in Rust (`update_message`), so it survives a restart.
import { Check, ChevronDown, FileText, FolderKanban, Tag, Trash2, TriangleAlert } from 'lucide-react'
import { useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { commands } from '@/api/bindings'
import { type ChatMessage, type DraftCard as Card, reviseDraft, updateMessage } from '@/api/conversations'
import { useGroups } from '@/api/knowledge'
import { useCategoryList } from '@/api/categories'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { countWords } from '@/lib/format'
import { TINT_CLASS } from '@/lib/labels'
import { cn } from '@/lib/utils'
import { REVIEW_PATH } from '@/store/creation'
import { openAsPending } from '../creation/pending'
import { TermSuggestions } from '../glossary/TermSuggestions'
import { isActiveCard } from './messages'
import { groupPath } from '../knowledge/groupTree'

export function DraftCard({ message }: { message: ChatMessage }) {
  const draft = message.draft!
  const navigate = useNavigate()
  const categories = useCategoryList()
  const groups = useGroups()
  const [open, setOpen] = useState(false)
  // `t` is a topic, tag or term below.
  const m = useT()
  const d = m.assistant.draft

  const category = categories.find((p) => p.id === draft.categoryId)
  const group = groups.find((g) => g.id === draft.groupId)
  const words = countWords(draft.body)
  const old = draft.superseded
  const active = isActiveCard(draft) && draft.state === 'draft'
  const patch = (c: Partial<Card>) => updateMessage(message.id, { draft: { ...draft, ...c } })

  // The category changes where the page goes, so the group suggested for the old category no longer
  // fits: ask the check again for the new one (the default model; no model, no suggestion).
  const changeCategory = (categoryId: string) => {
    void patch({ categoryId, groupId: null, groupName: null }).then(() =>
      commands
        .checkPage({ categoryId, title: draft.title, body: draft.body, shownTerms: draft.shownTerms, jobId: null })
        .then((r) =>
          updateMessage(message.id, {
            draft: {
              ...draft,
              categoryId,
              groupId: r.group?.kind === 'existing' ? r.group.groupId : null,
              groupName: r.group?.kind === 'new' ? r.group.name : null,
              tags: r.tags.length ? r.tags : draft.tags,
            },
          }),
        )
        .catch(() => undefined),
    )
  }

  // FR-KN-SV-*: the review pipeline does the topic check, group suggestion, duplicate check and merge.
  // The card's tags and new group are suggestions: the review offers the tags, and shows the group as "(new)".
  const review = async () => {
    const started = await openAsPending(
      {
        source: 'file',
        fileName: draft.fileName ?? undefined,
        categoryId: draft.categoryId,
        groupId: draft.groupId ?? undefined,
        newGroup: draft.groupId ? undefined : (draft.groupName ?? undefined),
        title: draft.title,
        body: draft.body,
        tags: [],
        suggestedTags: draft.tags,
        sourceInfo: draft.sourceInfo ?? undefined,
        originConversation: { conversationId: message.conversationId, messageId: message.id },
        shownTerms: draft.shownTerms,
        incomplete: draft.incomplete,
      },
      REVIEW_PATH,
      navigate,
    )
    // A saved card keeps its "Saved as" link; reviewing it again may save another page.
    if (started && draft.state !== 'saved') void patch({ state: 'in_review', discarded: false })
  }

  // FR-KN-DV-3 from the card: a revision request, so the conversation's model rewrites the draft.
  const keepOnly = (topic: string) => {
    // The level picked for the draft (D-65); a model default stays the default.
    // The button's own text, sent as the user's message: the model answers in its language.
    void reviseDraft({ conversationId: message.conversationId, text: d.keepOnly(topic), modelId: message.modelId, effort: message.effort })
  }

  // A term added or ignored leaves the card (FR-KN-GL-7). Several can be resolved before the
  // refreshed card arrives, so the ones already dropped are remembered here.
  const droppedNow = useRef<string[]>([])
  const [dropped, setDropped] = useState<string[]>([])
  const termResolved = (term: string) => {
    droppedNow.current = [...droppedNow.current, term]
    setDropped(droppedNow.current)
    void patch({ terms: draft.terms.filter((t) => !droppedNow.current.includes(t.term)) })
  }
  const terms = draft.terms.filter((t) => !dropped.includes(t.term))

  return (
    <div className={cn('mt-3 w-full max-w-2xl overflow-hidden rounded-2xl border bg-card shadow-[0_1px_2px_rgba(16,24,40,0.04)]', (old || draft.discarded) && 'opacity-60')}>
      <div className="flex items-center gap-2.5 border-b bg-tint-purple/50 px-4 py-2.5">
        <TintIcon icon={FileText} tint="purple" size="sm" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium text-muted-foreground">{d.title(draft.asIs)}</p>
          <p className="truncate text-sm font-semibold">{draft.title || d.untitled}</p>
        </div>
        {old && <span className="rounded-full bg-muted px-2.5 py-0.5 text-xs font-medium text-muted-foreground">{d.earlier}</span>}
        {draft.discarded && <span className="rounded-full bg-muted px-2.5 py-0.5 text-xs font-medium text-muted-foreground">{d.discarded}</span>}
      </div>

      <div className="space-y-3 px-4 py-3">
        {/* The model stopped at its token limit: saving half a page by mistake is easy. */}
        {active && draft.incomplete && (
          <p className="flex items-start gap-2 rounded-xl border border-tint-amber bg-tint-amber/50 p-3 text-sm text-tint-amber-fg">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" />
            <span>{d.incomplete}</span>
          </p>
        )}

        {/* FR-KN-DOC-2: several topics */}
        {active && draft.topics.length > 0 && (
          <div className="rounded-xl border border-tint-amber bg-tint-amber/50 p-3">
            <p className="flex items-start gap-2 text-sm text-tint-amber-fg">
              <TriangleAlert className="mt-0.5 size-4 shrink-0" />
              <span>{d.topics(draft.topics)}</span>
            </p>
            <div className="mt-2.5 flex flex-wrap gap-2">
              {draft.topics.map((t) => (
                <Button key={t} size="sm" variant="outline" className="h-auto max-w-full rounded-full bg-card py-1 whitespace-normal" onClick={() => keepOnly(t)}>
                  {d.keepOnly(t)}
                </Button>
              ))}
            </div>
          </div>
        )}

        <dl className="grid grid-cols-[5.5rem_1fr] items-center gap-x-3 gap-y-2 text-sm">
          <dt className="text-muted-foreground">{m.common.category}</dt>
          <dd className="flex items-center gap-2">
            {active ? (
              <Select value={draft.categoryId} onValueChange={changeCategory}>
                <SelectTrigger size="sm" className="w-auto gap-2 rounded-full" aria-label={m.assistant.changeCategory}>
                  <FolderKanban className="size-3.5 text-muted-foreground" />
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {categories.filter((p) => p.status === 'active').map((p) => (
                    <SelectItem key={p.id} value={p.id}>{p.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <span>{category?.name}</span>
            )}
          </dd>
          <dt className="text-muted-foreground">{m.common.group}</dt>
          <dd>
            {group || draft.groupName ? (
              <span className={cn('inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium', TINT_CLASS.slate)}>
                {group ? groupPath(group, groups) : draft.groupName}
                {!group && <span className="font-normal opacity-70">{d.newGroup}</span>}
              </span>
            ) : (
              <span className="text-muted-foreground">{d.noSuggestion}</span>
            )}
          </dd>
          <dt className="text-muted-foreground">{d.suggestedTags}</dt>
          <dd className="flex flex-wrap gap-1.5">
            {draft.tags.length ? (
              draft.tags.map((t) => (
                <span key={t} className={cn('inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-xs font-medium', TINT_CLASS.blue)}>
                  <Tag className="size-3" />{t}
                </span>
              ))
            ) : (
              <span className="text-muted-foreground">{m.common.none}</span>
            )}
          </dd>
          <dt className="text-muted-foreground">{d.sourceInfo}</dt>
          <dd className="min-w-0 truncate" title={draft.sourceInfo ?? undefined}>{draft.sourceInfo ?? draft.fileName ?? '—'}</dd>
        </dl>

        <div>
          <div className={cn('relative overflow-hidden rounded-xl border bg-muted/30 px-3 py-2', !open && 'max-h-36')}>
            <MarkdownView text={draft.body} className="text-sm" />
            {!open && <div className="pointer-events-none absolute inset-x-0 bottom-0 h-10 bg-gradient-to-t from-card to-transparent" />}
          </div>
          <div className="mt-1.5 flex items-center justify-between text-xs text-muted-foreground">
            <button type="button" onClick={() => setOpen(!open)} className="inline-flex items-center gap-1 font-medium text-primary hover:underline">
              <ChevronDown className={cn('size-3.5 transition-transform', open && 'rotate-180')} />
              {open ? m.common.showLess : d.showFull}
            </button>
            <span>{d.words(words)}</span>
          </div>
        </div>

        {/* FR-KN-GL-7: "N possible glossary terms", collapsed */}
        {active && terms.length > 0 && <TermSuggestions categoryId={draft.categoryId} terms={terms} onResolved={termResolved} />}
      </div>

      <div className="flex flex-wrap items-center gap-2 border-t px-4 py-2.5">
        {active && (
          <>
            <Button size="sm" onClick={review}><Check /> {m.common.review}</Button>
            <Button size="sm" variant="ghost" onClick={() => void patch({ discarded: true })}><Trash2 /> {m.common.discard}</Button>
            <span className="ml-auto text-xs text-muted-foreground">{d.notSaved}</span>
          </>
        )}
        {draft.state === 'in_review' && !old && !draft.discarded && (
          <>
            <p className="text-sm text-muted-foreground">{d.inReview}</p>
            <Button size="sm" variant="outline" className="ml-auto" onClick={review}>{d.reviewAgain}</Button>
          </>
        )}
        {draft.state === 'saved' && (
          <>
            <p className="flex items-center gap-1.5 text-sm font-medium text-tint-green-fg">
              <Check className="size-4" />
              {d.savedAs(draft.savedPageId ? <Link to={`/knowledge/${draft.savedPageId}`} className="underline">{draft.title}</Link> : draft.title)}
            </p>
            <Button size="sm" variant="outline" className="ml-auto" onClick={review}>{d.reviewAgain}</Button>
          </>
        )}
        {old && <p className="text-sm text-muted-foreground">{d.replaced}</p>}
        {draft.discarded && !old && draft.state !== 'saved' && (
          <>
            <p className="text-sm text-muted-foreground">{d.discardedNote}</p>
            <Button size="sm" variant="outline" className="ml-auto" onClick={review}>{d.reviewAgain}</Button>
          </>
        )}
      </div>
    </div>
  )
}
