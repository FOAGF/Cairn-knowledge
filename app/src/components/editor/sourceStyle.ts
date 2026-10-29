// Styled Markdown source (FR-KN-ED-2, D-62). All markup stays visible and editable: lines are only
// styled (headings larger, code blocks and quotes set off) and task keys tinted. One thing is
// replaced: a page mention `[Title](page:<id>)` shows as a chip with the page's current title, since
// the id means nothing to a reader. The chip is atomic: the cursor steps over it and Backspace
// removes it whole. How the page will look is the Preview's job (MarkdownView, the reading renderer).
import { syntaxTree } from '@codemirror/language'
import { type Range } from '@codemirror/state'
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from '@codemirror/view'
import { getT } from '@/i18n'
import { PAGE_LINK_RE, TASK_MENTION_RE } from '@/lib/mentions'
import { resolvePageTitle } from './pageTitles'

/** `[Title](page:<id>)` shown as a chip with the page's current title. */
class PageChipWidget extends WidgetType {
  id: string
  title: string
  constructor(id: string, title: string) {
    super()
    this.id = id
    this.title = title
  }
  eq(o: PageChipWidget) {
    return o.id === this.id && o.title === this.title
  }
  toDOM(view: EditorView) {
    const s = document.createElement('span')
    s.className = 'cm-page-chip'
    s.textContent = this.title || getT().editor.pageChip
    resolvePageTitle(this.id, (t) => {
      if (t) s.textContent = t
      else s.classList.add('cm-page-missing')
      view.requestMeasure()
    })
    return s
  }
}

const HEADING = /^ATXHeading([1-6])$/
const PAGE_LINK_ANCHORED = new RegExp(`^${PAGE_LINK_RE.source}$`)

function build(view: EditorView): { decorations: DecorationSet; chips: DecorationSet } {
  const out: Range<Decoration>[] = []
  const chips: Range<Decoration>[] = []
  const { state } = view
  const lines = (from: number, to: number, cls: string) => {
    const a = state.doc.lineAt(from).number
    const b = state.doc.lineAt(to).number
    for (let n = a; n <= b; n++) out.push(Decoration.line({ class: cls }).range(state.doc.line(n).from))
  }
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const h = HEADING.exec(node.name)
        if (h) out.push(Decoration.line({ class: `cm-src-h cm-src-h${h[1]}` }).range(state.doc.lineAt(node.from).from)) // i18n-ignore: CSS class
        else if (node.name === 'FencedCode') lines(node.from, node.to, 'cm-src-code')
        else if (node.name === 'Blockquote') lines(node.from, node.to, 'cm-src-quote')
        else if (node.name === 'Link') {
          const m = PAGE_LINK_ANCHORED.exec(state.doc.sliceString(node.from, node.to))
          if (m) {
            chips.push(Decoration.replace({ widget: new PageChipWidget(m[2], m[1].replace(/\\(.)/g, '$1')) }).range(node.from, node.to))
            return false
          }
        }
      },
    })
    // `#KEY` task mentions are tinted; the text stays as written.
    for (let pos = from; pos <= to; ) {
      const line = state.doc.lineAt(pos)
      const re = new RegExp(TASK_MENTION_RE.source, 'g')
      for (let m = re.exec(line.text); m; m = re.exec(line.text)) {
        const start = line.from + m.index + m[1].length
        out.push(Decoration.mark({ class: 'cm-src-task' }).range(start, start + m[2].length + 1))
      }
      pos = line.to + 1
    }
  }
  const chipSet = Decoration.set(chips, true)
  return { decorations: Decoration.set([...out, ...chips], true), chips: chipSet }
}

const plugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet
    chips: DecorationSet
    constructor(view: EditorView) {
      const b = build(view)
      this.decorations = b.decorations
      this.chips = b.chips
    }
    update(u: ViewUpdate) {
      if (!u.docChanged && !u.viewportChanged) return
      const b = build(u.view)
      this.decorations = b.decorations
      this.chips = b.chips
    }
  },
  { decorations: (v) => v.decorations },
)

const theme = EditorView.theme({
  '.cm-src-h': { lineHeight: '1.4' },
  '.cm-src-h1': { fontSize: '1.45em', paddingTop: '0.3em' },
  '.cm-src-h2': { fontSize: '1.25em', paddingTop: '0.25em' },
  '.cm-src-h3': { fontSize: '1.12em', paddingTop: '0.15em' },
  '.cm-src-h4, .cm-src-h5, .cm-src-h6': { fontSize: '1.03em' },
  '.cm-src-code': {
    fontFamily: 'ui-monospace, monospace', // i18n-ignore: CSS
    fontSize: '0.88em',
    backgroundColor: 'var(--muted)',
    paddingLeft: '0.75rem !important',
  },
  '.cm-src-quote': { borderLeft: '3px solid var(--border)', paddingLeft: '0.75rem !important' }, // i18n-ignore: CSS
  '.cm-page-chip': {
    backgroundColor: 'var(--tint-purple)',
    color: 'var(--tint-purple-fg)',
    borderRadius: '0.375rem',
    padding: '0 0.375rem',
    fontWeight: '500',
    fontSize: '0.92em',
  },
  '.cm-page-missing': { backgroundColor: 'transparent', color: 'var(--muted-foreground)', border: '1px dashed var(--border)' }, // i18n-ignore: CSS
  '.cm-src-task': {
    backgroundColor: 'var(--accent)',
    color: 'var(--accent-foreground)',
    borderRadius: '0.375rem',
    padding: '0 0.25rem',
    fontWeight: '500',
  },
})

/** The editor extension: styling, page chips, and the chips as atomic ranges. */
export const sourceStyle = [plugin, theme, EditorView.atomicRanges.of((view) => view.plugin(plugin)?.chips ?? Decoration.none)]
