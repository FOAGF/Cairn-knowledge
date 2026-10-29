// FR-RAG-EM-3, D-82: the search model on Home, past the first start (the first-start card has its
// own note). While a download or import runs, or after one failed, a one-line progress; otherwise,
// on a copy without the model, a one-time notice offering the download, dismissible for good.
import { ScanSearch } from 'lucide-react'
import { aboutMB, useDismissSearchModelNotice, useSearchModelStatus } from '@/api/searchModel'
import { SectionCard } from '@/components/shared/PageHeader'
import { SearchModelDownload, SearchModelTaskLine } from '@/components/shared/SearchModel'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { useFirstStart } from './firstStart'

export function SearchModelNotice() {
  const status = useSearchModelStatus()
  const firstStart = useFirstStart()
  const dismiss = useDismissSearchModelNotice()
  const m = useT().searchModel
  if (!status || firstStart !== false) return null
  if (status.task) return <SearchModelTaskLine className="rounded-xl border bg-card px-3.5 py-2.5" />
  if (status.install !== 'notInstalled' || status.noticeDismissed) return null
  return (
    <SectionCard
      icon={ScanSearch}
      tint="purple"
      title={m.noticeTitle}
      description={m.noticeText(aboutMB(status.sizeBytes))}
      action={
        <Button variant="ghost" size="sm" disabled={dismiss.isPending} onClick={() => dismiss.mutate()}>
          {m.noticeDismiss}
        </Button>
      }
    >
      <SearchModelDownload status={status} label={m.downloadInBackground} />
    </SectionCard>
  )
}
