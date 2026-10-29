// Duplicate check (FR-KN-DU-1): similar pages for a draft or an existing page.
// - A draft in the review step, a page in the checks and the merge screen get their matches from
//   `find_duplicates` (by meaning, keyword search while the index isn't ready): `useDuplicateMatches`
//   only looks up the pages.
// - A saved page ("Find similar pages", FR-RAG-SR-5) uses search by meaning: `useSimilarToPage`.
import { useQuery } from '@tanstack/react-query'
import { commands, type DuplicateReport } from '@/api/bindings'
import { findSimilarPages, type MeaningStatus } from '@/api/search'
import type { Match } from '@/lib/search'

/**
 * Pages closest in meaning to the saved page `pageId` (cosine score 0..1 in `Match.score`), plus
 * how far the index is (FR-RAG-IX-6). Refreshes when indexing finishes (`DataChanged { search }`).
 */
export function useSimilarToPage(enabled: boolean, pageId: string): { matches: Match[]; meaning?: MeaningStatus; searching: boolean } {
  const { data, isPending } = useQuery({
    queryKey: ['search', 'similar', pageId],
    enabled,
    queryFn: async () => {
      const result = await findSimilarPages(pageId, 5)
      const pages = await commands.getPagesByIds(result.hits.map((h) => h.id))
      const matches = result.hits.flatMap((h) => {
        const page = pages.find((p) => p.id === h.id)
        return page ? [{ page, score: Math.max(0, h.score ?? 0), snippet: h.snippet.replace(/[«»]/g, '') }] : []
      })
      return { matches, meaning: result.meaning }
    },
  })
  return { matches: data?.matches ?? [], meaning: data?.meaning, searching: enabled && isPending }
}

export type DuplicateMatch = Match & { likely: boolean }

/** The pages of a `find_duplicates` report (FR-KN-DU-1), in its order, with likely ones marked. */
export function useDuplicateMatches(report: DuplicateReport | null): { matches: DuplicateMatch[]; meaning?: MeaningStatus } {
  const ids = report?.hits.map((h) => h.pageId) ?? []
  const { data: pages } = useQuery({
    queryKey: ['knowledge', 'pages', 'by-ids', ids],
    enabled: ids.length > 0,
    queryFn: () => commands.getPagesByIds(ids),
  })
  const matches = (report?.hits ?? []).flatMap((h) => {
    const page = pages?.find((p) => p.id === h.pageId)
    return page ? [{ page, score: Math.max(0, h.score ?? 0), snippet: h.snippet, likely: h.likely }] : []
  })
  return { matches, meaning: report?.meaning }
}
