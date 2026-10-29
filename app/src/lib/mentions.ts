// Mention syntax (docs/requirements.md §3.3, DM-MN-1): `#PRX-12` for tasks and `[Title](page:<id>)` for pages.
// Pages are stored by id, so renames never break a mention; the displayed title is looked up when shown.
// A legacy `[[Title]]` is plain text now (kept only so old text can still be recognised).

export const TASK_MENTION_RE = /(^|[^\w&])#([A-Z][A-Z0-9]{1,9}-\d+)\b/g
/** `[Title](page:<id>)`: group 1 is the stored title, group 2 the page id. */
export const PAGE_LINK_RE = /\[((?:[^\]\n\\]|\\.)*)\]\(page:([\w-]+)\)/g
/** Legacy `[[Title]]` (plain text now). */
export const PAGE_MENTION_RE = /\[\[([^\]\n]+)\]\]/g

export function extractMentions(text: string): { taskKeys: string[]; pageIds: string[] } {
  const taskKeys = [...text.matchAll(TASK_MENTION_RE)].map((m) => m[2])
  const pageIds = [...text.matchAll(PAGE_LINK_RE)].map((m) => m[2])
  return { taskKeys: [...new Set(taskKeys)], pageIds: [...new Set(pageIds)] }
}

/** Markdown for a page mention; brackets in the title are escaped so the link stays well-formed. */
export function pageLink(title: string, id: string): string {
  return `[${title.replace(/[[\]\\]/g, '\\$&').replace(/\s+/g, ' ')}](page:${id})`
}

/**
 * Turns `#KEY` into a markdown link with the custom scheme `cairn-task:KEY` so the renderer can draw a chip.
 * `[Title](page:<id>)` is already a link and is handled by the renderer.
 */
export function mentionsToLinks(text: string): string {
  return text.replace(TASK_MENTION_RE, (_m, pre: string, key: string) => `${pre}[${key}](cairn-task:${key})`)
}
