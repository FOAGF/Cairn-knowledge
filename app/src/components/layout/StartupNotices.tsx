// Messages about what start-up did, shown once (NFR-REL-5; M7 part A), e.g. "An import was
// interrupted. Your data from before it is back."
import { useEffect } from 'react'
import { toast } from 'sonner'
import { commands } from '@/api/bindings'
import { isTauri } from '@/api/client'

export function StartupNotices() {
  useEffect(() => {
    if (!isTauri()) return
    // Rust hands each notice out once, so a second call (React's dev double-run) gets none.
    void commands
      .takeStartupNotices()
      .then((notices) => {
        // Stays until closed, so it isn't missed while the app is still loading.
        for (const n of notices) toast.warning(n, { duration: Infinity, closeButton: true })
      })
      .catch((e) => console.error('could not read the start-up notices', e))
  }, [])
  return null
}
