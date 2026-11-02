import type { SliceCreator } from './types'

export interface UiSlice {
  commandOpen: boolean
  setCommandOpen: (open: boolean) => void
}

export const createUiSlice: SliceCreator<UiSlice> = (set) => ({
  commandOpen: false,
  setCommandOpen: (open) => set({ commandOpen: open }),
})
