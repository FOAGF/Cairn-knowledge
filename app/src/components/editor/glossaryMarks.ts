// Glossary terms in the editor (FR-KN-GL-6, D-38: first occurrence only): a dotted underline and a hover tooltip. It is a plain mark decoration,
// separate from the spelling/grammar underlines of harperLint.ts (those are wavy and come from @codemirror/lint).
import { syntaxTree } from '@codemirror/language'
import { type Range, StateEffect } from '@codemirror/state'
import { Decoration, type DecorationSet, EditorView, hoverTooltip, ViewPlugin, type ViewUpdate } from '@codemirror/view'
import type { GlossaryEntry } from '@/api/glossary'
import { getT } from '@/i18n'
import { firstPerEntry, type TermMatch, type TermMatcher } from '@/lib/glossary'

export interface GlossaryMarkHooks {
  /** Matcher for the page's category, or null when the glossary is off. Read on every update. */
  matcher: () => TermMatcher<GlossaryEntry> | null
  /** Scope name for the tooltip ("Global" or the category name). */
  scopeName: (entry: GlossaryEntry) => string
}

// Markdown parts where terms are not marked: code, links, images, raw HTML.
const SKIP = new Set(['InlineCode', 'FencedCode', 'CodeBlock', 'Link', 'Image', 'URL', 'Autolink', 'HTMLTag', 'HTMLBlock']) // i18n-ignore: Lezer node names

function inSkippedNode(view: EditorView, pos: number): boolean {
  for (let n: { name: string; parent: unknown } | null = syntaxTree(view.state).resolveInner(pos, 1); n; n = n.parent as typeof n)
    if (SKIP.has(n.name)) return true
  return false
}

function matchesIn(view: EditorView, hooks: GlossaryMarkHooks): TermMatch<GlossaryEntry>[] {
  const matcher = hooks.matcher()
  if (!matcher) return []
  // Skipped parts (code, links) don't count as the first occurrence.
  return firstPerEntry(matcher.find(view.state.doc.toString()).filter((m) => !inSkippedNode(view, m.start)))
}

const dotted = Decoration.mark({ class: 'cm-glossary-term' })

/** Finding the terms reads the whole text, too slow for every key of a long page: while typing, the
 *  marks move with the text, and they are found again once typing pauses for this long. */
const REBUILD_DELAY_MS = 300
const rebuild = StateEffect.define<null>()

export function glossaryMarks(hooks: GlossaryMarkHooks) {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      timer: ReturnType<typeof setTimeout> | undefined
      view: EditorView
      constructor(view: EditorView) {
        this.view = view
        this.decorations = this.build(view)
      }
      update(u: ViewUpdate) {
        if (u.transactions.some((tr) => tr.effects.some((e) => e.is(rebuild)))) {
          this.decorations = this.build(u.view)
          return
        }
        if (u.docChanged) this.decorations = this.decorations.map(u.changes)
        if (u.docChanged || u.viewportChanged || syntaxTree(u.startState) !== syntaxTree(u.state)) {
          clearTimeout(this.timer)
          this.timer = setTimeout(() => this.view.dispatch({ effects: rebuild.of(null) }), REBUILD_DELAY_MS)
        }
      }
      destroy() {
        clearTimeout(this.timer)
      }
      build(view: EditorView) {
        const ranges: Range<Decoration>[] = matchesIn(view, hooks).map((m) => dotted.range(m.start, m.end))
        return Decoration.set(ranges, true)
      }
    },
    { decorations: (v) => v.decorations },
  )

  const tooltip = hoverTooltip((view, pos) => {
    const hit = matchesIn(view, hooks).find((m) => pos >= m.start && pos <= m.end)
    if (!hit) return null
    return {
      pos: hit.start,
      end: hit.end,
      above: true,
      create() {
        const dom = document.createElement('div')
        dom.className = 'cm-glossary-card'
        const add = (cls: string, text: string) => {
          const p = dom.appendChild(document.createElement('div'))
          p.className = cls
          p.textContent = text
        }
        add('cm-glossary-title', `${hit.entry.term} · ${hooks.scopeName(hit.entry)}`)
        if (hit.entry.otherNames.length) add('cm-glossary-names', getT().editor.glossary.otherNames(hit.entry.otherNames))
        add('cm-glossary-desc', hit.entry.description)
        return { dom }
      },
    }
  })

  const theme = EditorView.baseTheme({
    '.cm-glossary-term': { textDecoration: 'underline dotted', textDecorationColor: 'var(--muted-foreground)', textUnderlineOffset: '4px' }, // i18n-ignore: CSS
    '.cm-glossary-card': { padding: '0.5rem 0.75rem', maxWidth: '22rem', fontSize: '0.85rem', lineHeight: '1.4' },
    '.cm-glossary-title': { fontWeight: '600' },
    '.cm-glossary-names': { color: 'var(--muted-foreground)', fontSize: '0.75rem' },
    '.cm-glossary-desc': { marginTop: '0.25rem', whiteSpace: 'pre-line' },
  })

  return [plugin, tooltip, theme]
}
