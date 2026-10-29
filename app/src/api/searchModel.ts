// The search model added after installing (FR-RAG-EM-3, D-82). Lite packages leave the model out;
// Settings › Search downloads it or adds it from a folder or a `.zip`, and the first-start card and
// a one-time Home notice offer the download too.
//
//   const status = useSearchModelStatus()            // install, the task running or failed, ...
//   const download = useDownloadSearchModel()        // download.mutate(acceptTerms)
//   const path = await pickModelZip()                // or pickModelFolder(); null if cancelled
//
// Rust runs one download or import at a time, in the background; every change arrives as the
// `SearchModelStatus` event and the end of one as `SearchModelDone` (a toast, mounted once:
// `useSearchModelDoneToasts`). Each command returns the new state, which goes into the cache.
// Failures toast automatically (see client.ts).
import { open } from '@tauri-apps/plugin-dialog'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'
import { toast } from 'sonner'
import { getT } from '@/i18n'
import { commands, events, type ModelTask, type SearchModelStatus } from './bindings'
import { isTauri } from './client'
import { useEventQuery } from './search'

export type { ModelTask, SearchModelStatus } from './bindings'

const KEY = ['searchModel']

/** The search model's state: the current value, then every `SearchModelStatus` event. */
export function useSearchModelStatus(): SearchModelStatus | undefined {
  return useEventQuery(KEY, commands.getSearchModelStatus, events.searchModelStatus)
}

/** A command that returns the new state: the state goes into the cache at once. */
function useModelCommand<V = void>(run: (v: V) => Promise<SearchModelStatus>) {
  const client = useQueryClient()
  return useMutation<SearchModelStatus, unknown, V>({ mutationFn: run, onSuccess: (s) => client.setQueryData(KEY, s) })
}

/** Starts the download; `acceptTerms` records the Gemma Terms of Use tick (not asked again). */
export const useDownloadSearchModel = () => useModelCommand((acceptTerms: boolean) => commands.downloadSearchModel(acceptTerms))
/** Adds the model from a folder or a `.zip`, checked in the background. */
export const useAddSearchModel = () => useModelCommand((path: string) => commands.addSearchModel(path))
/** Stops a running download (its files are forgotten) or import; clears a failed one. */
export const useCancelSearchModelTask = () => useModelCommand(() => commands.cancelSearchModelTask())
/** Deletes an added model; the stored vectors are kept, so adding it back needs no rebuild. */
export const useRemoveSearchModel = () => useModelCommand(() => commands.removeSearchModel())
/** The one-time Home notice won't show again. */
export const useDismissSearchModelNotice = () => useModelCommand(() => commands.dismissSearchModelNotice())

/** Opens a file dialog for a `.zip` holding the model. Returns the path, or `null` if cancelled. */
export async function pickModelZip(): Promise<string | null> {
  const path = await open({ multiple: false, directory: false, filters: [{ name: getT().searchModel.zipFiles, extensions: ['zip'] }] })
  return typeof path === 'string' ? path : null
}

/** Opens a folder dialog for a folder holding the model. Returns the path, or `null` if cancelled. */
export async function pickModelFolder(): Promise<string | null> {
  const path = await open({ multiple: false, directory: true })
  return typeof path === 'string' ? path : null
}

/** Bytes in whole MB, for progress. */
export const toMB = (bytes: number) => Math.round(bytes / 1_000_000)

/** The model's size for the texts ("about 210 MB"), to the nearest 10 MB. */
export const aboutMB = (bytes: number) => Math.round(bytes / 10_000_000) * 10

/** How far a task is, 0 to 100. */
export const taskPercent = (task: ModelTask) => (task.totalBytes > 0 ? Math.min(100, Math.floor((task.doneBytes * 100) / task.totalBytes)) : 0)

/** A toast when a download or import ends (FR-RAG-EM-3). Mount once, in the app shell. */
export function useSearchModelDoneToasts() {
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void events.searchModelDone
      .listen(({ payload }) => {
        const m = getT().searchModel
        if (payload.ok) toast.success(m.ready)
        else {
          const error = payload.error ?? ''
          toast.error(payload.kind === 'download' ? m.downloadFailed(error) : m.importFailed(error))
        }
      })
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}
