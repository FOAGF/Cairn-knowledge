import { useEffect } from 'react'
import { Outlet } from 'react-router'
import { isTauri, useDataChangedSync } from '@/api/client'
import { useT } from '@/i18n'
import { useLlmDeltaSync } from '@/api/llm'
import { installDropGuard } from '@/lib/dropGuard'
import { installUiErrorLog } from '@/lib/uiErrors'
import { seedDevCategories } from '@/api/categories'
import type { KnowledgeSeed, TaskSeed } from '@/api/bindings'
import { seedDevKnowledge } from '@/api/knowledge'
import { seedDevGlossary } from '@/api/glossary'
import { glossarySeed } from '@/lib/glossary-sample'
import { installDraftCloseHook } from '@/api/drafts'
import { useSettingsQuery } from '@/api/settings'
import { useKnowledgeProgressSync } from '@/api/storage'
import { useSearchModelDoneToasts } from '@/api/searchModel'
import { seedDevTasks } from '@/api/tasks'
import * as mock from '@/lib/mock-data'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { PendingPageDialog } from '@/features/creation/PendingPage'
import { installPendingPage } from '@/features/creation/pending'
import { KnowledgeSwitchDialog } from '@/features/storage/KnowledgeSwitchDialog'
import { ModeChooser } from '@/features/storage/ModeChooser'
import { SwitchBackupWarning } from '@/features/storage/SwitchBackupWarning'
import { CommandPalette } from './CommandPalette'
import { LanguageSync } from './LanguageSync'
import { StartupNotices } from './StartupNotices'
import { ThemeSync } from './ThemeSync'
import { TopNav } from './TopNav'

/** `[[Page title]]` (prototype syntax) -> `[Page title](page:<id>)`, using the sample pages' ids (DM-MN-3). */
function pageMentions(text: string): string {
  return text.replace(/\[\[([^\]\n]+)\]\]/g, (m, title: string) => {
    const page = mock.pages.find((p) => p.title.toLowerCase() === title.trim().toLowerCase())
    return page ? `[${page.title}](page:${page.id})` : m
  })
}

/** The prototype's sample tasks in the generated types (`undefined` -> `null`, timestamps filled in). */
function taskSeed(): TaskSeed {
  const snap = (s: (typeof mock.taskHistory)[number]['snapshot']) => ({ ...s, description: pageMentions(s.description), dueDate: s.dueDate ?? null })
  return {
    tasks: mock.tasks.map((t) => ({ ...t, description: pageMentions(t.description), dueDate: t.dueDate ?? null, completedAt: t.completedAt ?? null })),
    links: mock.taskLinks.map((l) => ({ ...l, createdAt: mock.tasks.find((t) => t.id === l.fromTaskId)?.createdAt ?? new Date().toISOString() })),
    comments: mock.taskComments.map((c) => ({ ...c, text: pageMentions(c.text), updatedAt: c.updatedAt ?? null })),
    history: mock.taskHistory.map((v) => ({ ...v, snapshot: snap(v.snapshot) })),
    activity: mock.activity.filter((e) => e.kind.startsWith('task')) as TaskSeed['activity'],
  }
}

/** The prototype's sample knowledge in the generated types (`undefined` -> `null`, mentions by id). */
function knowledgeSeed(): KnowledgeSeed {
  const snap = (s: (typeof mock.pageHistory)[number]['snapshot']) => ({
    title: s.title,
    body: pageMentions(s.body),
    tags: s.tags,
    groupId: s.groupId ?? null,
    sourceInfo: s.sourceInfo ?? null,
  })
  return {
    groups: mock.groups.map((g) => ({
      ...g,
      description: g.description ?? '',
      parentId: null,
      path: null,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    })),
    pages: mock.pages.map((p) => ({
      ...p,
      body: pageMentions(p.body),
      groupId: p.groupId ?? null,
      sourceInfo: p.sourceInfo ?? null,
      originTaskId: p.originTaskId ?? null,
      archived: false,
      path: null,
      fileHash: null,
    })),
    versions: mock.pageHistory.map((v) => ({ ...v, snapshot: snap(v.snapshot) })),
    taskPageLinks: mock.taskPageLinks.map((l) => ({ ...l, createdAt: new Date().toISOString() })),
  }
}

/** The dev seed runs once a start, after the knowledge mode is known. */
let seeded = false

export function AppShell() {
  useDataChangedSync()
  useLlmDeltaSync()
  useKnowledgeProgressSync()
  useSearchModelDoneToasts()
  // FR-KN-EXT-7: until the knowledge mode is chosen (a new data folder), the window shows only the choice.
  const knowledgeMode = useSettingsQuery().data?.knowledgeMode
  const choosing = isTauri() && knowledgeMode === null
  const t = useT()
  // FR-KN-ED-10: keep unsaved editor content when the app is closed normally.
  useEffect(() => {
    installDraftCloseHook()
    installDropGuard()
    installUiErrorLog()
    void installPendingPage()
  }, [])
  // Dev only: the mock tasks and pages refer to the sample category ids, so make sure those exist.
  // `VITE_CAIRN_NO_SEED=1` skips it, to check the first-start screen (FR-DATA-8). It waits for the
  // knowledge mode (the choice refuses a folder once pages exist); a folder gets no sample pages.
  useEffect(() => {
    if (!knowledgeMode || seeded) return
    if (import.meta.env.DEV && isTauri() && !import.meta.env.VITE_CAIRN_NO_SEED) {
      seeded = true
      void seedDevCategories(mock.categories)
        .then(() => seedDevTasks(taskSeed()))
        .then(() => (knowledgeMode === 'internal' ? seedDevKnowledge(knowledgeSeed()) : undefined))
        .then(() => seedDevGlossary(glossarySeed()))
        .catch((e) => console.error('dev seed failed', e))
    }
  }, [knowledgeMode])
  if (choosing)
    return (
      <TooltipProvider delayDuration={300}>
        <ThemeSync />
        <LanguageSync />
        <ModeChooser />
        <StartupNotices />
        <Toaster position="bottom-right" />
      </TooltipProvider>
    )
  return (
    <TooltipProvider delayDuration={300}>
      <ThemeSync />
      <LanguageSync />
      <div className="min-h-screen bg-background">
        {!isTauri() && (
          <p className="bg-tint-amber px-6 py-1.5 text-center text-xs text-tint-amber-fg">
            {t.layout.shell.browserPreview}
          </p>
        )}
        <TopNav />
        <main className="mx-auto max-w-[1480px] px-6 py-6">
          <Outlet />
        </main>
      </div>
      <CommandPalette />
      <PendingPageDialog />
      <KnowledgeSwitchDialog />
      <SwitchBackupWarning />
      <StartupNotices />
      <Toaster position="bottom-right" />
    </TooltipProvider>
  )
}

/** Standard page width for feature pages that need to stay readable. */
export function PageContainer({ children }: { children: React.ReactNode }) {
  return <div className="mx-auto max-w-[1280px]">{children}</div>
}
