// Streamed LLM text and *Stop* (M5, FR-LLM-AS-7, FR-KN-UP-4b). Commands that take a `streamId`,
// and the chat jobs (stream id = the assistant message id), send their text as `LlmDelta` events.
// One listener (mounted in the app shell) collects every stream from the start, so text that
// arrives before a component subscribes is not lost.
import { useEffect, useSyncExternalStore } from 'react'
import { newId } from '@/lib/id'
import { commands, events } from './bindings'
import { isApiError, isTauri } from './client'

const streams = new Map<string, string>()
const listeners = new Set<() => void>()
const notify = () => listeners.forEach((l) => l())

/** A fresh id for a command's `streamId` (and its *Stop*). */
export const newStreamId = () => newId('st')

/** Collects `LlmDelta` events. Mount once, in the app shell. */
export function useLlmDeltaSync() {
  useEffect(() => {
    if (!isTauri()) return
    let unlisten: (() => void) | undefined
    let cancelled = false
    void events.llmDelta
      .listen(({ payload }) => {
        streams.set(payload.streamId, (streams.get(payload.streamId) ?? '') + payload.text)
        notify()
      })
      .then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

/** The text streamed under `streamId` so far; "" for none. */
export function useLlmStream(streamId: string | null | undefined): string {
  return useSyncExternalStore(subscribe, () => (streamId ? (streams.get(streamId) ?? '') : ''))
}

/** Forget a stream's text once its result is shown (call when the command resolved). */
export function clearStream(streamId: string) {
  if (streams.delete(streamId)) notify()
}

/** *Stop*: ends the job or stream started under `id`. False when it had already finished. */
export function cancelJob(id: string): Promise<boolean> {
  return commands.cancelJob(id)
}

/** The error a stopped job ends with. */
export const isCancelled = (e: unknown) => isApiError(e) && e.kind === 'cancelled'

/** No usable model for this step (FR-KN-SV-3: skip it, save "not checked"); the message says what to set up. */
export const isLlmUnavailable = (e: unknown) => isApiError(e) && e.kind === 'llm_unavailable'
