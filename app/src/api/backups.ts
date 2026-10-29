// Backups (NFR-REL-2, UI-SET-4). Rust makes the daily backup by itself and sends DataChanged
// { backups } after every backup, so these queries refresh on their own.
//
//   const status = useBackupStatus().data     // folder, last backup, last failure, database check
//   const list = useBackups().data            // newest first
//   const backupNow = useBackupNow()          // backupNow.mutate()
//   const setFolder = useSetBackupFolder()    // setFolder.mutate(path) or setFolder.mutate(null)
import { open } from '@tauri-apps/plugin-dialog'
import { useMutation, useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { getT } from '@/i18n'
import { type BackupFile, type BackupStatus, commands } from './bindings'

export type { BackupFile, BackupStatus }

/** The *Last backup* line and the folder in use. */
export function useBackupStatus() {
  return useQuery({ queryKey: ['backups', 'status'], queryFn: () => commands.backupStatus() })
}

/** The backups in the backup folder, newest first. */
export function useBackups() {
  return useQuery({ queryKey: ['backups', 'list'], queryFn: () => commands.listBackups() })
}

/** *Back up now*: a manual backup, kept until the user deletes it. */
export function useBackupNow() {
  return useMutation({
    mutationFn: () => commands.backupNow(),
    onSuccess: (f) => toast.success(getT().settings.backup.saved(f.fileName)),
  })
}

/** Saves the backup folder after Rust checked it can write there; `null` is the default folder. */
export function useSetBackupFolder() {
  return useMutation({ mutationFn: (folder: string | null) => commands.setBackupFolder(folder) })
}

/** The folder picker for *Choose folder…*. `null` if cancelled. */
export async function pickBackupFolder(): Promise<string | null> {
  const path = await open({ directory: true, multiple: false })
  return typeof path === 'string' ? path : null
}
