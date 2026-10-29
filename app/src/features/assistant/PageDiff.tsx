// The line diff of a proposed page change (FR-LLM-AS-3, FR-KN-ED-5): old body against new body,
// the same look as the page history. Collapsed it shows the changed lines with one line around
// each; *Show the whole page* shows every line.
import { ChevronDown } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { lineDiff } from '../knowledge/utils'

const CONTEXT = 1

export function PageDiff({ oldBody, newBody }: { oldBody: string; newBody: string }) {
  const [whole, setWhole] = useState(false)
  const diff = useMemo(() => lineDiff(oldBody, newBody), [oldBody, newBody])
  const changed = diff.some((d) => d.type !== 'same')
  const t = useT()
  if (!changed) return <p className="text-sm text-muted-foreground">{t.assistant.diff.none}</p>

  // Which lines are shown when collapsed: changed ones and their neighbours.
  const near = diff.map((_, i) => diff.slice(Math.max(0, i - CONTEXT), i + CONTEXT + 1).some((d) => d.type !== 'same'))
  const rows: { line: (typeof diff)[number]; gap: boolean }[] = []
  diff.forEach((line, i) => {
    if (whole || near[i]) rows.push({ line, gap: !whole && i > 0 && !near[i - 1] })
  })

  return (
    <div>
      <pre className="max-h-72 overflow-y-auto rounded-xl border bg-background py-1.5 font-mono text-xs leading-6 whitespace-pre-wrap">
        {rows.map(({ line, gap }, i) => (
          <div key={i}>
            {gap && <div className="px-2 text-muted-foreground">…</div>}
            <div
              className={cn(
                'px-2',
                line.type === 'add' && 'bg-tint-green text-tint-green-fg',
                line.type === 'del' && 'bg-tint-rose text-tint-rose-fg line-through decoration-1',
                line.type === 'same' && 'text-muted-foreground',
              )}
            >
              {line.type === 'add' ? '+ ' : line.type === 'del' ? '- ' : '  '}
              {line.text || ' '}
            </div>
          </div>
        ))}
      </pre>
      <button type="button" onClick={() => setWhole(!whole)} className="mt-1.5 inline-flex items-center gap-1 text-xs font-medium text-primary hover:underline">
        <ChevronDown className={cn('size-3.5 transition-transform', whole && 'rotate-180')} />
        {whole ? t.assistant.diff.onlyChanges : t.assistant.diff.wholePage}
      </button>
    </div>
  )
}
