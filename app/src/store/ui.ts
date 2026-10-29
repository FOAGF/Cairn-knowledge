import type { SliceCreator } from './types'

export interface UiSlice {
  commandOpen: boolean
  setCommandOpen: (open: boolean) => void
}

export const initialUi = () => ({})

export const createUiSlice: SliceCreator<UiSlice> = (set) => ({
  ...initialUi(),
  commandOpen: false,
  setCommandOpen: (open) => set({ commandOpen: open }),
})
