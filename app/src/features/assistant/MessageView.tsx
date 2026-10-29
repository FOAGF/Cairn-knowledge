import { ChevronRight, CircleCheck, CircleX, FileText, Globe, Loader2, Minus, RotateCcw, Sparkles, Square, TriangleAlert } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { clearStream, cancelJob, useLlmStream } from '@/api/llm'
import { type ChatMessage, type ChatStep, useDraftAsIs, useRetryChatJob } from '@/api/conversations'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { type Messages, useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { MentionText } from './MentionText'
import { AnswerView } from './AnswerView'
import { DraftCard } from './DraftCard'
import { FileChip } from './FileChip'
import { FindList } from './FindList'
import { isRunning } from './messages'
import { ProposalCard } from './ProposalCard'

/**
 * Processing steps from Rust (§5.3, FR-KN-UP-4b): "Reading page 3 of 12 with …", "Drafting the page with …".
 * Once the message is finished and every step went well, they fold into one quiet line that opens the list.
 */
function Steps({ steps, finished }: { steps: ChatStep[]; finished: boolean }) {
  const [open, setOpen] = useState(false)
  const quiet = finished && steps.every((x) => x.state === 'done' && !x.label.startsWith('Warning:'))
  if (quiet && !open) {
    return (
      <button type="button" onClick={() => setOpen(true)} className="mb-1.5 flex max-w-full items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground">
        <CircleCheck className="size-3.5 shrink-0 text-tint-green-fg" />
        <span className="truncate">{steps.map((x) => x.label).join(' · ')}</span>
        <ChevronRight className="size-3.5 shrink-0" />
      </button>
    )
  }
  return (
    <ol className="mb-2 space-y-1.5">
      {steps.map((x, i) => {
        const warning = x.label.startsWith('Warning:')
        return (
          <li key={`${i}-${x.label}`} className={cn('flex items-center gap-2 text-sm', x.state === 'pending' || x.state === 'skipped' ? 'text-muted-foreground' : '', x.state === 'failed' && 'text-destructive', warning && 'text-tint-amber-fg')}>
            {warning ? (
              <TriangleAlert className="size-4 shrink-0" />
            ) : x.state === 'done' ? (
              <CircleCheck className="size-4 shrink-0 text-tint-green-fg" />
            ) : x.state === 'active' ? (
              <Loader2 className="size-4 shrink-0 animate-spin text-primary" />
            ) : x.state === 'failed' ? (
              <CircleX className="size-4 shrink-0" />
            ) : x.state === 'skipped' ? (
              <Minus className="size-4 shrink-0" />
            ) : (
              <span className="size-4 shrink-0 rounded-full border" />
            )}
            {x.label}
          </li>
        )
      })}
    </ol>
  )
}

/** The draft as the model writes it (FR-KN-UP-5, FR-LLM-AS-7), with *Stop* (FR-KN-UP-4b). Cleared once the card has arrived. */
function RunningJob({ message }: { message: ChatMessage }) {
  const streamed = useLlmStream(message.id)
  const t = useT()
  return (
    <div>
      {streamed && (
        <div className="mb-2 max-h-72 overflow-y-auto rounded-xl border bg-muted/30 px-3 py-2">
          <MarkdownView text={streamed} className="text-sm" />
        </div>
      )}
      <Button size="sm" variant="outline" onClick={() => void cancelJob(message.id)}>
        <Square /> {t.common.stop}
      </Button>
    </div>
  )
}

/** A failed or stopped job: what happened, and what the user can do next (FR-KN-UP-10). */
function JobError({ message }: { message: ChatMessage }) {
  const error = message.error!
  const retry = useRetryChatJob()
  const asIs = useDraftAsIs()
  const t = useT()
  const stopped = error.kind === 'cancelled'
  return (
    <div className={cn('mt-1 rounded-xl border p-3 text-sm', stopped ? 'bg-muted/30' : 'border-destructive/40 bg-destructive/5')}>
      <p className={cn('flex items-start gap-2', !stopped && 'text-destructive')}>
        {stopped ? <Square className="mt-0.5 size-4 shrink-0" /> : <TriangleAlert className="mt-0.5 size-4 shrink-0" />}
        <span className="min-w-0 break-words">{error.message}</span>
      </p>
      {error.kind === 'llm_unavailable' && (
        <p className="mt-1 ml-6 text-xs text-muted-foreground">
          {t.assistant.message.setUpModel((label) => <Link to="/settings/providers" className="underline underline-offset-2">{label}</Link>)}
        </p>
      )}
      <div className="mt-2.5 flex flex-wrap gap-2">
        <Button size="sm" variant="outline" disabled={retry.isPending} onClick={() => retry.mutate({ messageId: message.id, modelId: message.modelId })}>
          <RotateCcw /> {t.assistant.message.retry}
        </Button>
        {error.canUseAsIs && (
          <Button size="sm" variant="outline" disabled={asIs.isPending} onClick={() => asIs.mutate(message.id)}>
            <FileText /> {t.assistant.message.useAsIs}
          </Button>
        )}
      </div>
    </div>
  )
}

/**
 * D-65: the effort level the reply's calls sent: the picked one, or the model's default (marked).
 * `sentEffort` is "" when none was sent; it is null while the job runs and on messages from before
 * 0.10.1, which only know the picked level.
 */
function effortNote(m: ChatMessage, t: Messages): string {
  const sent = m.sentEffort ?? m.effort
  if (!sent) return ''
  return t.assistant.message.effortNote(sent, !m.effort)
}

export function MessageView({ message, web }: { message: ChatMessage; web?: boolean }) {
  const running = isRunning(message)
  const t = useT()
  // The streamed text belongs to the job; forget it once the job has ended (card, answer or error).
  useEffect(() => {
    if (!running) clearStream(message.id)
  }, [running, message.id])
  if (message.role === 'user') {
    return (
      <div className="flex justify-end">
        <div className="flex max-w-[75%] flex-col items-end gap-1.5">
          {message.attachment && <FileChip name={message.attachment.name} size={message.attachment.size} kind={message.attachment.kind} />}
          {message.text && (
            <div className="rounded-2xl rounded-br-md bg-tint-blue px-4 py-2.5 text-tint-blue-fg">
              <p className="whitespace-pre-wrap break-words text-sm">
                <MentionText text={message.text} />
              </p>
              {web && (
                <p className="mt-1 flex items-center gap-1 text-xs opacity-80">
                  <Globe className="size-3" /> {t.assistant.message.webOnly}
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    )
  }
  return (
    <div className="flex items-start gap-3">
      <TintIcon icon={Sparkles} tint="purple" size="sm" className="mt-0.5 rounded-full" />
      <div className="min-w-0 flex-1">
        {message.steps.length > 0 && <Steps steps={message.steps} finished={!running && !message.error} />}
        {running && <RunningJob message={message} />}
        {message.error && <JobError message={message} />}
        {message.answer ? <AnswerView message={message} /> : message.find ? <FindList find={message.find} /> : message.text && <MarkdownView text={message.text} className="text-sm" />}
        {message.draft && <DraftCard message={message} />}
        {message.proposal && <ProposalCard message={message} />}
        {message.modelLabel && (
          <p className="mt-2 text-xs text-muted-foreground">
            {message.modelLabel}
            {effortNote(message, t)}
            {message.reasoningTokens ? t.assistant.message.thoughtFor(message.reasoningTokens.toLocaleString()) : ''}
          </p>
        )}
      </div>
    </div>
  )
}

/** A file on its way to Rust (`fraction` 0..1). */
export function UploadingIndicator({ fraction }: { fraction: number }) {
  const t = useT()
  return (
    <div className="flex items-center gap-3">
      <TintIcon icon={Sparkles} tint="purple" size="sm" className="rounded-full" />
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" /> {t.assistant.message.sending(Math.round(fraction * 100))}
      </p>
    </div>
  )
}
