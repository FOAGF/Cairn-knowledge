import { Archive, ArchiveRestore, BookOpen, CircleCheckBig, FolderKanban, Lock, MoreHorizontal, Pencil, Plus, TriangleAlert } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { errorMessage } from '@/api/client'
import { useArchiveCategory, useCreateCategory, useCategoryList, useCategories, useSuggestCategoryKey, useUpdateCategory } from '@/api/categories'
import { PageHeader } from '@/components/shared/PageHeader'
import { ColorPicker } from '@/components/shared/ColorPicker'
import { CategoryIcon, categoryTint } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useT } from '@/i18n'
import { isClosed } from '@/lib/labels'
import type { Category } from '@/api/bindings'
import { type TaskFilter, useTaskList } from '@/api/tasks'
import type { CategoryColor } from '@/lib/types'
import { cn } from '@/lib/utils'
import { useGroups, usePageList } from '@/api/knowledge'

const EMPTY: Category[] = []

type FormState = { id?: string; name: string; key: string; description: string; color: CategoryColor }

function CategoryDialog({ initial, onClose }: { initial: FormState | null; onClose: () => void }) {
  const createCategory = useCreateCategory()
  const updateCategory = useUpdateCategory()
  const suggestKey = useSuggestCategoryKey()
  const categories = useCategoryList()
  // Once the user types their own key, stop suggesting one from the name.
  const keyTouched = useRef(false)
  const suggestSeq = useRef(0)
  const [form, setForm] = useState<FormState>(initial ?? { name: '', key: '', description: '', color: 'blue' })
  const editing = Boolean(initial?.id)
  const t = useT()
  const d = t.categories.dialog

  const keyTaken = categories.some((p) => p.key === form.key && p.id !== form.id)
  const valid = form.name.trim() && /^[A-Z][A-Z0-9]{1,5}$/.test(form.key) && !keyTaken

  const saving = createCategory.isPending || updateCategory.isPending
  // Failed saves show a toast (see api/client.ts); the dialog stays open so nothing typed is lost.
  const save = async () => {
    if (editing) {
      await updateCategory.mutateAsync({ id: form.id!, changes: { name: form.name, description: form.description, color: form.color } })
      toast.success(d.updated)
    } else {
      await createCategory.mutateAsync({ name: form.name, key: form.key, description: form.description, color: form.color })
      toast.success(d.created(form.name))
    }
    onClose()
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{editing ? d.editTitle : d.newTitle}</DialogTitle>
          <DialogDescription>{d.description}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="grid grid-cols-[1fr_7rem] gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="pname">{t.common.name}</Label>
              <Input
                id="pname"
                value={form.name}
                onChange={(e) => {
                  const name = e.target.value
                  setForm((f) => ({ ...f, name }))
                  // Suggest a free key from the name while creating (FR-PRJ-6).
                  if (!editing && !keyTouched.current) {
                    const seq = ++suggestSeq.current
                    suggestKey.mutate(name, {
                      onSuccess: (key) => {
                        if (seq === suggestSeq.current && !keyTouched.current) setForm((f) => ({ ...f, key: name.trim() ? key : '' }))
                      },
                    })
                  }
                }}
                autoFocus
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="pkey">{d.taskKey}</Label>
              <Input
                id="pkey"
                value={form.key}
                disabled={editing}
                onChange={(e) => {
                  keyTouched.current = true
                  setForm((f) => ({ ...f, key: e.target.value.toUpperCase() }))
                }}
                maxLength={6}
                className="font-mono"
              />
            </div>
          </div>
          {form.key && (
            <p className={cn('-mt-2 text-xs', keyTaken ? 'text-destructive' : 'text-muted-foreground')}>
              {keyTaken ? d.keyTaken : d.keyNumbering(form.key)}
              {editing && d.keyFixed}
            </p>
          )}
          <div className="space-y-1.5">
            <Label htmlFor="pdesc">{t.common.description}</Label>
            <Textarea
              id="pdesc"
              rows={3}
              value={form.description}
              placeholder={d.descriptionPlaceholder}
              onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))}
            />
            <p className="text-xs text-muted-foreground">{d.descriptionHint}</p>
          </div>
          <div className="space-y-1.5">
            <Label>{d.colour}</Label>
            <ColorPicker value={form.color} onChange={(color) => setForm((f) => ({ ...f, color }))} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button onClick={() => void save().catch(() => {})} disabled={!valid || saving}>
            {editing ? t.common.save : d.create}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function CategoryCard({ category, onEdit }: { category: Category; onEdit: () => void }) {
  const filter = useMemo<TaskFilter>(() => ({ categoryIds: [category.id], includeClosed: true }), [category.id])
  const tasks = useTaskList(filter)
  const pageFilter = useMemo(() => ({ categoryIds: [category.id] }), [category.id])
  const pages = usePageList(pageFilter)
  const groupCount = useGroups(category.id).length
  const archiveCategory = useArchiveCategory()
  const open = tasks.filter((t) => !isClosed(t.status))
  const blocked = open.filter((t) => t.status === 'blocked').length
  const done = tasks.filter((t) => t.status === 'done').length
  const progress = tasks.length ? Math.round((done / tasks.length) * 100) : 0
  const archived = category.status === 'archived'
  const t = useT()
  const c = t.categories.card

  return (
    <div className={cn('relative flex flex-col rounded-2xl border bg-card p-5 transition-shadow hover:shadow-md', archived && 'opacity-60')}>
      <div className="flex items-start justify-between">
        <CategoryIcon icon={FolderKanban} color={category.color} size="lg" />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="relative z-10 size-8">
              <MoreHorizontal className="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onSelect={onEdit}>
              <Pencil /> {t.common.edit}
            </DropdownMenuItem>
            {!category.isGeneral && (
              <DropdownMenuItem onSelect={() => archiveCategory.mutate({ id: category.id, archived: !archived })}>
                {archived ? <ArchiveRestore /> : <Archive />} {archived ? t.common.restore : t.common.archive}
              </DropdownMenuItem>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="mt-4 flex items-center gap-2">
        {/* The title links to the category page; the stretched ::after makes the whole card clickable. */}
        <Link to={`/categories/${category.id}`} className="text-lg font-semibold after:absolute after:inset-0 after:rounded-2xl hover:text-primary">
          {category.name}
        </Link>
        <span className="rounded-md bg-muted px-1.5 py-0.5 font-mono text-xs text-muted-foreground">{category.key}</span>
        {category.isGeneral && (
          <span title={c.builtIn} className="text-muted-foreground">
            <Lock className="size-3.5" />
          </span>
        )}
      </div>
      <p className="mt-1 line-clamp-2 min-h-10 text-sm text-muted-foreground">{category.description}</p>

      <div className="mt-4 grid grid-cols-3 gap-2 text-center">
        <Link to={`/tasks?category=${category.id}`} className="relative z-10 rounded-xl bg-muted/60 py-2 hover:bg-muted">
          <p className="text-lg font-semibold">{open.length}</p>
          <p className="flex items-center justify-center gap-1 text-xs text-muted-foreground">
            <CircleCheckBig className="size-3" /> {c.open}
          </p>
        </Link>
        <Link to={`/knowledge?category=${category.id}`} className="relative z-10 rounded-xl bg-muted/60 py-2 hover:bg-muted">
          <p className="text-lg font-semibold">{pages.length}</p>
          <p className="flex items-center justify-center gap-1 text-xs text-muted-foreground">
            <BookOpen className="size-3" /> {c.pages}
          </p>
        </Link>
        <div className="rounded-xl bg-muted/60 py-2">
          <p className={cn('text-lg font-semibold', blocked && 'text-tint-rose-fg')}>{blocked}</p>
          <p className="flex items-center justify-center gap-1 text-xs text-muted-foreground">
            <TriangleAlert className="size-3" /> {c.blocked}
          </p>
        </div>
      </div>

      <div className="mt-4">
        <div className="mb-1 flex justify-between text-xs text-muted-foreground">
          <span>{c.progress(done, tasks.length, groupCount)}</span>
          <span>{progress}%</span>
        </div>
        <div className="h-1.5 overflow-hidden rounded-full bg-muted">
          <div className={cn('h-full rounded-full', categoryTint(category.color).className)} style={{ ...categoryTint(category.color).style, width: `${progress}%`, backgroundColor: 'currentcolor' }} />
        </div>
      </div>
    </div>
  )
}

export default function CategoriesPage() {
  const { data: categories, error } = useCategories()
  const [dialog, setDialog] = useState<FormState | null | 'new'>(null)
  const [showArchived, setShowArchived] = useState(false)
  const visible = (categories ?? EMPTY).filter((p) => showArchived || p.status === 'active')
  const t = useT()
  const l = t.categories.list

  return (
    <div>
      <PageHeader
        icon={FolderKanban}
        tint="purple"
        title={t.common.nav.categories}
        description={l.description}
        actions={
          <>
            <label className="mr-2 flex items-center gap-2 text-sm text-muted-foreground">
              <Switch checked={showArchived} onCheckedChange={setShowArchived} /> {l.showArchived}
            </label>
            <Button onClick={() => setDialog('new')}>
              <Plus /> {l.newCategory}
            </Button>
          </>
        }
      />
      {error && <p className="mb-4 text-sm text-destructive">{l.loadFailed(errorMessage(error))}</p>}
      <div className="grid gap-5 sm:grid-cols-2 xl:grid-cols-3">
        {visible.map((p) => (
          <CategoryCard
            key={p.id}
            category={p}
            onEdit={() => setDialog({ id: p.id, name: p.name, key: p.key, description: p.description, color: p.color })}
          />
        ))}
      </div>
      {dialog && <CategoryDialog initial={dialog === 'new' ? null : dialog} onClose={() => setDialog(null)} />}
    </div>
  )
}
