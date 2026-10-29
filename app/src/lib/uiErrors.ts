// Errors in the UI go to the app's log (NFR-SEC-3, D-47): uncaught exceptions and rejected
// promises, with the screen they happened on and never any content.
import { commands } from '@/api/bindings'
import { isTauri } from '@/api/client'

/** Long enough for the error's name and message, short enough to leave no room for page text. */
const MAX_MESSAGE = 300

let installed = false

function describe(reason: unknown): string {
  if (reason instanceof Error) return `${reason.name}: ${reason.message}`
  if (typeof reason === 'string') return reason
  // A failed command rejects with the app's `{ kind, message }` (messages may name a title or file).
  if (reason && typeof reason === 'object' && 'message' in reason && typeof reason.message === 'string') {
    const kind = 'kind' in reason && typeof reason.kind === 'string' ? reason.kind : 'error'
    return `${kind}: ${reason.message}`
  }
  return 'non-error value'
}

function report(message: string) {
  if (!isTauri()) return
  // The route path is the screen; its query and hash can hold search words, so they stay out.
  const screen = window.location.pathname
  const short = message.length > MAX_MESSAGE ? `${message.slice(0, MAX_MESSAGE)}…` : message
  // A failing log call must never raise another error event.
  void commands.logUiError(short, screen).catch(() => {})
}

/** Installs the window's error handlers once. */
export function installUiErrorLog() {
  if (installed) return
  installed = true
  window.addEventListener('error', (e) => {
    // A broken image is an event on its element, not an exception; it has no `error`/`message`.
    if (!e.error && !e.message) return
    report(e.error ? describe(e.error) : e.message)
  })
  window.addEventListener('unhandledrejection', (e) => report(`unhandled rejection: ${describe(e.reason)}`)) // i18n-ignore: log line
}
