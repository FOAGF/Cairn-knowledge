// Settings: one query, optimistic updates, data folder.
import { useMutation, useQuery } from '@tanstack/react-query'
import { type DataFolderInfo, type Settings, type SettingsPatch, commands } from './bindings'
import { queryClient } from './client'

export type { Settings, SettingsPatch, DataFolderInfo }

const KEY = ['settings'] as const

/**
 * Shown until the real settings arrive (and outside the Tauri window). Mirrors the Rust defaults in
 * `cairn-core/src/settings.rs`. A module-level constant, so selectors and effects see a stable value.
 */
export const DEFAULT_SETTINGS: Settings = {
  userName: '',
  theme: 'system',
  language: 'system',
  defaultModelId: null,
  ocrModelId: null,
  wordWarningThreshold: 1000,
  largeFileWarningMB: 10,
  duplicateScore: 0.78,
  checksAtOnce: 5,
  embeddingIdleTimeoutMin: 5,
  assistantMinRelevance: 0.4,
  assistantSourceBudget: 8000,
  assistantHistoryBudget: 30000,
  webSearchesPerMessage: 5,
  spellcheck: true,
  grammar: true,
  personalDictionary: [],
  backupEnabled: true,
  backupFolder: '',
  backupKeep: 7,
  welcomeDone: false,
  // While loading this reads as "not chosen"; the mode chooser waits for the real settings.
  knowledgeMode: null,
  externalRoot: null,
  switchBackupWarnGb: 2,
}

/** The settings query itself, for callers that must tell "still loading" from real values. */
export function useSettingsQuery() {
  return useQuery({ queryKey: KEY, queryFn: () => commands.getSettings() })
}

/** The settings, or the defaults while loading. */
export function useSettings(): Settings {
  return useSettingsQuery().data ?? DEFAULT_SETTINGS
}

/** For code outside React (store actions, pipelines): the cached settings, or the defaults. */
export function readSettings(): Settings {
  return queryClient.getQueryData<Settings>(KEY) ?? DEFAULT_SETTINGS
}

function applyPatch(s: Settings, p: SettingsPatch): Settings {
  const next: Record<string, unknown> = { ...s }
  for (const [k, v] of Object.entries(p)) {
    if (v === undefined || v === null) continue
    // An empty string clears a model choice (same rule as Rust).
    next[k] = (k === 'defaultModelId' || k === 'ocrModelId') && v === '' ? null : v
  }
  return next as Settings
}

/** Changes the given fields. The screen updates at once and rolls back if Rust rejects the change. */
export function useUpdateSettings() {
  const { mutate } = useMutation({
    mutationFn: (patch: SettingsPatch) => commands.updateSettings(patch),
    onMutate: async (patch) => {
      await queryClient.cancelQueries({ queryKey: KEY })
      const previous = queryClient.getQueryData<Settings>(KEY)
      queryClient.setQueryData<Settings>(KEY, applyPatch(previous ?? DEFAULT_SETTINGS, patch))
      return { previous }
    },
    onError: (_e, _patch, ctx) => {
      if (ctx?.previous) queryClient.setQueryData(KEY, ctx.previous)
    },
    onSettled: () => void queryClient.invalidateQueries({ queryKey: KEY }),
  })
  return mutate
}

/** Current data folder, the one saved for the next start, and whether it can be changed here. */
export function useDataFolder() {
  return useQuery({ queryKey: ['settings', 'dataFolder'], queryFn: () => commands.getDataFolder() })
}

export function useSetDataFolder() {
  return useMutation({ mutationFn: (folder: string) => commands.setDataFolder(folder) })
}
