import { useEffect } from 'react'
import { useSettingsQuery } from '@/api/settings'
import { setLang, systemLang } from '@/i18n'

/**
 * Applies the language setting (NFR-I18N-2, D-79). Until the settings load, the interface stays in
 * the last language shown (`i18n/index.ts`), so the first paint doesn't switch.
 */
export function LanguageSync() {
  const language = useSettingsQuery().data?.language
  useEffect(() => {
    if (!language) return
    const apply = () => setLang(language === 'system' ? systemLang() : language)
    apply()
    if (language !== 'system') return
    window.addEventListener('languagechange', apply)
    return () => window.removeEventListener('languagechange', apply)
  }, [language])
  return null
}
