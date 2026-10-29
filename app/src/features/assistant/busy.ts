import { create } from 'zustand'

/**
 * Transient chat state (not persisted, not from Rust): a file on its way to Rust. Answers, drafts
 * and revisions are Rust jobs; their state is on the stored message.
 */
interface BusyState {
  /** A file being sent to Rust: 0..1. */
  uploading: Record<string, number>
  setUploading: (id: string, fraction?: number) => void
}

export const useBusy = create<BusyState>((set) => ({
  uploading: {},
  setUploading: (id, fraction) =>
    set((s) => {
      const next = { ...s.uploading }
      if (fraction === undefined) delete next[id]
      else next[id] = fraction
      return { uploading: next }
    }),
}))
