// Home (FR-HOME-1..5, D-54): one Rust call gives everything Home shows, so Home never loads every
// task or page.
//
//   const overview = useHomeOverview().data   // counts, the Today groups, due counts, hasData
//
// The query key starts with 'home': client.ts fetches it again whenever tasks, pages, categories or
// conversations change. The local date is part of the key, so it is fetched again after midnight.
import { useQuery } from '@tanstack/react-query'
import { format } from 'date-fns'
import { useEffect, useState } from 'react'
import { commands, type HomeCounts, type HomeOverview, type TodayGroup, type TodayGroupKind } from './bindings'

export type { HomeCounts, HomeOverview, TodayGroup, TodayGroupKind }

const dayKey = () => format(new Date(), 'yyyy-MM-dd')

/** Today's local date as `YYYY-MM-DD`; it changes when the day does (checked every minute). */
export function useLocalDay(): string {
  const [day, setDay] = useState(dayKey)
  useEffect(() => {
    const id = setInterval(() => setDay(dayKey()), 60_000)
    return () => clearInterval(id)
  }, [])
  return day
}

export function useHomeOverview() {
  const day = useLocalDay()
  return useQuery({ queryKey: ['home', day], queryFn: () => commands.homeOverview() })
}
