import { X } from 'lucide-react'
import { useId, useState } from 'react'
import { useT } from '@/i18n'
import { TINT_CLASS, tintForTag } from '@/lib/labels'
import { cn } from '@/lib/utils'

/** Chip input for tags with autocomplete from existing tags (§5.1). */
export function TagInput({ tags, suggestions, onChange, placeholder }: { tags: string[]; suggestions: string[]; onChange: (tags: string[]) => void; placeholder?: string }) {
  const [text, setText] = useState('')
  const k = useT().knowledge.tagInput
  const listId = useId()
  const add = (raw: string) => {
    const t = raw.trim().replace(/^#/, '')
    if (t && !tags.includes(t)) onChange([...tags, t])
    setText('')
  }
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {tags.map((t) => (
        <span key={t} className={cn('inline-flex items-center gap-1 rounded-full py-0.5 pr-1 pl-2.5 text-xs font-medium whitespace-nowrap', TINT_CLASS[tintForTag(t)])}>
          {t}
          <button className="rounded-full p-0.5 opacity-60 hover:opacity-100" aria-label={k.remove(t)} onClick={() => onChange(tags.filter((x) => x !== t))}>
            <X className="size-3" />
          </button>
        </span>
      ))}
      <input
        list={listId}
        value={text}
        placeholder={tags.length ? k.more : (placeholder ?? k.placeholder)}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if ((e.key === 'Enter' || e.key === ',') && !e.nativeEvent.isComposing) {
            e.preventDefault()
            add(text)
          } else if (e.key === 'Backspace' && !text && tags.length) onChange(tags.slice(0, -1))
        }}
        onBlur={() => add(text)}
        className="min-w-16 flex-1 bg-transparent text-xs outline-none placeholder:text-muted-foreground"
      />
      <datalist id={listId}>
        {suggestions.filter((s) => !tags.includes(s)).map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
    </div>
  )
}
