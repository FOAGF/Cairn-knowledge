// UI ↔ Rust plumbing (spec TC-13, TC-14).
//
// - Data owned by Rust is read with TanStack Query hooks (one file per area in this folder,
//   e.g. `categories.ts`) that call the generated `commands` from `bindings.ts`.
// - Query keys start with the Rust `Entity` name, e.g. ['categories'] or ['categories', id].
//   When Rust emits `DataChanged`, every query under that entity is fetched again.
// - Zustand (src/store) keeps only UI state and the not-yet-migrated mock areas.
import { MutationCache, QueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { toast } from 'sonner'
import { type ApiError, type Entity, events } from './bindings'

/** True inside the Tauri window; false in a plain browser tab (e.g. the Vite URL opened directly). */
export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export function isApiError(e: unknown): e is ApiError {
  return typeof e === 'object' && e !== null && 'kind' in e && 'message' in e
}

/** External knowledge mode: a page's file changed after editing started (FR-KN-EXT-11). */
export const isChangedOnDisk = (e: unknown) => isApiError(e) && e.kind === 'changed_on_disk'

/** A readable message for any error thrown by a command. */
export function errorMessage(e: unknown): string {
  if (isApiError(e)) return e.message
  if (e instanceof Error) return e.message
  return String(e)
}

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // Data only changes through our own commands, and those emit DataChanged.
      staleTime: Infinity,
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
  // Failed changes always tell the user; queries show their error where the data would be. A page
  // whose file changed while it was edited (D-72) gets the editor's conflict dialog instead.
  mutationCache: new MutationCache({ onError: (e) => !isChangedOnDisk(e) && toast.error(errorMessage(e)) }),
})

/** Home's overview (api/home.ts) is built from these, so it is fetched again when one changes. */
const HOME_SOURCES: Entity[] = ['tasks', 'knowledge', 'categories', 'conversations']

/** Refetch the queries of every entity Rust reports as changed. Mount once, in the app shell. */
export function useDataChangedSync() {
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void events.dataChanged
      .listen(({ payload }) => {
        if (payload.entities.includes('all')) void queryClient.invalidateQueries()
        else {
          for (const entity of payload.entities) void queryClient.invalidateQueries({ queryKey: [entity] })
          if (payload.entities.some((e) => HOME_SOURCES.includes(e))) void queryClient.invalidateQueries({ queryKey: ['home'] })
        }
      })
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}

export type { Entity }
