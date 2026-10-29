// Interface languages (NFR-I18N-2, D-79): English and Simplified Chinese.
//
// Every string the interface writes lives in `en/<area>.ts`, with its Chinese in `zh-CN/<area>.ts`
// typed against the English, so a missing or extra key is a type error. Components read them with
// `const t = useT()` and `t.tasks.board.empty`; a message with values is a function
// (`t.tasks.overdue(3)`). Code outside components (toasts in `api/`) uses `getT()`.
//
// Text Cairn stores as data (page bodies, saved titles, what the UI writes into a draft) is not
// interface text and stays in the code, in English. So do messages from the Rust core.
import { useSyncExternalStore } from 'react'
import { en, type Messages } from './en'
import { zhCN } from './zh-CN'

export type { Messages }

/** A shown language; the setting can also be `system` (`LanguageSync` resolves it). */
export type Lang = 'en' | 'zh-CN'

const TABLES: Record<Lang, Messages> = { en, 'zh-CN': zhCN }

/** The last language shown, so the first paint is in it before the settings load. */
const LANG_KEY = 'cairn-language'

/** Chinese when the system's first language is Chinese, otherwise English. */
export function systemLang(): Lang {
  const first = (navigator.languages?.[0] ?? navigator.language ?? '').toLowerCase()
  return first.startsWith('zh') ? 'zh-CN' : 'en'
}

function lastLang(): Lang {
  try {
    const v = localStorage.getItem(LANG_KEY)
    if (v === 'en' || v === 'zh-CN') return v
  } catch {
    // storage unavailable: follow the system
  }
  return systemLang()
}

let current: Lang = lastLang()
document.documentElement.lang = current
const listeners = new Set<() => void>()

/** Shows the interface in `lang`; components that read it re-render. */
export function setLang(lang: Lang) {
  if (lang === current) return
  current = lang
  document.documentElement.lang = lang
  try {
    localStorage.setItem(LANG_KEY, lang)
  } catch {
    // storage unavailable: only the first-paint guess is lost
  }
  for (const l of listeners) l()
}

export function getLang(): Lang {
  return current
}

/** The messages in the current language, for code outside components. */
export function getT(): Messages {
  return TABLES[current]
}

function subscribe(l: () => void) {
  listeners.add(l)
  return () => listeners.delete(l)
}

/**
 * The current language. A component that shows text through a helper (dates, `statusLabel`)
 * without calling `useT` calls this, so it re-renders when the language changes.
 */
export function useLang(): Lang {
  return useSyncExternalStore(subscribe, getLang)
}

/** The messages in the current language; the component re-renders when it changes. */
export function useT(): Messages {
  return TABLES[useLang()]
}
