// Small building blocks shared by the Tasks screens.
import { format, parseISO } from 'date-fns'
import { CalendarDays, X } from 'lucide-react'
import { useState } from 'react'
import { type Task, useTaskLabels } from '@/api/tasks'
import { Button } from '@/components/ui/button'
import { Calendar } from '@/components/ui/calendar'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { useT } from '@/i18n'
import { closedDueLabel, dateLocale, dueLabel, formatDate } from '@/lib/format'
import { isClosed, TINT_CLASS, tintForTag } from '@/lib/labels'
import { cn } from '@/lib/utils'

/** Updates a task and, when it becomes Done, suggests saving knowledge (FR-TASK-KN-3). */
// Moved to a shared file so Home can use it too.
export { useChangeTask } from '@/components/shared/task-status'

/** Due date text, red when overdue. A closed task's text is fixed when it closed and never highlighted. */
export function DueText({
  task,
  className,
}: {
  task: { dueDate?: string | null; status: Task['status']; completedAt?: string | null }
  className?: string
}) {
  const t = useT()
  if (!task.dueDate) return <span className={cn('text-muted-foreground', className)}>{t.common.noDueDate}</span>
  if (isClosed(task.status))
    return <span className={cn('text-muted-foreground', className)}>{closedDueLabel(task.dueDate, task.completedAt ?? null)}</span>
  const due = dueLabel(task.dueDate)
  const hot = due.tone === 'overdue' || due.tone === 'today'
  return <span className={cn(hot ? 'text-destructive' : 'text-muted-foreground', className)}>{due.text}</span>
}

/** All distinct labels used on tasks, for suggestions. */
/** Every label in use, for suggestions (from Rust). */
export function useAllLabels() {
  return useTaskLabels()
}

/** Date picker button: shadcn Calendar inside a Popover. */
export function DatePicker({
  value,
  onChange,
  className,
  placeholder,
}: {
  value?: string | null
  onChange: (v: string | undefined) => void
  className?: string
  placeholder?: string
}) {
  const [open, setOpen] = useState(false)
  const t = useT()
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" className={cn('justify-start font-normal', !value && 'text-muted-foreground', className)}>
          <CalendarDays />
          {value ? formatDate(value, 'day') : (placeholder ?? t.common.noDueDate)}
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-auto p-0" align="start">
        <Calendar
          mode="single"
          locale={dateLocale()}
          weekStartsOn={1}
          selected={value ? parseISO(value) : undefined}
          onSelect={(d) => {
            onChange(d ? format(d, 'yyyy-MM-dd') : undefined)
            setOpen(false)
          }}
        />
        {value && (
          <div className="border-t p-2">
            <Button
              variant="ghost"
              size="sm"
              className="w-full"
              onClick={() => {
                onChange(undefined)
                setOpen(false)
              }}
            >
              {t.tasks.labels.clearDate}
            </Button>
          </div>
        )}
      </PopoverContent>
    </Popover>
  )
}

/** Chip input for labels with suggestions from existing labels (FR task Labels field). */
export function LabelInput({ value, onChange }: { value: string[]; onChange: (v: string[]) => void }) {
  const all = useAllLabels()
  const [text, setText] = useState('')
  const [focused, setFocused] = useState(false)
  const t = useT()
  const add = (raw: string) => {
    const l = raw.trim()
    if (l && !value.includes(l)) onChange([...value, l])
    setText('')
  }
  const suggestions = all.filter((l) => !value.includes(l) && l.toLowerCase().includes(text.trim().toLowerCase())).slice(0, 8)
  return (
    <div className="space-y-2">
      {value.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {value.map((l) => (
            <span
              key={l}
              className={cn('inline-flex items-center gap-1 rounded-full py-0.5 pr-1.5 pl-2.5 text-xs font-medium', TINT_CLASS[tintForTag(l)])}
            >
              {l}
              <button
                type="button"
                aria-label={t.tasks.labels.remove(l)}
                className="rounded-full opacity-60 hover:opacity-100"
                onClick={() => onChange(value.filter((x) => x !== l))}
              >
                <X className="size-3" />
              </button>
            </span>
          ))}
        </div>
      )}
      <Input
        value={text}
        onFocus={() => setFocused(true)}
        onBlur={() => setTimeout(() => setFocused(false), 150)}
        placeholder={t.tasks.labels.placeholder}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if ((e.key === 'Enter' || e.key === ',') && !e.nativeEvent.isComposing) {
            e.preventDefault()
            add(text)
          }
          if (e.key === 'Backspace' && !text && value.length) onChange(value.slice(0, -1))
        }}
      />
      {focused && suggestions.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-xs text-muted-foreground">{t.tasks.labels.suggestions}</span>
          {suggestions.map((l) => (
            <button key={l} type="button" onClick={() => add(l)} className="rounded-full border px-2 py-0.5 text-xs text-muted-foreground hover:bg-muted">
              + {l}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
