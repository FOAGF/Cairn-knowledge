// References in the Assistant's box (FR-LLM-AS-14, D-81): `#` suggests tasks and `[[` suggests pages,
// from the page editor's own lookups (DM-MN-3). A picked page goes into the box as `[[Title]]`, which
// reads well in plain text; sending turns it into the stored `[Title](page:<id>)`, so the message
// shows it as a chip and Rust sends that page to the model.
import { type RefObject, useEffect, useRef, useState } from 'react'
import { suggestPages, suggestTasks } from '@/components/editor/mentionComplete'
import { pageLink } from '@/lib/mentions'
import { type Suggestion, SuggestionList } from './SuggestionList'

type Trigger = { kind: 'task' | 'page'; from: number; query: string }

/** The trigger right before the caret: `#query` (not inside a word) or `[[query`. */
export function triggerAt(text: string, caret: number): Trigger | null {
  const before = text.slice(0, caret)
  const page = /\[\[([^\]\n]*)$/.exec(before)
  if (page) return { kind: 'page', from: page.index, query: page[1] }
  const task = /(^|[^\w&#])#([A-Za-z0-9-]*)$/.exec(before)
  if (task) return { kind: 'task', from: task.index + task[1].length, query: task[2] }
  return null
}

/**
 * The suggestions for the composer's text box. `onKeyDown` returns true when it used the key (the
 * list is open: arrows move, Enter or Tab picks, Escape closes). `stored` gives the text to send.
 */
export function useReferences(text: string, setText: (t: string) => void, area: RefObject<HTMLTextAreaElement | null>) {
  const [trigger, setTrigger] = useState<Trigger | null>(null)
  const [items, setItems] = useState<Suggestion[] | null>(null)
  const [active, setActive] = useState(0)
  // The pages picked into this message, by the title written in the box.
  const picked = useRef(new Map<string, string>())

  const kind = trigger?.kind
  const query = trigger?.query
  useEffect(() => {
    if (!kind || query === undefined) return
    let stale = false
    const timer = setTimeout(
      () => {
        const found: Promise<Suggestion[]> =
          kind === 'task'
            ? suggestTasks(query).then((ts) => ts.map((t) => ({ kind: 'task' as const, ...t })))
            : suggestPages(query).then((ps) => ps.map((p) => ({ kind: 'page' as const, ...p })))
        void found.then((list) => {
          if (stale) return
          setItems(list)
          setActive(0)
        })
      },
      kind === 'page' ? 150 : 0,
    )
    return () => {
      stale = true
      clearTimeout(timer)
    }
  }, [kind, query])

  /** Call after the text or the caret changed. */
  const follow = (value: string, caret: number) => {
    const next = triggerAt(value, caret)
    setTrigger(next)
    if (!next) setItems(null)
  }
  const close = () => {
    setTrigger(null)
    setItems(null)
  }
  const pick = (s: Suggestion) => {
    const el = area.current
    if (!trigger || !el) return
    const caret = el.selectionStart
    // Swallow a "]]" already after the caret, so no stray brackets are left.
    const end = s.kind === 'page' && text.slice(caret, caret + 2) === ']]' ? caret + 2 : caret
    const insert = s.kind === 'task' ? `#${s.key} ` : `[[${s.title}]] `
    if (s.kind === 'page') picked.current.set(s.title, s.id)
    const next = text.slice(0, trigger.from) + insert + text.slice(end)
    setText(next)
    close()
    const at = trigger.from + insert.length
    requestAnimationFrame(() => {
      el.focus()
      el.setSelectionRange(at, at)
    })
  }
  const open = !!trigger && !!items
  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!open || e.nativeEvent.isComposing) return false
    if (e.key === 'Escape') {
      close()
    } else if (!items.length) {
      return false
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      setActive((i) => (i + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length)
    } else if ((e.key === 'Enter' && !e.shiftKey) || (e.key === 'Tab' && !e.shiftKey)) {
      pick(items[Math.min(active, items.length - 1)])
    } else {
      return false
    }
    e.preventDefault()
    return true
  }
  /** The text to send: each picked `[[Title]]` as its page link. Forgets the picks. */
  const stored = (value: string) => {
    let out = value
    for (const [title, id] of picked.current) out = out.split(`[[${title}]]`).join(pageLink(title, id))
    picked.current.clear()
    return out
  }
  const list = open ? <SuggestionList kind={trigger.kind} items={items} active={active} onPick={pick} onHover={setActive} /> : null
  return { follow, onKeyDown, stored, list, close }
}
