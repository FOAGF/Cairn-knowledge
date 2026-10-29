import { SendHorizontal, Sparkles } from 'lucide-react'
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'

/** "Ask anything" bar. Sends the question to the Assistant page. `large` is Home's (FR-HOME-1). */
export function AskBar({ size = 'default' }: { size?: 'default' | 'large' }) {
  const [q, setQ] = useState('')
  const navigate = useNavigate()
  const t = useT()
  const submit = () => {
    if (!q.trim()) return
    navigate(`/assistant?q=${encodeURIComponent(q.trim())}`)
    setQ('')
  }
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault()
        submit()
      }}
      className={cn(
        'flex items-center gap-3 border bg-card px-4 shadow-[0_1px_3px_rgba(16,24,40,0.05)] focus-within:ring-2 focus-within:ring-ring/40',
        size === 'large' ? 'h-16 rounded-[1.25rem] text-[15px]' : 'h-14 rounded-2xl text-sm',
      )}
    >
      <Sparkles className="size-5 shrink-0 text-primary" />
      <span className="h-6 w-px bg-border" />
      <input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={t.layout.askBar.placeholder}
        className="h-full flex-1 bg-transparent outline-none placeholder:text-muted-foreground"
      />
      <button
        type="submit"
        className="flex size-9 items-center justify-center rounded-full bg-primary text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-40"
        disabled={!q.trim()}
        aria-label={t.layout.askBar.ask}
      >
        <SendHorizontal className="size-4" />
      </button>
    </form>
  )
}
