import type { StateCreator } from 'zustand'
import type { CreationSlice } from './creation'
import type { UiSlice } from './ui'

export type AppState = CreationSlice & UiSlice

/** Use this to type each slice so it can read and call the rest of the store. */
export type SliceCreator<T> = StateCreator<AppState, [['zustand/persist', unknown]], [], T>
