// View an older version and compare it with the current page (FR-KN-ED-5).
import { useState } from 'react'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { fullDate } from '@/lib/format'
import type { Page, PageVersion } from '@/api/knowledge'
import { DiffRow } from '@/components/shared/PageDiff'
import { lineDiff } from '@/lib/lineDiff'

export function HistoryDialog({ version, page, onClose, onRestore }: { version: PageVersion | null; page: Page; onClose: () => void; onRestore: (v: PageVersion) => void }) {
  const [view, setView] = useState<'version' | 'diff'>('diff')
  const diff = version ? lineDiff(version.snapshot.body, page.body) : []
  const changed = diff.some((d) => d.type !== 'same')
  const h = useT().knowledge.history
  return (
    <Dialog open={!!version} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-hidden sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{h.versionFrom(version ? fullDate(version.date) : '')}</DialogTitle>
          <DialogDescription>{view === 'diff' ? h.diffHint : h.versionHint}</DialogDescription>
        </DialogHeader>
        <div className="flex gap-2">
          <Button size="sm" variant={view === 'diff' ? 'secondary' : 'outline'} onClick={() => setView('diff')}>
            {h.changes}
          </Button>
          <Button size="sm" variant={view === 'version' ? 'secondary' : 'outline'} onClick={() => setView('version')}>
            {h.thatVersion}
          </Button>
          <Button size="sm" variant="outline" className="ml-auto" onClick={() => version && onRestore(version)}>
            {h.restoreVersion}
          </Button>
        </div>
        <div className="min-h-0 overflow-y-auto rounded-xl border bg-background p-4">
          {version && view === 'version' && (
            <>
              <h3 className="mb-3 text-lg font-semibold">{version.snapshot.title}</h3>
              <MarkdownView text={version.snapshot.body} />
            </>
          )}
          {version && view === 'diff' &&
            (changed ? (
              <pre className="font-mono text-xs leading-6 whitespace-pre-wrap">
                {diff.map((d, i) => (
                  <DiffRow key={i} line={d} />
                ))}
              </pre>
            ) : (
              <p className="py-6 text-center text-sm text-muted-foreground">{h.noDifferences}</p>
            ))}
        </div>
      </DialogContent>
    </Dialog>
  )
}
