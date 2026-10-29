import {
  ArrowLeft,
  Check,
  CircleCheckBig,
  Clock,
  FileText,
  FolderKanban,
  Lock,
  Pencil,
  SendHorizontal,
  Sparkles,
} from 'lucide-react'
import { useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router'
import { SectionCard } from '@/components/shared/PageHeader'
import { ColorPicker } from '@/components/shared/ColorPicker'
import { CategoryIcon, TintIcon } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useT } from '@/i18n'
import { timeAgo } from '@/lib/format'
import type { ActivityKind } from '@/api/bindings'
import { type TaskFilter, useActivity, useTaskList } from '@/api/tasks'
import { usePageList } from '@/api/knowledge'
import type { CategoryColor, TintColor } from '@/lib/types'
import { cn } from '@/lib/utils'
import { CategoryKnowledgeSection } from './CategoryKnowledgeSection'
import { CategoryTasksSection } from './CategoryTasksSection'
import { errorMessage, isApiError } from '@/api/client'
import { type Category, useCategory, useUpdateCategory } from '@/api/categories'


function EditCategoryDialog({ category, open, onClose }: { category: Category; open: boolean; onClose: () => void }) {
  const updateCategory = useUpdateCategory()
  const [name, setName] = useState(category.name)
  const [description, setDescription] = useState(category.description)
  const [color, setColor] = useState<CategoryColor>(category.color)
  const t = useT()
  const p = t.categories.page
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{p.editCategory}</DialogTitle>
          <DialogDescription>{p.keyFixed(category.key)}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="pp-name">{t.common.name}</Label>
            <Input id="pp-name" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="pp-desc">{t.common.description}</Label>
            <Textarea id="pp-desc" rows={3} value={description} onChange={(e) => setDescription(e.target.value)} />
            <p className="text-xs text-muted-foreground">{p.descriptionHint}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{t.categories.dialog.colour}</Label>
            <ColorPicker value={color} onChange={setColor} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button
            disabled={!name.trim() || updateCategory.isPending}
            onClick={() => {
              updateCategory.mutate({ id: category.id, changes: { name: name.trim(), description: description.trim(), color } }, { onSuccess: onClose })
            }}
          >
            {t.common.save}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Description with inline edit. */
function Description({ category }: { category: Category }) {
  const updateCategory = useUpdateCategory()
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const t = useT()
  const p = t.categories.page
  if (editing)
    return (
      <div className="mt-3 max-w-3xl space-y-2">
        <Textarea autoFocus rows={3} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={p.descriptionPlaceholder} />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            disabled={updateCategory.isPending}
            onClick={() => {
              updateCategory.mutate({ id: category.id, changes: { description: draft.trim() } }, { onSuccess: () => setEditing(false) })
            }}
          >
            <Check /> {t.common.save}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
            {t.common.cancel}
          </Button>
        </div>
      </div>
    )
  return (
    <div className="mt-3 max-w-3xl">
      <p className={cn('text-base leading-relaxed', category.description ? 'text-foreground/85' : 'text-muted-foreground italic')}>
        {category.description || p.noDescription}
      </p>
      <div className="mt-1.5 flex flex-wrap items-center gap-x-3 text-xs text-muted-foreground">
        <button
          className="inline-flex items-center gap-1 font-medium text-primary hover:underline"
          onClick={() => {
            setDraft(category.description)
            setEditing(true)
          }}
        >
          <Pencil className="size-3" /> {t.common.edit}
        </button>
        <span>{p.descriptionNote}</span>
      </div>
    </div>
  )
}

interface Change {
  id: string
  icon: typeof FileText
  tint: TintColor
  /** What happened, named in the interface language when shown. */
  kind: ActivityKind
  title: string
  to: string
  at: string
}

/** Latest changes in this category: activity events, task/page updates, comments. */
function RecentActivity({ category }: { category: Category }) {
  const filter = useMemo<TaskFilter>(() => ({ categoryIds: [category.id], includeClosed: true }), [category.id])
  const myTasks = useTaskList(filter)
  const pageFilter = useMemo(() => ({ categoryIds: [category.id], includeArchived: true }), [category.id])
  const myPages = usePageList(pageFilter)
  const activity = useActivity(200)
  const a = useT().categories.activity

  const items = useMemo(() => {
    // One entry per task/page: the newest thing that happened to it.
    const byRef = new Map<string, Change>()
    const put = (c: Change, key: string) => {
      const old = byRef.get(key)
      if (!old || c.at > old.at) byRef.set(key, c)
    }
    for (const t of myTasks) {
      const base = { title: t.title, to: `/tasks/${t.key}`, icon: CircleCheckBig }
      put({ ...base, id: `tu-${t.id}`, tint: 'blue', kind: 'task_updated', at: t.updatedAt }, `t-${t.id}`)
    }
    for (const p of myPages) {
      put({ id: `pu-${p.id}`, icon: FileText, tint: 'blue', kind: 'page_updated', title: p.title, to: `/knowledge/${p.id}`, at: p.updatedAt }, `p-${p.id}`)
    }
    for (const e of activity) {
      const isPage = e.kind.startsWith('page')
      const t = isPage ? undefined : myTasks.find((x) => x.id === e.refId)
      const p = isPage ? myPages.find((x) => x.id === e.refId) : undefined
      if (!t && !p) continue
      const tint: TintColor = e.kind === 'task_completed' ? 'green' : isPage ? (e.kind === 'page_created' ? 'purple' : 'blue') : 'blue'
      put(
        {
          id: e.id,
          icon: isPage ? FileText : CircleCheckBig,
          tint,
          kind: e.kind,
          title: (t?.title ?? p?.title) as string,
          to: t ? `/tasks/${t.key}` : `/knowledge/${p!.id}`,
          at: e.at,
        },
        `${isPage ? 'p' : 't'}-${e.refId}`,
      )
    }
    const list = [...byRef.values()]
    return list.sort((a, b) => b.at.localeCompare(a.at)).slice(0, 5)
  }, [myTasks, myPages, activity, category.id])

  return (
    <SectionCard icon={Clock} tint="slate" title={a.title} description={a.description}>
      {items.length === 0 ? (
        <p className="py-4 text-center text-sm text-muted-foreground">{a.empty}</p>
      ) : (
        <ul className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-5">
          {items.map((e) => (
            <li key={e.id}>
              <Link to={e.to} className="flex items-start gap-3">
                <TintIcon icon={e.icon} tint={e.tint} />
                <div className="min-w-0">
                  <p className="truncate text-xs text-muted-foreground">{a[e.kind]}</p>
                  <p className="truncate text-sm font-medium">{e.title}</p>
                  <p className="text-xs text-muted-foreground">{timeAgo(e.at)}</p>
                </div>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </SectionCard>
  )
}

function ScopedAskBar({ category }: { category: Category }) {
  const navigate = useNavigate()
  const [q, setQ] = useState('')
  const p = useT().categories.page
  const submit = () => {
    if (!q.trim()) return
    navigate(`/assistant?q=${encodeURIComponent(q.trim())}&scope=${encodeURIComponent(category.id)}`)
    setQ('')
  }
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault()
        submit()
      }}
      className="flex h-14 items-center gap-3 rounded-2xl border bg-card px-4 shadow-[0_1px_3px_rgba(16,24,40,0.05)] focus-within:ring-2 focus-within:ring-ring/40"
    >
      <Sparkles className="size-5 shrink-0 text-primary" />
      <span className="h-6 w-px bg-border" />
      <input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={p.askPlaceholder(category.name)}
        className="h-full min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
      />
      <button
        type="submit"
        disabled={!q.trim()}
        aria-label={p.ask}
        className="flex size-9 items-center justify-center rounded-full bg-primary text-primary-foreground transition-opacity hover:opacity-90 disabled:opacity-40"
      >
        <SendHorizontal className="size-4" />
      </button>
    </form>
  )
}

function BackLink() {
  const t = useT()
  return (
    <Link to="/categories" className="mb-4 inline-flex items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
      <ArrowLeft className="size-4" /> {t.common.nav.categories}
    </Link>
  )
}

export default function CategoryPage() {
  const { categoryId } = useParams()
  const { data: category, error, isPending } = useCategory(categoryId)
  const [editOpen, setEditOpen] = useState(false)
  const t = useT()
  const p = t.categories.page

  if (isPending) return null
  if (!category) {
    const notFound = isApiError(error) && error.kind === 'not_found'
    return (
      <div>
        <BackLink />
        <div className="mx-auto mt-10 flex max-w-md flex-col items-center rounded-2xl border bg-card p-10 text-center">
          <TintIcon icon={FolderKanban} tint="slate" size="lg" />
          <h1 className="mt-4 text-xl font-semibold">{notFound ? p.notFound : p.loadFailed}</h1>
          <p className="mt-1 text-sm text-muted-foreground">{notFound || !error ? p.notFoundHint : errorMessage(error)}</p>
          <Button asChild className="mt-5">
            <Link to="/categories">{p.backToCategories}</Link>
          </Button>
        </div>
      </div>
    )
  }


  return (
    <div className="space-y-6">
      <div>
        <BackLink />
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="flex min-w-0 items-start gap-4">
            <CategoryIcon icon={FolderKanban} color={category.color} size="lg" className="size-16 rounded-3xl [&>svg]:size-8" />
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2.5">
                <h1 className="text-3xl font-semibold tracking-tight">{category.name}</h1>
                <span className="rounded-md bg-muted px-2 py-0.5 font-mono text-sm text-muted-foreground">{category.key}</span>
                {category.isGeneral && (
                  <span title={t.categories.card.builtIn} className="text-muted-foreground">
                    <Lock className="size-4" />
                  </span>
                )}
                {category.status === 'archived' && <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{p.archived}</span>}
              </div>
              <Description category={category} />
            </div>
          </div>
          <Button variant="outline" onClick={() => setEditOpen(true)}>
            <Pencil /> {p.editCategory}
          </Button>
        </div>
      </div>

      <ScopedAskBar category={category} />

      <div className="grid items-start gap-6 xl:grid-cols-[minmax(0,1.35fr)_minmax(0,1fr)]">
        <CategoryTasksSection category={category} />
        <CategoryKnowledgeSection category={category} />
      </div>

      <RecentActivity category={category} />

      <EditCategoryDialog key={String(editOpen)} category={category} open={editOpen} onClose={() => setEditOpen(false)} />
    </div>
  )
}
