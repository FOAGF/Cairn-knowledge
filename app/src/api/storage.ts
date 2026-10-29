// Where knowledge lives (spec §5.11, D-72): Cairn's database (internal mode) or a folder of Markdown
// files (external mode). Rust: `src-tauri/src/commands/storage.rs`.
//
//   const external = useIsExternal()              // pages are files in a folder
//   const storage = useKnowledgeStorage().data    // mode, root folder, whether it is there
//   const list = useSwitchBackups().data          // FR-KN-EXT-9: the switch backups and their size
//   await switchKnowledge({ mode: 'external', root })   // returns once Rust checked; runs in the background
//   const run = useSwitchRun()                    // the switch in progress, or finished until closed
//
// Scans and switches report `KnowledgeProgress` events (mounted once: `useKnowledgeProgressSync`). A
// switch ends with DataChanged { all }; a scan that changed something with DataChanged { knowledge, … }.
// A large scan shows a quiet progress toast, and files left out of the index a notice (once a session).
// When the folder can't be watched (e.g. the system's limit on watched folders is used up), a scan
// ends with a notice too, once a session; *Rescan* tries the watcher again.
import { convertFileSrc } from '@tauri-apps/api/core'
import { useMutation, useQuery } from '@tanstack/react-query'
import { useEffect, useSyncExternalStore } from 'react'
import { toast } from 'sonner'
import { getT } from '@/i18n'
import { commands, events, type KnowledgeMode, type KnowledgeProgress, type ScanSkip } from './bindings'
import { errorMessage, isTauri, queryClient } from './client'
import { useSettings } from './settings'

export type { BackupInfo, KnowledgeMode, KnowledgeProgress, KnowledgeStorage, NameClash, ScanSummary, SwitchBackup, SwitchBackups } from './bindings'

/** Pages are Markdown files in a folder. False while the settings load. */
export function useIsExternal(): boolean {
  return useSettings().knowledgeMode === 'external'
}

/** The mode, the root folder and whether it is there. Read again on every mount: a drive can go away. */
export function useKnowledgeStorage() {
  return useQuery({ queryKey: ['settings', 'knowledgeStorage'], queryFn: () => commands.getKnowledgeStorage(), staleTime: 0 })
}

/** FR-KN-EXT-7: the first-start choice. External mode then reads the folder in the background. */
export async function chooseKnowledgeMode(mode: KnowledgeMode, root: string | null) {
  const storage = await commands.chooseKnowledgeMode(mode, root)
  await queryClient.invalidateQueries({ queryKey: ['settings'] })
  return storage
}

/** *Open folder*: the knowledge folder (external mode, when it is there) or the switch backups, in the file manager. */
export function openStorageFolder(which: 'knowledge' | 'switchBackups') {
  return commands.openFolder(which).catch((e) => toast.error(errorMessage(e)))
}

/** *Rescan*: reads the whole folder again in the background. */
export function rescanKnowledgeFolder() {
  return commands.rescanKnowledgeFolder()
}

// ---------------------------------------------------------------- switch backups (FR-KN-EXT-8, -9)

const BACKUPS_KEY = ['backups', 'switch'] as const

/** Newest first, with their total size and whether it is past the warning size. */
export function useSwitchBackups() {
  return useQuery({ queryKey: BACKUPS_KEY, queryFn: () => commands.listSwitchBackups() })
}

/** Reads the list again, e.g. after the warning size changed. */
export function refreshSwitchBackups() {
  return queryClient.invalidateQueries({ queryKey: BACKUPS_KEY })
}

export function useDeleteSwitchBackup() {
  return useMutation({
    mutationFn: (fileName: string) => commands.deleteSwitchBackup(fileName),
    onSuccess: (list) => queryClient.setQueryData(BACKUPS_KEY, list),
  })
}

// ---------------------------------------------------------------- scans and switches in progress

/** Where a switch goes: a mode, or (`folder`) another folder while staying in external mode. */
export type SwitchTarget = KnowledgeMode | 'folder'

type Runs = {
  scan: KnowledgeProgress | null
  /** Shown as the blocking progress dialog until closed after it finished. */
  switch: KnowledgeProgress | null
  /** Where the switch goes, known from the moment it is asked for. */
  switchTo: SwitchTarget | null
}

let runs: Runs = { scan: null, switch: null, switchTo: null }
const listeners = new Set<() => void>()
function setRuns(next: Partial<Runs>) {
  runs = { ...runs, ...next }
  listeners.forEach((l) => l())
}
function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** The switch in progress (or finished, until `closeSwitchRun`), and where it goes. */
export function useSwitchRun(): { progress: KnowledgeProgress | null; to: SwitchTarget | null } {
  const progress = useSyncExternalStore(subscribe, () => runs.switch)
  const to = useSyncExternalStore(subscribe, () => runs.switchTo)
  return { progress, to }
}

/** The newest scan event: running, or the summary of the last one. */
export function useScanRun(): KnowledgeProgress | null {
  return useSyncExternalStore(subscribe, () => runs.scan)
}

export function closeSwitchRun() {
  setRuns({ switch: null, switchTo: null })
}

/** The first step shown, until Rust reports its own (in the language of the moment). */
const starting = (): KnowledgeProgress => ({ task: 'switch', step: getT().storage.starting, done: 0, total: 0, finished: false, error: null, scan: null, switched: null })

/**
 * FR-KN-EXT-8: switch to a folder, back to Cairn's database (`backup`: the switch backup whose pages
 * are loaded, or null for no pages), or (`folder`) from the current folder to another one: one switch
 * with one switch backup, as if switching to internal mode and on to the other folder. The progress
 * dialog opens at once; Rust's events take it from there. Throws when Rust refuses before starting
 * (the folder can't be used, it is the current one, …).
 */
export async function switchKnowledge(
  to: { mode: 'external'; root: string } | { mode: 'internal'; backup: string | null } | { mode: 'folder'; root: string },
) {
  setRuns({ switch: starting(), switchTo: to.mode })
  try {
    if (to.mode === 'external') await commands.switchKnowledgeToExternal(to.root)
    else if (to.mode === 'folder') await commands.switchKnowledgeFolder(to.root)
    else await commands.switchKnowledgeToInternal(to.backup)
  } catch (e) {
    closeSwitchRun()
    throw e
  }
}

/** A scan with more files than this shows its progress. */
const LARGE_SCAN = 200
const SCAN_TOAST = 'knowledge-scan'
let scanToast = false
/** Files left out that were already reported this session. */
const skipsShown = new Set<string>()
let watchWarned = false

/** After a scan: tells once a session that changes made by other programs aren't picked up live. */
async function warnIfNotWatched() {
  await queryClient.invalidateQueries({ queryKey: ['settings', 'knowledgeStorage'] })
  if (watchWarned) return
  const s = await queryClient.fetchQuery({ queryKey: ['settings', 'knowledgeStorage'], queryFn: () => commands.getKnowledgeStorage() })
  if (!s.watchError) return
  watchWarned = true
  const t = getT().storage.notWatched
  toast.warning(t.title, {
    description: t.toast(s.watchError),
    duration: 20000,
    closeButton: true,
  })
}

/** "Updating the index: 340 of 1,200". */
export function progressText(p: KnowledgeProgress): string {
  return p.total > 0 ? getT().storage.progress(p.step, p.done.toLocaleString(), p.total.toLocaleString()) : p.step
}

function skipLine(s: ScanSkip) {
  return `${s.path}: ${s.reason}`
}

function onProgress(p: KnowledgeProgress) {
  // A switch's own scan reports as part of the switch.
  if (p.task === 'switch') {
    setRuns({ switch: p })
    return
  }
  setRuns({ scan: p })
  if (!p.finished) {
    if (scanToast || p.total > LARGE_SCAN) {
      scanToast = true
      toast.loading(getT().storage.scan.reading, { id: SCAN_TOAST, description: progressText(p), duration: Infinity })
    }
    return
  }
  if (scanToast) {
    toast.dismiss(SCAN_TOAST)
    scanToast = false
  }
  const t = getT().storage.scan
  if (p.error) toast.error(t.failed(p.error))
  void warnIfNotWatched().catch(() => undefined)
  const fresh = (p.scan?.skipped ?? []).filter((s) => !skipsShown.has(skipLine(s)))
  if (fresh.length === 0) return
  for (const s of fresh) skipsShown.add(skipLine(s))
  toast.warning(t.skipped(fresh.length), {
    description: t.skippedDescription(fresh.slice(0, 3).map(skipLine).join(' · '), fresh.length - 3),
    duration: 15000,
    closeButton: true,
  })
}

/** Collects `KnowledgeProgress` events. Mount once, in the app shell. */
export function useKnowledgeProgressSync() {
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void events.knowledgeProgress.listen(({ payload }) => onProgress(payload)).then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}

// ---------------------------------------------------------------- files in the folder

/** `folder:<path>` → the path from the root, each part decoded (the index percent-encodes them). */
export function folderPath(target: string): string {
  return target
    .replace(/^folder:/, '')
    .split('/')
    .map((part) => {
      try {
        return decodeURIComponent(part)
      } catch {
        return part
      }
    })
    .join('/')
}

/** URL for a `folder:<path>` image under `root` (Tauri asset protocol; Rust allows the root). */
export function folderFileUrl(root: string, target: string): string {
  return convertFileSrc(`${root.replace(/[\\/]+$/, '')}/${folderPath(target)}`)
}
