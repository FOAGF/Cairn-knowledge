import { format } from 'date-fns'
import { formatDate } from '@/lib/format'
import { Globe } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useLang } from '@/i18n'

/** "UTC+10:00" for the browser's current offset (follows daylight saving). */
export function utcOffsetLabel(d: Date): string {
  const mins = -d.getTimezoneOffset()
  const sign = mins >= 0 ? '+' : '-'
  const abs = Math.abs(mins)
  return `UTC${sign}${String(Math.floor(abs / 60)).padStart(2, '0')}:${String(abs % 60).padStart(2, '0')}`
}

/** Home's large local clock with seconds, then the date and time zone (FR-HOME-5). Re-renders only itself. */
export function LocalClock() {
  useLang() // the date is in the interface language
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 1000)
    return () => clearInterval(id)
  }, [])
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone

  return (
    <div className="flex flex-col items-center gap-3">
      <p className="text-[5rem] leading-none font-semibold tracking-[-0.035em] tabular-nums" aria-live="off">
        {format(now, 'HH:mm')}
        <span className="ml-2 text-4xl font-medium tracking-tight text-muted-foreground">{format(now, 'ss')}</span>
      </p>
      <div className="flex flex-wrap items-center justify-center gap-x-2.5 gap-y-1 text-[15px] text-muted-foreground">
        <span>{formatDate(now, 'longDate')}</span>
        <span className="inline-flex items-center gap-1 rounded-full bg-tint-blue px-2.5 py-0.5 text-xs font-semibold text-tint-blue-fg tabular-nums">
          <Globe className="size-3.5" />
          {utcOffsetLabel(now)}
        </span>
        <span title={zone}>{zone.replace(/_/g, ' ')}</span>
      </div>
    </div>
  )
}
