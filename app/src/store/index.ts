import { create } from 'zustand'
import { createCreationSlice } from './creation'
import type { AppState } from './types'
import { createUiSlice } from './ui'

export type { AppState } from './types'

/** UI state only, in memory: data lives in Rust, and the pending page is saved there too (creation/pending.ts). */
export const useAppStore = create<AppState>()((...a) => ({
  ...createCreationSlice(...a),
  ...createUiSlice(...a),
}))

export function resetDemoData() {
  useAppStore.setState({
    draft: null,
    draftQueue: [],
    pageEdit: null,
  })
}

// Dev only: expose the store for quick experiments and screenshot scripts.
if (import.meta.env.DEV) (window as unknown as { __cairn: typeof useAppStore }).__cairn = useAppStore
