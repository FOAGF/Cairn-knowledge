// Right sidebar of the page view: properties, linked tasks, mentions, history, actions.
import { History, Link2, MessageSquare, Plus, X } from 'lucide-react'
import { memo, useState } from 'react'
import { Link } from 'react-router'
import { toast } from 'sonner'
import { SectionCard } from '@/components/shared/PageHeader'
import { CategoryDot, StatusBadge } from '@/components/shared/tint'
import { Button } from '@/components/ui/button'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useT } from '@/i18n'
import { fullDate } from '@/lib/format'
import {
  type Page,
  useBacklinks,
  useCreateGroup,
  useGroups,
  useLinkTaskPage,
  useMovePage,
  usePageTags,
  usePageVersions,
  useRestorePageVersion,
  useTaskPageLinks,
  useUnlinkTaskPage,
  useUpdatePage,
} from '@/api/knowledge'
import { GroupNameDialog } from './GroupDialogs'
import { groupOptions } from './groupTree'
import { HistoryDialog } from './HistoryDialog'
import { TagInput } from './TagInput'
import { useCategoryList } from '@/api/categories'
import { ALL_TASKS, useTaskList } from '@/api/tasks'

const NONE = '__none'
const NEW_GROUP = '__new'

function Prop({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <p className="text-xs font-medium text-muted-foreground">{label}</p>
      {children}
    </div>
  )
}

// Memoised: the page editor re-renders on every key, and a re-render of the sidebar's text fields makes
// WebKitGTK repaint the whole window (see `followHeight` in MarkdownEditor.tsx).
export const PageSidebar = memo(function PageSidebar({ page }: { page: Page }) {
  const categories = useCategoryList()
  const groups = useGroups(page.categoryId)
  const allTags = usePageTags()
  const tasks = useTaskList(ALL_TASKS)
  const myLinks = useTaskPageLinks({ pageId: page.id })
  const backlinks = useBacklinks('page', page.id)
  const versions = [...usePageVersions(page.id)].reverse() // newest first
  const { mutate: updatePage } = useUpdatePage()
  const { mutate: movePage } = useMovePage()
  const { mutateAsync: createGroup } = useCreateGroup()
  const { mutate: linkTaskPage } = useLinkTaskPage()
  const { mutate: unlinkTaskPage } = useUnlinkTaskPage()
  const { mutate: restoreVersion } = useRestorePageVersion()
  const [newGroup, setNewGroup] = useState(false)
  const [versionDate, setVersionDate] = useState<string | null>(null)
  const [pickerOpen, setPickerOpen] = useState(false)
  const t = useT()
  const k = t.knowledge
  const sb = k.sidebar

  const linkedTaskIds = new Set(myLinks.map((l) => l.taskId))
  const version = versions.find((v) => v.date === versionDate) ?? null

  return (
    <div className="space-y-4">
      {/* FR-KN-GR-4: move between groups at any time */}
      <SectionCard title={sb.properties}>
        <div className="space-y-4">
          <Prop label={t.common.category}>
            <Select value={page.categoryId} onValueChange={(v) => movePage({ id: page.id, categoryId: v })}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {categories.filter((p) => p.status === 'active' || p.id === page.categoryId).map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    <CategoryDot color={p.color} className="size-2" /> {p.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Prop>
          <Prop label={t.common.group}>
            <Select
              value={page.groupId && groups.some((g) => g.id === page.groupId) ? page.groupId : NONE}
              onValueChange={(v) => (v === NEW_GROUP ? setNewGroup(true) : updatePage({ id: page.id, patch: { groupId: v === NONE ? '' : v } }))}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>{k.ungrouped}</SelectItem>
                {groupOptions(groups).map(({ group: g, label }) => (
                  <SelectItem key={g.id} value={g.id}>
                    {label}
                  </SelectItem>
                ))}
                <SelectItem value={NEW_GROUP}>{sb.newGroupItem}</SelectItem>
              </SelectContent>
            </Select>
          </Prop>
          <Prop label={t.common.tags}>
            <div className="rounded-lg border px-2 py-1.5">
              <TagInput tags={page.tags} suggestions={allTags} onChange={(tags) => updatePage({ id: page.id, patch: { tags } })} />
            </div>
          </Prop>
        </div>
      </SectionCard>

      {/* DM-5 */}
      <SectionCard
        title={sb.linkedTasks}
        action={
          <Popover open={pickerOpen} onOpenChange={setPickerOpen}>
            <PopoverTrigger asChild>
              <Button variant="outline" size="sm">
                <Plus /> {t.common.add}
              </Button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-80 p-0">
              <Command>
                <CommandInput placeholder={sb.findTask} />
                <CommandList>
                  <CommandEmpty>{sb.noTasksFound}</CommandEmpty>
                  {tasks
                    .filter((x) => !linkedTaskIds.has(x.id))
                    .map((x) => (
                      <CommandItem
                        key={x.id}
                        value={`${x.key} ${x.title}`}
                        onSelect={() => {
                          linkTaskPage({ taskId: x.id, pageId: page.id })
                          setPickerOpen(false)
                        }}
                      >
                        <span className="font-mono text-xs text-muted-foreground">{x.key}</span>
                        <span className="truncate">{x.title}</span>
                      </CommandItem>
                    ))}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        }
      >
        {myLinks.length === 0 ? (
          <p className="text-sm text-muted-foreground">{sb.noLinkedTasks}</p>
        ) : (
          <ul className="space-y-2">
            {myLinks.map((l) => {
              const task = tasks.find((x) => x.id === l.taskId)
              if (!task) return null
              return (
                <li key={l.id} className="group flex items-center gap-2">
                  <Link to={`/tasks/${task.key}`} className="min-w-0 flex-1 rounded-lg hover:bg-muted">
                    <p className="truncate text-sm font-medium">
                      <span className="mr-1.5 font-mono text-xs text-muted-foreground">{task.key}</span>
                      {task.title}
                    </p>
                    <StatusBadge status={task.status} className="mt-1" />
                  </Link>
                  <Button variant="ghost" size="icon-xs" aria-label={sb.unlink(task.key)} onClick={() => unlinkTaskPage(l.id)}>
                    <X />
                  </Button>
                </li>
              )
            })}
          </ul>
        )}
      </SectionCard>

      <SectionCard title={sb.mentionedIn} description={sb.mentionedInDescription}>
        {backlinks.length === 0 ? (
          <p className="text-sm text-muted-foreground">{sb.noMentions}</p>
        ) : (
          <ul className="space-y-1.5 text-sm">
            {backlinks.map((b) => (
              <li key={`${b.source}:${b.sourceId}`}>
                <Link to={b.pageId ? `/knowledge/${b.pageId}` : `/tasks/${b.taskId}`} className="flex items-center gap-2 hover:text-primary">
                  {b.source === 'comment' ? <MessageSquare className="size-3.5 shrink-0 text-muted-foreground" /> : <Link2 className="size-3.5 shrink-0 text-muted-foreground" />}
                  <span className="truncate">{b.label}</span>
                  {b.source === 'comment' && <span className="shrink-0 text-xs text-muted-foreground">{sb.comment}</span>}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </SectionCard>

      {/* FR-KN-ED-5 */}
      <SectionCard title={sb.history} description={sb.historyDescription}>
        {versions.length === 0 ? (
          <p className="text-sm text-muted-foreground">{sb.noVersions}</p>
        ) : (
          <ul className="space-y-1">
            {versions.map((v) => (
              <li key={v.date}>
                <button onClick={() => setVersionDate(v.date)} className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm hover:bg-muted">
                  <History className="size-4 text-muted-foreground" />
                  {fullDate(v.date)}
                </button>
              </li>
            ))}
          </ul>
        )}
      </SectionCard>


      <GroupNameDialog
        open={newGroup}
        title={k.newGroup}
        onClose={() => setNewGroup(false)}
        onSubmit={(name) => {
          void createGroup({ categoryId: page.categoryId, name }).then((g) => updatePage({ id: page.id, patch: { groupId: g.id } }))
          setNewGroup(false)
        }}
      />
      <HistoryDialog
        version={version}
        page={page}
        onClose={() => setVersionDate(null)}
        onRestore={(v) => {
          restoreVersion(
            { pageId: page.id, date: v.date },
            { onSuccess: () => toast.success(sb.restored(fullDate(v.date))) },
          )
          setVersionDate(null)
        }}
      />
    </div>
  )
})
