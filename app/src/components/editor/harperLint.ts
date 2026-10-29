// Spelling and grammar in the editor (FR-KN-ED-6..8): Harper runs in Rust (`lintText`); this is a
// `@codemirror/lint` source around it. Spelling is a red wavy underline, grammar an amber one.
import { type Action, type Diagnostic, forEachDiagnostic, forceLinting, linter, setDiagnostics } from '@codemirror/lint'
import { type EditorView } from '@codemirror/view'
import { lintText } from '@/api/search'
import { getT } from '@/i18n'

export interface LintSettings {
  spelling: boolean
  grammar: boolean
}

export interface LintHooks {
  /** Current toggles; read on every run so they can change without rebuilding the editor. */
  options: () => LintSettings
  /** Adds a word to the personal dictionary; call `done` once it is saved so the editor re-lints. */
  addWord: (word: string, done: () => void) => void
}

/** Debounce between the last keystroke and the lint run. */
const LINT_DELAY_MS = 500

let warned = false

/** Each open editor's check, to run again on demand (`relint`). */
const checks = new WeakMap<EditorView, () => Promise<void>>()

/** Drops the spelling underlines of `word` at once, before the check after saving it confirms. */
function hideWord(view: EditorView, word: string) {
  const lower = word.toLowerCase()
  const keep: Diagnostic[] = []
  forEachDiagnostic(view.state, (d, from, to) => {
    if (d.markClass !== 'cm-lint-spelling' || view.state.sliceDoc(from, to).toLowerCase() !== lower) keep.push({ ...d, from, to })
  })
  view.dispatch(setDiagnostics(view.state, keep))
}

export function harperLint(hooks: LintHooks) {
  const recheck = async (view: EditorView) => {
    const doc = view.state.doc
    const found = await source(view)
    if (view.state.doc === doc) view.dispatch(setDiagnostics(view.state, found))
  }
  const source = async (view: EditorView): Promise<Diagnostic[]> => {
    checks.set(view, () => recheck(view))
    const options = hooks.options()
    if (!options.spelling && !options.grammar) return []
    const doc = view.state.doc
    const text = doc.toString()
    if (!text.trim()) return []
    let hits
    try {
      hits = await lintText(text, { spelling: options.spelling, grammar: options.grammar })
    } catch (e) {
      // The editor stays usable without checking; say so once, no toast.
      if (!warned) {
        warned = true
        console.warn('Spelling and grammar check unavailable:', e)
      }
      return []
    }
    if (view.state.doc !== doc) return [] // edited meanwhile: the next run covers it
    // Harper's own messages stay in English; the labels around them follow the interface.
    const t = getT().editor.lint
    return hits
      .filter((h) => h.start >= 0 && h.end > h.start && h.end <= text.length)
      .map((h): Diagnostic => {
        const word = text.slice(h.start, h.end)
        const actions: Action[] = h.suggestions.slice(0, 5).map((s) => ({
          name: s === '' ? t.remove : s,
          apply: (v: EditorView, from: number, to: number) => v.dispatch({ changes: { from, to, insert: s } }),
        }))
        if (h.category === 'spelling')
          actions.push({
            name: t.addToDictionary,
            apply: (v: EditorView) => {
              hideWord(v, word)
              hooks.addWord(word, () => relint(v))
            },
          })
        return {
          from: h.start,
          to: h.end,
          severity: h.category === 'spelling' ? 'error' : 'warning',
          markClass: `cm-lint-${h.category}`,
          source: h.category === 'spelling' ? t.spelling : t.grammar,
          message: h.message,
          actions,
        }
      })
  }
  return linter(source, { delay: LINT_DELAY_MS })
}

/**
 * Runs the check again now: after a word was added to the dictionary or a toggle changed. Not
 * `forceLinting`: that only hurries a check already waiting after an edit, so with the text
 * unchanged it does nothing.
 */
export function relint(view: EditorView) {
  const check = checks.get(view)
  if (check) void check()
  else forceLinting(view) // the first check hasn't run yet
}
