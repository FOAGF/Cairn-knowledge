// Completion summary for Done tasks only (FR-TASK-SUM-1..4).
import { PartyPopper } from 'lucide-react'
import { useCompletionSummary } from '@/api/tasks'
import { useT } from '@/i18n'

export function CompletionSummary({ taskId }: { taskId: string }) {
  // Rust computes the numbers; null unless the task is done.
  const summary = useCompletionSummary(taskId).data
  const c = useT().tasks.completion
  if (!summary) return null
  const stats = { spent: summary.workingDays, blocked: summary.blockedDays }

  return (
    <section className="rounded-2xl border border-tint-green bg-tint-green/45 p-5">
      <div className="flex items-center gap-2 text-sm font-semibold text-tint-green-fg">
        <PartyPopper className="size-4" /> {c.title}
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3">
        <div className="rounded-xl bg-card/70 p-3">
          <p className="text-2xl font-semibold">{stats.spent}</p>
          <p className="text-xs text-muted-foreground">{c.spent(stats.spent)}</p>
        </div>
        <div className="rounded-xl bg-card/70 p-3">
          <p className="text-2xl font-semibold">{stats.blocked}</p>
          <p className="text-xs text-muted-foreground">{c.blocked(stats.blocked)}</p>
        </div>
      </div>
      <p className="mt-3 text-xs text-muted-foreground">{c.note}</p>
    </section>
  )
}
