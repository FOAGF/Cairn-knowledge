// Export / import of all data (FR-DATA-*). Rust writes and reads the archive; the UI only picks paths.
//
// Usage (Settings → Data):
//   const exportData = useExportData()          // exportData.mutate() opens "Save as", then exports
//   const path = await pickImportArchive()      // "Open" dialog; null if cancelled
//   const info = useBackupInfo(path).data       // its knowledge mode and folder (FR-KN-EXT-14)
//   const importData = useImportData()          // importData.mutate({ path }) after the user confirmed
//
// Failures toast automatically (see client.ts). After an import Rust emits DataChanged { all },
// which refreshes every query.
import { open, save } from '@tauri-apps/plugin-dialog'
import { useMutation, useQuery } from '@tanstack/react-query'
import { commands, type ExportSummary, type ImportSummary } from './bindings'

const ZIP_FILTER = [{ name: 'Cairn export', extensions: ['zip'] }] // i18n-ignore: the file type's name, kept like the suggested file name

/** Opens "Save as" and exports. Resolves with `null` if the user cancelled the dialog. */
export function useExportData() {
  return useMutation<ExportSummary | null>({
    mutationFn: async () => {
      const day = new Date().toLocaleDateString('sv') // YYYY-MM-DD in local time
      const path = await save({ defaultPath: `cairn-export-${day}.zip`, filters: ZIP_FILTER })
      return path ? commands.exportData(path) : null
    },
  })
}

/** Opens a file dialog for an export archive. Returns the path, or `null` if cancelled. */
export async function pickImportArchive(): Promise<string | null> {
  const path = await open({ multiple: false, directory: false, filters: ZIP_FILTER })
  return typeof path === 'string' ? path : null
}

/** What an archive says about itself before it is imported: its knowledge mode and folder (FR-KN-EXT-14). */
export function useBackupInfo(path: string | null) {
  return useQuery({ queryKey: ['backupInfo', path], queryFn: () => commands.readBackupInfo(path ?? ''), enabled: !!path, staleTime: 0, gcTime: 0 })
}

/**
 * An import: the whole archive; an external-mode archive with its folder at `root` (where it is on this
 * computer now); or, with `tasksOnly`, only its tasks and categories (FR-KN-EXT-14).
 */
export type ImportRequest = { path: string; root?: string; tasksOnly?: boolean }

/** Replaces ALL data with the archive at `path` (FR-DATA-6: Rust backs up the current data first). Ask first. */
export function useImportData() {
  return useMutation<ImportSummary, unknown, ImportRequest>({
    mutationFn: ({ path, root, tasksOnly }) =>
      tasksOnly ? commands.importTasksOnly(path) : root ? commands.importDataWithRoot(path, root) : commands.importData(path),
  })
}
