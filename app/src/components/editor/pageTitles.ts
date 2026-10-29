// Cached lookup of current page titles for `[Title](page:<id>)` chips in the editor.
// Requests made in the same tick are batched into one `getPagesByIds` call. Entries expire so renames show up.
import { commands } from '@/api/bindings'

const TTL_MS = 30_000
const cache = new Map<string, { title: string | null; at: number }>()
const waiting = new Map<string, Array<(title: string | null) => void>>()
let scheduled = false

export function rememberPageTitle(id: string, title: string) {
  cache.set(id, { title, at: Date.now() })
}

function flush() {
  scheduled = false
  const batch = new Map(waiting)
  waiting.clear()
  const ids = [...batch.keys()]
  commands.getPagesByIds(ids).then(
    (pages) => {
      const found = new Map(pages.map((p) => [p.id, p.title]))
      for (const id of ids) {
        const title = found.get(id) ?? null
        cache.set(id, { title, at: Date.now() })
        batch.get(id)?.forEach((cb) => cb(title))
      }
    },
    () => ids.forEach((id) => batch.get(id)?.forEach((cb) => cb(null))),
  )
}

/** The page's current title (null: not found or lookup failed). Calls `cb` once, possibly synchronously. */
export function resolvePageTitle(id: string, cb: (title: string | null) => void) {
  const hit = cache.get(id)
  if (hit && Date.now() - hit.at < TTL_MS) return cb(hit.title)
  const list = waiting.get(id) ?? []
  list.push(cb)
  waiting.set(id, list)
  if (!scheduled) {
    scheduled = true
    queueMicrotask(flush)
  }
}
