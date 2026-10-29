// FR-DATA-8: whether this copy is at its first start (Home's first-start card) or past it (the
// one-time search model notice, D-82).
import { useHomeOverview } from '@/api/home'
import { useSettingsQuery } from '@/api/settings'

/**
 * True when this copy is empty: no tasks, pages (archived ones too) or conversations, only the
 * General category (Rust's `hasData`), and the choice wasn't made yet. False once it has data or
 * the choice was made; undefined until both have loaded.
 */
export function useFirstStart(): boolean | undefined {
  const settings = useSettingsQuery()
  const overview = useHomeOverview()
  if (!settings.data || !overview.data) return undefined
  return !settings.data.welcomeDone && !overview.data.hasData
}
