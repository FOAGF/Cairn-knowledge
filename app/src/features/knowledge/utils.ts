import type { PageFilter } from '@/api/knowledge'

export interface PageFilters {
  query: string
  categoryId: string // '' = all
  tag: string // '' = all
  notChecked: boolean
  showArchived: boolean
}

/** The list filter for the left panel (FR-KN-UI-1). The text search is separate (FR-KN-UI-2). */
export function toPageFilter(f: PageFilters): PageFilter {
  return {
    categoryIds: f.categoryId ? [f.categoryId] : null,
    tags: f.tag ? [f.tag] : null,
    checkStatus: f.notChecked ? 'not_checked' : null,
    includeArchived: f.showArchived,
    sort: 'title',
    descending: false,
  }
}

export type DiffLine = { type: 'same' | 'add' | 'del'; text: string }

/** Simple line diff (LCS). `from` = old version, `to` = new. */
export function lineDiff(from: string, to: string): DiffLine[] {
  const a = from.split('\n')
  const b = to.split('\n')
  const dp: number[][] = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0))
  for (let i = a.length - 1; i >= 0; i--)
    for (let j = b.length - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < a.length && j < b.length) {
    if (a[i] === b[j]) out.push({ type: 'same', text: a[i++] }), j++
    else if (dp[i + 1][j] >= dp[i][j + 1]) out.push({ type: 'del', text: a[i++] })
    else out.push({ type: 'add', text: b[j++] })
  }
  while (i < a.length) out.push({ type: 'del', text: a[i++] })
  while (j < b.length) out.push({ type: 'add', text: b[j++] })
  return out
}
