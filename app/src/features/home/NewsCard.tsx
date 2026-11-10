// FR-HOME-7, D-92: the tech news on Home, below the Today list and the calendar. Only when it is
// switched on in Settings › General. Rust fetches once a day by itself (the first visit to Home
// that day) and again on *Refresh*; the card shows the saved result meanwhile.
import { isToday, parseISO } from 'date-fns'
import { ChevronDown, ChevronUp, Newspaper, RefreshCw } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Link } from 'react-router'
import { openUrl } from '@/api/assistant'
import { toastError } from '@/api/client'
import { useLocalDay } from '@/api/home'
import { type NewsStory, startNews, useNews } from '@/api/news'
import { useModelList, useProviderList, webSearchModel } from '@/api/providers'
import { useSettings } from '@/api/settings'
import { SectionCard } from '@/components/shared/PageHeader'
import { Button } from '@/components/ui/button'
import { formatDate } from '@/lib/format'
import { cn } from '@/lib/utils'
import { useLang, useT } from '@/i18n'

/** Stories shown before *Show more*. */
const SHOWN = 5

function StoryRow({ story }: { story: NewsStory }) {
  const n = useT().home.news
  const meta = [story.source, story.date && formatDate(story.date, 'shortDay')].filter(Boolean).join(' · ')
  return (
    <li className="border-t py-2.5 first:border-t-0">
      <button
        type="button"
        title={n.open}
        onClick={() => openUrl(story.url).catch(toastError)}
        className="text-left text-sm font-medium hover:text-primary hover:underline"
      >
        {story.title}
      </button>
      {story.summary && <p className="mt-0.5 text-sm text-muted-foreground">{story.summary}</p>}
      {meta && <p className="mt-0.5 text-xs text-muted-foreground">{meta}</p>}
    </li>
  )
}

export function NewsCard() {
  const { techNews, webSearchModelId } = useSettings()
  const lang = useLang()
  const day = useLocalDay()
  const view = useNews().data
  const web = webSearchModel(useProviderList(), useModelList(), webSearchModelId)
  const [open, setOpen] = useState(false)
  const t = useT()
  const n = t.home.news

  // The daily fetch: Rust decides whether one is due today.
  useEffect(() => {
    if (techNews) startNews(lang, false).catch(toastError)
    // Not again when only the language changes: *Refresh* fetches in the new one.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [techNews, day])

  if (!techNews) return null
  const news = view?.news
  const fetching = !!view?.fetching
  const stories = news?.stories ?? []
  const fetchedAt = news?.fetchedAt
  const when = fetchedAt && (isToday(parseISO(fetchedAt)) ? formatDate(fetchedAt, 'time') : formatDate(fetchedAt, 'dateTime'))
  const description = fetching ? n.fetching : [when && n.updated(when), news?.topics].filter(Boolean).join(' · ')

  const refresh = (
    <Button variant="ghost" size="sm" title={n.refreshHint} disabled={fetching || !web} onClick={() => startNews(lang, true).catch(toastError)}>
      <RefreshCw className={cn('size-4', fetching && 'animate-spin')} /> {n.refresh}
    </Button>
  )

  return (
    <SectionCard icon={Newspaper} tint="purple" title={n.title} description={description || undefined} action={refresh}>
      {!web && (
        <p className="pb-2 text-sm text-muted-foreground">
          {n.noModel((text) => (
            <Link to="/settings/models" className="text-primary underline-offset-4 hover:underline">
              {text}
            </Link>
          ))}
        </p>
      )}
      {news?.error && !fetching && (
        <div className="mb-2 flex flex-wrap items-center gap-3 rounded-lg bg-tint-rose px-3 py-2 text-sm text-tint-rose-fg">
          <span className="min-w-0 flex-1">{n.failed(news.error)}</span>
          {web && (
            <Button size="xs" variant="outline" onClick={() => startNews(lang, true).catch(toastError)}>
              {t.common.tryAgain}
            </Button>
          )}
        </div>
      )}
      {stories.length > 0 ? (
        <>
          <ul>
            {(open ? stories : stories.slice(0, SHOWN)).map((s) => (
              <StoryRow key={s.url} story={s} />
            ))}
          </ul>
          {stories.length > SHOWN && (
            <button type="button" onClick={() => setOpen(!open)} className="mt-1 flex items-center gap-1.5 border-t border-dashed pt-2 text-sm text-muted-foreground hover:text-foreground">
              {open ? <ChevronUp className="size-4" /> : <ChevronDown className="size-4" />}
              {open ? t.common.showLess : n.showMore(stories.length - SHOWN)}
            </button>
          )}
        </>
      ) : (
        !fetching && web && <p className="py-6 text-center text-sm text-muted-foreground">{fetchedAt ? n.empty : n.notYet}</p>
      )}
    </SectionCard>
  )
}
