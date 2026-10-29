// The system's folder picker, shared by the Markdown folder dialogs, the knowledge mode chooser,
// Settings › General's *Knowledge pages* and the import of an external-mode backup.
import { open } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { isTauri } from '@/api/client'
import { getT } from '@/i18n'

/** A folder chosen by the user, or `null` when cancelled. */
export async function pickFolder(title: string): Promise<string | null> {
  // Development builds only: a scripted UI check answers the picker, which it can't drive.
  const scripted = import.meta.env.DEV ? (window as { __cairnPickFolder?: () => string | null }).__cairnPickFolder : undefined
  if (scripted) return scripted()
  if (!isTauri()) {
    toast.error(getT().storage.pickFolder.onlyInApp)
    return null
  }
  const path = await open({ directory: true, multiple: false, title })
  return typeof path === 'string' ? path : null
}
