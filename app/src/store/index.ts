import { create } from 'zustand'
import { createJSONStorage, persist } from 'zustand/middleware'
import { createCreationSlice } from './creation'
import type { AppState } from './types'
import { createUiSlice, initialUi } from './ui'

export type { AppState } from './types'

/**
 * Prototype store. Data is kept in the browser's localStorage so edits survive
 * a reload; `resetDemoData()` restores the sample data.
 */
export const useAppStore = create<AppState>()(
  persist(
    (...a) => ({
      ...createCreationSlice(...a),
      ...createUiSlice(...a),
    }),
    {
      name: 'cairn-prototype',
      // v3: settings moved to the Rust core (api/settings.ts). v4: categories too (api/categories.ts).
      // v5: tasks, links, comments and history too (api/tasks.ts).
      // v6: knowledge (groups, pages, history, task-page links) and activity too (api/knowledge.ts).
      // v7: AI providers and models too (api/providers.ts).
      // v8: conversations too (api/conversations.ts, saved from M5).
      version: 8,
      migrate: (persisted, version) => {
        const s = persisted as {
          categories?: unknown
          settings?: unknown
          tasks?: unknown
          taskLinks?: unknown
          taskComments?: unknown
          taskHistory?: unknown
          groups?: unknown
          pages?: unknown
          pageHistory?: unknown
          taskPageLinks?: unknown
          activity?: unknown
          providers?: unknown
          models?: unknown
          conversations?: unknown
        }
        // Both moved to Rust; drop the saved copies (the old settings/categories keys).
        if (version < 4) {
          delete s.settings
          delete s.categories
        }
        // Tasks (with links, comments and history) moved to Rust as well.
        if (version < 5) {
          delete s.tasks
          delete s.taskLinks
          delete s.taskComments
          delete s.taskHistory
        }
        // Knowledge and activity moved to Rust as well.
        if (version < 6) {
          delete s.groups
          delete s.pages
          delete s.pageHistory
          delete s.taskPageLinks
          delete s.activity
        }
        // Providers and models moved to Rust as well.
        if (version < 7) {
          delete s.providers
          delete s.models
        }
        // Conversations are saved by Rust from M5 (the prototype's sample ones are dropped).
        if (version < 8) delete s.conversations
        return s as AppState
      },
      storage: createJSONStorage(() => localStorage),
      // Only persist data, not UI flags. The pending page is kept in the data folder instead
      // (features/creation/pending.tsx): this storage is shared by every Cairn build on the machine.
      partialize: ({ commandOpen: _c, draft: _d, draftQueue: _q, draftPath: _p, pageEdit: _e, pendingAsk: _a, ...rest }) => rest,
    },
  ),
)

export function resetDemoData() {
  useAppStore.setState({
    ...initialUi(),
    draft: null,
    draftQueue: [],
    pageEdit: null,
  })
}

// Dev only: expose the store for quick experiments and screenshot scripts.
if (import.meta.env.DEV) (window as unknown as { __cairn: typeof useAppStore }).__cairn = useAppStore
