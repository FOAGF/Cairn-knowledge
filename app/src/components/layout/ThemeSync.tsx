import { useEffect, useSyncExternalStore } from 'react'
import { useSettingsQuery } from '@/api/settings'

type Theme = 'system' | 'light' | 'dark'
const THEME_KEY = 'cairn-theme'
const DARK_QUERY = '(prefers-color-scheme: dark)'

/** The last theme seen, so the first paint does not flash light/dark before settings load. */
function lastTheme(): Theme {
  try {
    const t = localStorage.getItem(THEME_KEY)
    if (t === 'light' || t === 'dark' || t === 'system') return t
  } catch {
    // storage unavailable: use the default
  }
  return 'system'
}

/** The theme to show: the saved setting once loaded, the last known theme before that. */
function useTheme(): Theme {
  const { data } = useSettingsQuery()
  return data?.theme ?? lastTheme()
}

/** Applies the theme setting (light / dark / follow OS) to <html>. */
export function ThemeSync() {
  const theme = useTheme()
  useEffect(() => {
    try {
      localStorage.setItem(THEME_KEY, theme)
    } catch {
      // storage unavailable: only the flash guard is lost
    }
  }, [theme])
  useEffect(() => {
    const media = window.matchMedia(DARK_QUERY)
    const apply = () => {
      const dark = theme === 'dark' || (theme === 'system' && media.matches)
      document.documentElement.classList.toggle('dark', dark)
    }
    apply()
    media.addEventListener('change', apply)
    return () => media.removeEventListener('change', apply)
  }, [theme])
  return null
}

function watchSystemTheme(onChange: () => void) {
  const media = window.matchMedia(DARK_QUERY)
  media.addEventListener('change', onChange)
  return () => media.removeEventListener('change', onChange)
}

const systemIsDark = () => window.matchMedia(DARK_QUERY).matches

/** Whether the window is dark now; follows the OS while the theme is "system". */
export function useIsDark() {
  const theme = useTheme()
  const systemDark = useSyncExternalStore(watchSystemTheme, systemIsDark)
  return theme === 'dark' || (theme === 'system' && systemDark)
}
