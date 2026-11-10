// Duplicate check (FR-KN-DU-1): similar pages for a draft or an existing page. A draft in the review
// step, a page in the checks (*Find similar pages*, FR-RAG-SR-5) and the merge screen get their
// matches from `find_duplicates` (by meaning, keyword search while the index isn't ready):
// `useDuplicateMatches` only looks up the pages.
import type { DuplicateReport } from '@/api/bindings'
import { usePagesByIds } from '@/api/knowledge'
import type { MeaningStatus } from '@/api/search'
import type { Match } from '@/lib/search'

export type DuplicateMatch = Match & { likely: boolean }

/** The pages of a `find_duplicates` report (FR-KN-DU-1), in its order, with likely ones marked. */
export function useDuplicateMatches(report: DuplicateReport | null): { matches: DuplicateMatch[]; meaning?: MeaningStatus } {
  const ids = report?.hits.map((h) => h.pageId) ?? []
  const { data: pages } = usePagesByIds(ids)
  const matches = (report?.hits ?? []).flatMap((h) => {
    const page = pages?.find((p) => p.id === h.pageId)
    return page ? [{ page, score: Math.max(0, h.score ?? 0), snippet: h.snippet, likely: h.likely }] : []
  })
  return { matches, meaning: report?.meaning }
}
