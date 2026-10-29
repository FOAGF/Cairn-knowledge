// Keyword search over pages and tasks (FR-KN-UI-2; NFR-I18N-4 Chinese) and spelling/grammar
// checks with native Harper (FR-KN-ED-6..8). Search results refresh on DataChanged { search }.
import { keepPreviousData, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import {
  commands,
  events,
  type HybridResult,
  type IndexProgress,
  type LintHit,
  type LintOptions,
  type MeaningStatus,
  type SearchHit,
  type SearchQuery,
  type SidecarStatus,
} from './bindings'
import { isTauri } from './client'

export type { LintCategory, LintHit, LintOptions, SearchHit, SearchKind, SearchQuery } from './bindings'
export type { HybridResult, IndexProgress, MatchedBy, MeaningStatus, SidecarStatus, WorkerStatus } from './bindings'

const EMPTY_HITS: SearchHit[] = []

/**
 * Pages and tasks matching `query.text`, best first. An empty text returns nothing without a call.
 * Keep `query` stable (useMemo); results stay visible while the next ones load.
 */
export function useSearch(query: SearchQuery) {
  const enabled = query.text.trim().length > 0
  const result = useQuery({
    queryKey: ['search', query],
    queryFn: () => commands.search(query),
    enabled,
    placeholderData: keepPreviousData,
  })
  return { ...result, hits: enabled ? (result.data ?? EMPTY_HITS) : EMPTY_HITS }
}

/** For code outside React (command palette actions, fake assistant). */
export function searchOnce(query: SearchQuery): Promise<SearchHit[]> {
  return query.text.trim() ? commands.search(query) : Promise.resolve(EMPTY_HITS)
}

/**
 * Spelling and grammar issues in Markdown `text`. Offsets are UTF-16 (usable on JS strings directly).
 * The personal dictionary from settings is applied in Rust. The first call loads Harper (about 0.6 s).
 */
export function lintText(text: string, options: LintOptions): Promise<LintHit[]> {
  if (!options.spelling && !options.grammar) return Promise.resolve([])
  return commands.lintText(text, options)
}

export function rebuildSearchIndex() {
  return commands.rebuildSearchIndex()
}

// ---------------------------------------------------------------------- M4: search by meaning

/**
 * True when results were ranked by meaning, so their scores are similarities. Without the search
 * model (`unavailable`, or `not_installed` on a lite package, D-82) they are keyword matches.
 */
export const rankedByMeaning = (meaning: MeaningStatus) => meaning.state !== 'unavailable' && meaning.state !== 'not_installed'

/** Cosine similarity from which a page counts as a likely duplicate (M0-b threshold, 256 dimensions). */
export const LIKELY_DUPLICATE_SCORE = 0.87

/**
 * Keyword + meaning search merged (FR-RAG-SR-1, -4, -5). Slower than `useSearch` (the query is
 * embedded first), so callers enable it only after a pause in typing. `data` is undefined until
 * the result for this exact query is in (no stale results from another text). Refreshes on
 * `DataChanged { search }`, e.g. when indexing finishes.
 */
export function useHybridSearch(query: SearchQuery, enabled: boolean) {
  return useQuery({
    queryKey: ['search', 'hybrid', query],
    queryFn: () => commands.searchHybrid(query),
    enabled: enabled && query.text.trim().length > 0,
  })
}

/** Pages closest in meaning to a saved page (*Find similar pages*, FR-RAG-SR-5). */
export function findSimilarPages(pageId: string, limit = 10): Promise<HybridResult> {
  return commands.findSimilarPages(pageId, limit)
}

/** Index progress for Settings › Search: the current value, then every `IndexProgress` event. */
export function useIndexProgress(): IndexProgress | undefined {
  return useEventQuery(['indexProgress'], commands.getIndexProgress, events.indexProgress)
}

/** State of the two AI worker processes: the current value, then every `SidecarStatus` event. */
export function useSidecarStatus(): SidecarStatus | undefined {
  return useEventQuery(['sidecarStatus'], commands.sidecarStatus, events.sidecarStatus)
}

/** Reads a value once, then keeps it current from a Rust event whose payload is the new value. */
export function useEventQuery<T>(
  queryKey: string[],
  read: () => Promise<T>,
  event: { listen: (handler: (e: { payload: T }) => void) => Promise<() => void> },
): T | undefined {
  const client = useQueryClient()
  const { data } = useQuery({
    queryKey,
    queryFn: read,
    enabled: isTauri(),
    // Read again whenever a screen using it opens, in case an event was missed meanwhile.
    staleTime: 0,
  })
  const key = queryKey.join('/')
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void event
      .listen(({ payload }) => client.setQueryData(key.split('/'), payload))
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [client, key, event])
  return data
}
