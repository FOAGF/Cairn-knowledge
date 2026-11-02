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
