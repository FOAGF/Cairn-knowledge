// Glossary: queries and mutations (FR-KN-GL-*, UI-KN-8). Rust emits DataChanged { glossary } after every change,
// which refetches everything under ['glossary'] (the list and every "Used in").
import { useInfiniteQuery, useMutation, useQuery } from '@tanstack/react-query'
import { type GlossaryPatch, type GlossarySeed, type NewGlossaryEntry, commands } from './bindings'

export type { GlossaryEntry, GlossaryPatch, GlossaryUsage, NewGlossaryEntry, UsageHit } from './bindings'

/** Every entry, global and category ones. A small list, so it is read whole and filtered in the UI. */
export function useGlossary() {
  return useQuery({ queryKey: ['glossary'], queryFn: () => commands.listGlossary() })
}

export function useCreateGlossaryEntry() {
  return useMutation({ mutationFn: (input: NewGlossaryEntry) => commands.createGlossaryEntry(input) })
}

export function useUpdateGlossaryEntry() {
  return useMutation({ mutationFn: ({ id, patch }: { id: string; patch: GlossaryPatch }) => commands.updateGlossaryEntry(id, patch) })
}

export function useDeleteGlossaryEntry() {
  return useMutation({ mutationFn: (id: string) => commands.deleteGlossaryEntry(id) })
}

/** UI-KN-8: "Used in" shows this many places first... */
export const USAGE_FIRST = 3
/** ...and this many more each time *Show more* is pressed. */
export const USAGE_STEP = 10

/**
 * Where an entry is used, a page of results at a time (the first 3, then 10 more per *Show more*),
 * so a term used in hundreds of places never makes the screen wait. `total` comes with every page.
 */
export function useGlossaryUsage(entryId: string) {
  const query = useInfiniteQuery({
    queryKey: ['glossary', 'usage', entryId],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => commands.glossaryUsage(entryId, pageParam, pageParam === 0 ? USAGE_FIRST : USAGE_STEP),
    // The next offset is how many items are loaded; no next page once all `total` are.
    getNextPageParam: (last, pages) => {
      const loaded = pages.reduce((n, p) => n + p.items.length, 0)
      return loaded < last.total && last.items.length > 0 ? loaded : undefined
    },
  })
  const items = query.data?.pages.flatMap((p) => p.items) ?? []
  const total = query.data?.pages[0]?.total ?? 0
  return { ...query, items, total }
}

/** Development only: inserts the sample glossary where ids are missing (never overwrites). Runs after categories and pages exist. */
export async function seedDevGlossary(seed: GlossarySeed[]) {
  await commands.devSeedGlossary(seed)
}
