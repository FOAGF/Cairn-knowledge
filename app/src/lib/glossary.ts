// The term-matching rules that draw the underlines (docs/requirements.md §5.9, FR-KN-GL-4, FR-KN-GL-6). Pure functions, no React.
// Rust has the same rules for "Used in" and search; `glossary-cases.json` holds the cases both must pass.

/** What the matcher needs from an entry; the generated `GlossaryEntry` (api/bindings.ts) has all of it. */
export interface MatchEntry {
  id: string
  term: string
  otherNames: string[]
  /** `null` = global; otherwise the one category the entry belongs to (FR-KN-GL-2). */
  categoryId: string | null
}

export interface TermMatch<E extends MatchEntry = MatchEntry> {
  start: number
  end: number
  entry: E
}

const HAN = /\p{Script=Han}/u
const escapeRegExp = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** FR-KN-GL-2: the entries an item of `categoryId` uses: global ones, with the category's own entries overriding same-term globals. */
export function effectiveEntries<E extends MatchEntry>(all: E[], categoryId: string | null): E[] {
  const own = categoryId ? all.filter((e) => e.categoryId === categoryId) : []
  const overridden = new Set(own.map((e) => e.term.trim().toLowerCase()))
  const globals = all.filter((e) => e.categoryId === null && !overridden.has(e.term.trim().toLowerCase()))
  return [...globals, ...own]
}

/** The regular expression for one name (term or other name), following FR-KN-GL-4. */
function patternFor(name: string): RegExp | null {
  const n = name.trim()
  if (!n) return null
  // Chinese: matches anywhere inside Chinese text, there are no word boundaries.
  if (HAN.test(n)) return new RegExp(escapeRegExp(n), 'g')
  const body = escapeRegExp(n).replace(/\s+/g, '\\s+')
  // ALL CAPITALS (at least two letters, no lowercase): case-sensitive. "AN" must not match "an".
  const capsOnly = n === n.toUpperCase() && /[A-Z]{2}/.test(n)
  // Whole word. Trailing "s"/"es" (plural) and "-<letters/digits>" (codes like MMU-07) belong to the word.
  // The boundary only looks at Latin letters and digits, so a term right next to Chinese text still matches.
  const word = '[A-Za-z0-9_\\u00C0-\\u024F]'
  return new RegExp(`(?<!${word})${body}(?:es|s)?(?:-[A-Za-z0-9]+)*(?!${word})`, capsOnly ? 'g' : 'gi')
}

export interface TermMatcher<E extends MatchEntry = MatchEntry> {
  /** Non-overlapping matches in text order. The longest match wins when two overlap. */
  find: (text: string) => TermMatch<E>[]
}

/** Builds a matcher for a set of entries (usually `effectiveEntries(...)`). */
export function buildMatcher<E extends MatchEntry>(entries: E[]): TermMatcher<E> {
  const patterns: { re: RegExp; entry: E }[] = []
  for (const entry of entries)
    for (const name of [entry.term, ...entry.otherNames]) {
      const re = patternFor(name)
      if (re) patterns.push({ re, entry })
    }
  return {
    find(text) {
      const all: TermMatch<E>[] = []
      for (const { re, entry } of patterns)
        for (const m of text.matchAll(re)) all.push({ start: m.index, end: m.index + m[0].length, entry })
      all.sort((a, b) => a.start - b.start || b.end - b.start - (a.end - a.start))
      const out: TermMatch<E>[] = []
      for (const m of all) if (!out.length || m.start >= out[out.length - 1].end) out.push(m)
      return out
    },
  }
}

/** Convenience for one-off use. */
export function findTerms<E extends MatchEntry>(all: E[], categoryId: string | null, text: string): TermMatch<E>[] {
  return buildMatcher(effectiveEntries(all, categoryId)).find(text)
}

/** D-38: a term is underlined only where it first appears on a page. Keeps the first match of each entry. */
export function firstPerEntry<E extends MatchEntry>(matches: TermMatch<E>[], seen = new Set<string>()): TermMatch<E>[] {
  return matches.filter((m) => !seen.has(m.entry.id) && !!seen.add(m.entry.id))
}

export interface MatchCases {
  entries: MatchEntry[]
  cases: { text: string; category: string | null; ids: string[] }[]
}

/**
 * Runs the shared cases (`glossary-cases.json`, also run by the Rust tests). Returns the failures; empty = all good.
 * No test framework in the repo; run it with node, see docs/development.md.
 */
export function selfCheckGlossaryRules(data: MatchCases): string[] {
  return data.cases.flatMap((c, i) => {
    const got = findTerms(data.entries, c.category, c.text).map((m) => m.entry.id)
    return got.join(',') === c.ids.join(',') ? [] : [`case ${i} (${JSON.stringify(c.text)} in ${c.category}): got [${got}], want [${c.ids}]`]
  })
}
