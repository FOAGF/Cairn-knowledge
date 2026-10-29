// /knowledge/new: chooser, upload, task, review and "run checks" entry points.
import { ClipboardList, FilePenLine, FileUp, Plus } from 'lucide-react'
import { Navigate, useNavigate, useSearchParams } from 'react-router'
import { EmptyState, PageHeader } from '@/components/shared/PageHeader'
import { TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import type { TintColor } from '@/lib/types'
import { useAppStore } from '@/store'
import { REVIEW_PATH } from '@/store/creation'
import { openAsPending } from './pending'
import { CheckScreen } from './CheckScreen'
import { ReviewScreen } from './ReviewScreen'
import { TaskPicker, TaskStep } from './TaskStep'
import { useCategoryList } from '@/api/categories'
import { useT } from '@/i18n'

function Option({ icon, tint, title, text, onClick }: { icon: typeof Plus; tint: TintColor; title: string; text: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className="group flex flex-col items-start justify-start rounded-2xl border bg-card p-6 text-left shadow-[0_1px_2px_rgba(16,24,40,0.04)] transition-shadow hover:shadow-md">
      <TintIcon icon={icon} tint={tint} size="lg" className="mb-4" />
      <h2 className="text-base font-semibold">{title}</h2>
      <p className="mt-1 text-sm text-muted-foreground">{text}</p>
    </button>
  )
}

export default function NewPagePage() {
  const [params, setParams] = useSearchParams()
  const navigate = useNavigate()
  const draft = useAppStore((s) => s.draft)
  const categories = useCategoryList()
  const t = useT()

  const check = params.get('check')
  const fromTask = params.get('fromTask')
  const mode = params.get('mode')

  if (check) return <CheckScreen target={check} />
  if (fromTask) return <TaskStep key={fromTask} taskKey={fromTask} />
  if (mode === 'file') return <Navigate to="/assistant?attach=1" replace />
  if (mode === 'task') return <TaskPicker />
  if (params.get('draft')) {
    if (!draft)
      return (
        <div className="mx-auto max-w-xl pt-10">
          <EmptyState icon={FilePenLine} title={t.creation.newPage.noDraft}>
            {t.creation.newPage.noDraftHint}
            <div className="mt-4"><Button onClick={() => navigate('/knowledge/new')}>{t.creation.newPage.back}</Button></div>
          </EmptyState>
        </div>
      )
    return <ReviewScreen />
  }

  function writeManually() {
    // Manual draft: goes through the same review screen (steps 2-3 always run). It becomes the one
    // pending page, so another unsaved one is asked about first.
    void openAsPending({ source: 'manual', categoryId: categories.find((p) => p.isGeneral)?.id ?? categories[0]?.id ?? '', title: '', body: '', tags: [] }, REVIEW_PATH, navigate)
  }

  return (
    <div className="mx-auto max-w-4xl">
      <PageHeader icon={Plus} title={t.creation.newPage.title} description={t.creation.newPage.description} />
      <div className="grid gap-4 md:grid-cols-3">
        <Option icon={FilePenLine} tint="blue" title={t.creation.newPage.manual} text={t.creation.newPage.manualText} onClick={writeManually} />
        <Option icon={FileUp} tint="purple" title={t.creation.newPage.file} text={t.creation.newPage.fileText} onClick={() => navigate('/assistant?attach=1')} />
        <Option icon={ClipboardList} tint="green" title={t.creation.newPage.task} text={t.creation.newPage.taskText} onClick={() => setParams({ mode: 'task' })} />
      </div>
    </div>
  )
}
