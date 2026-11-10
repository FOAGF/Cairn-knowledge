// The tech news on Home (FR-HOME-7, D-92). Rust fetches it in the background on the web search
// model and sends DataChanged { news } when a fetch starts and ends, which reads it again.
//
//   const view = useNews().data           // { news: the saved result, fetching }
//   startNews(lang, false)                // the daily fetch, when due; `true` for *Refresh*
//
// Query key: ['news']
import { useQuery } from '@tanstack/react-query'
import { commands, type NewsState, type NewsStory, type NewsView } from './bindings'
import { queryClient } from './client'
import type { Lang } from '@/i18n'

export type { NewsState, NewsStory, NewsView }

const KEY = ['news'] as const

export function useNews() {
  return useQuery({ queryKey: KEY, queryFn: () => commands.news() })
}

/** Starts a fetch when one is due (or always with `force`); returns at once. `lang` is the interface language the headlines are written in. */
export function startNews(lang: Lang, force: boolean): Promise<void> {
  return commands.fetchNews(lang, force).then((view) => {
    queryClient.setQueryData(KEY, view)
  })
}
