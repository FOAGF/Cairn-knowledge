// Categories: queries and mutations (FR-PRJ-*). Rust emits DataChanged after every change,
// so the mutations here do not invalidate anything themselves.
import { useMutation, useQuery } from '@tanstack/react-query'
import { type NewCategory, type Category, type CategoryChanges, commands } from './bindings'
import { queryClient } from './client'

export type { NewCategory, Category, CategoryChanges } from './bindings'

/** The prototype's sample category shape (lib/mock-data.ts). */
type SampleCategory = Pick<Category, 'id' | 'name' | 'key' | 'description' | 'color' | 'status' | 'createdAt'> & { isGeneral?: boolean }

const EMPTY: Category[] = []

const listKey = ['categories'] as const

/** All categories, archived ones included, General first. Screens filter by `status`. */
export function useCategories() {
  return useQuery({ queryKey: listKey, queryFn: () => commands.listCategories(true) })
}

/** The category list for screens that only look categories up: empty until loaded, never a new array. */
export function useCategoryList(): Category[] {
  return useCategories().data ?? EMPTY
}

export function useCategory(id: string | undefined) {
  return useQuery({
    queryKey: ['categories', id],
    queryFn: () => commands.getCategory(id ?? ''),
    enabled: !!id,
  })
}

/** Current cached list outside React (store actions, fake assistant code). Empty if not loaded yet. */
export function getCategoriesSnapshot(): Category[] {
  return queryClient.getQueryData<Category[]>(listKey) ?? EMPTY
}

export function useCreateCategory() {
  return useMutation({ mutationFn: (input: NewCategory) => commands.createCategory(input) })
}

export function useUpdateCategory() {
  return useMutation({
    mutationFn: ({ id, changes }: { id: string; changes: Partial<CategoryChanges> }) =>
      commands.updateCategory(id, { name: null, description: null, color: null, ...changes }),
  })
}

export function useArchiveCategory() {
  return useMutation({
    mutationFn: ({ id, archived }: { id: string; archived: boolean }) => commands.archiveCategory(id, archived),
  })
}

/** Asks Rust for a free task key for a category name (FR-PRJ-6). */
export function useSuggestCategoryKey() {
  return useMutation({ mutationFn: (name: string) => commands.suggestCategoryKey(name) })
}

/**
 * Development only: inserts the prototype's sample categories that the not-yet-migrated mock screens
 * (tasks, pages, ...) refer to. Insert-if-missing in Rust, so it never overwrites user changes.
 * Remove together with the last mock area that uses these ids.
 */
export async function seedDevCategories(sample: SampleCategory[]) {
  const seeds = sample.filter((p) => !p.isGeneral).map(({ id, name, key, description, color, status, createdAt }) => ({ id, name, key, description, color, status, createdAt }))
  await commands.devSeedCategories(seeds)
}
