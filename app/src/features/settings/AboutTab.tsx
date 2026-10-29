// Settings › About (UI-SET-5, NFR-SEC-2): the build's real version, when it was built (UTC), the
// commit it was built from and the database schema version.
import { Info } from 'lucide-react'
import { useAppInfo } from '@/api/app'
import { SectionCard } from '@/components/shared/PageHeader'
import { useT } from '@/i18n'

/** "2026-10-20T03:12:45Z" → "2026-10-20 03:12 UTC". */
const utc = (iso: string) => `${iso.slice(0, 10)} ${iso.slice(11, 16)} UTC`

export function AboutTab() {
  const info = useAppInfo().data
  const a = useT().settings.about
  const unknown = info ? a.unknown : ''
  const rows: { label: string; value: React.ReactNode; testId: string }[] = [
    { label: a.version, value: info ? `${info.version}${import.meta.env.DEV ? a.devBuild : ''}` : '', testId: 'about-version' },
    { label: a.built, value: info?.buildTime ? utc(info.buildTime) : unknown, testId: 'about-built' },
    {
      label: a.commit,
      value: info?.commit ? (
        <>
          {/* One click selects the whole SHA, for copying. */}
          <span className="font-mono text-xs select-all">{info.commit}</span>
          {info.localChanges && <span className="text-muted-foreground">{a.localChanges}</span>}
        </>
      ) : (
        unknown
      ),
      testId: 'about-commit',
    },
    { label: a.schema, value: info ? a.schemaVersion(info.schemaVersion) : '', testId: 'about-schema' },
  ]
  return (
    <SectionCard icon={Info} tint="slate" title="Cairn">
      <dl className="grid grid-cols-[max-content_1fr] items-baseline gap-x-8 gap-y-2.5 text-sm">
        {rows.map((r) => (
          <div key={r.label} className="contents">
            <dt className="text-muted-foreground">{r.label}</dt>
            <dd className="min-w-0 break-all tabular-nums" data-testid={r.testId}>
              {r.value}
            </dd>
          </div>
        ))}
      </dl>
    </SectionCard>
  )
}
