// A file dropped where nothing takes it must never replace the app: WebKit opens a dropped file as
// the page itself, and the app's close handling (api/drafts.ts) would go with it. Drop targets
// (e.g. the Assistant) call `preventDefault()` themselves; this catches every other place.
// The Rust side blocks any navigation away from the app as well (src-tauri/src/lib.rs).
import { commands } from '@/api/bindings'

let installed = false

const TYPES: Record<string, string> = {
  md: 'text/markdown',
  txt: 'text/plain',
  pdf: 'application/pdf',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
}

/** The first `file://` link in a drop's HTML (an `<a href>` or `<img src>`, else anywhere), or null. */
function linkInHtml(html: string): string | null {
  const m = /(?:href|src)\s*=\s*["']?(file:\/\/[^"'\s>]+)/i.exec(html) ?? /(file:\/\/[^\s"'<>]+)/i.exec(html)
  return m ? m[1].replace(/&amp;/g, '&') : null
}

/**
 * The first `file://` link of a drop, or null. Read it while the drop event runs: the data is gone
 * after. Some Linux file managers offer `text/uri-list` with no data in WebKitGTK and name the file
 * only in `text/html`, so that is read too.
 */
export function droppedLink(dataTransfer: DataTransfer | null): string | null {
  const list = dataTransfer?.getData('text/uri-list') ?? ''
  const fromList = list.split(/\r?\n/).map((l) => l.trim()).find((l) => l.startsWith('file://'))
  return fromList ?? linkInHtml(dataTransfer?.getData('text/html') ?? '')
}

/** The file named by a dropped link, read by Rust; only the types Cairn accepts (FR-KN-UP-1). */
export async function fileFromLink(link: string): Promise<File> {
  const { name, dataBase64 } = await commands.readDroppedFile(link)
  const bytes = Uint8Array.from(atob(dataBase64), (c) => c.charCodeAt(0))
  return new File([bytes], name, { type: TYPES[name.split('.').pop()?.toLowerCase() ?? ''] ?? '' })
}

/**
 * The dropped file. Linux file managers often send only a `file://` link, with no file in the drop;
 * then Rust reads the file. Call it in the drop handler itself: the link is read before the first await.
 */
export function fileFromDrop(dataTransfer: DataTransfer | null): Promise<File | null> {
  const file = dataTransfer?.files[0] ?? [...(dataTransfer?.items ?? [])].find((i) => i.kind === 'file')?.getAsFile()
  if (file) return Promise.resolve(file)
  const link = droppedLink(dataTransfer)
  return link ? fileFromLink(link) : Promise.resolve(null)
}

/** True for a drag of files from outside the page. File managers on Linux may offer only a
 *  `text/uri-list`, without `Files`, so that counts too unless the drag started inside the page. */
export function isExternalFileDrag(e: { dataTransfer: DataTransfer | null }): boolean {
  const types = [...(e.dataTransfer?.types ?? [])]
  return types.includes('Files') || (!internalDrag && types.includes('text/uri-list'))
}

let internalDrag = false

/** Installs the guard once. Call from the app shell. */
export function installDropGuard() {
  if (installed) return
  installed = true
  window.addEventListener('dragstart', () => (internalDrag = true))
  window.addEventListener('dragend', () => (internalDrag = false))
  window.addEventListener('dragover', (e) => {
    if (e.defaultPrevented || !isExternalFileDrag(e)) return
    // Not over a drop target: show "can't drop here" and cancel the default.
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'none'
  })
  window.addEventListener('drop', (e) => {
    internalDrag = false
    if (!e.defaultPrevented && isExternalFileDrag(e)) e.preventDefault()
  })
}
