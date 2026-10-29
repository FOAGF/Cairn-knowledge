import { useSettings, useUpdateSettings } from '@/api/settings'
import { BookA, BookOpen, CircleCheckBig, FolderKanban, House, type LucideIcon, MessageSquareText, Moon, Search, Settings, Sun } from 'lucide-react'
import { NavLink, useNavigate } from 'react-router'
import { CairnLogo } from '@/components/shared/CairnLogo'
import { SearchModelTaskLine } from '@/components/shared/SearchModel'
import { Avatar, AvatarFallback } from '@/components/ui/avatar'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { PendingPageTab } from '@/features/creation/PendingPage'
import { useOnPendingPage } from '@/features/creation/pending'
import { type Messages, useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { useAppStore } from '@/store'
import { useIsDark } from './ThemeSync'

/** `name`: its key in `common.nav`. */
const NAV: { to: string; name: keyof Messages['common']['nav']; icon: LucideIcon; end?: boolean }[] = [
  { to: '/', name: 'home', icon: House, end: true },
  { to: '/assistant', name: 'assistant', icon: MessageSquareText },
  { to: '/knowledge', name: 'knowledge', icon: BookOpen },
  { to: '/tasks', name: 'tasks', icon: CircleCheckBig },
  { to: '/categories', name: 'categories', icon: FolderKanban },
  // UI-KN-8: the glossary's entry point (D-39).
  { to: '/glossary', name: 'glossary', icon: BookA },
]

export function TopNav() {
  const navigate = useNavigate()
  const userName = useSettings().userName
  const updateSettings = useUpdateSettings()
  const setCommandOpen = useAppStore((s) => s.setCommandOpen)
  const dark = useIsDark()
  // On the pending page only its own tab is highlighted, not Knowledge as well.
  const onPending = useOnPendingPage()
  const t = useT()

  return (
    <header className="sticky top-0 z-30 border-b bg-surface/90 backdrop-blur">
      <div className="mx-auto flex h-16 max-w-[1480px] items-center gap-6 px-6">
        <NavLink to="/" className="flex items-center gap-2">
          <CairnLogo className="size-8" />
          <span className="text-xl font-bold tracking-tight">Cairn</span>
        </NavLink>

        <nav className="flex min-w-0 items-center gap-1">
          {NAV.map(({ to, name, icon: Icon, end }) => (
            <NavLink
              key={to}
              to={to}
              end={end}
              className={({ isActive }) =>
                cn(
                  'relative flex h-10 items-center gap-2 rounded-xl px-3.5 text-sm font-medium transition-colors',
                  isActive && !onPending
                    ? 'bg-accent text-accent-foreground after:absolute after:inset-x-3 after:-bottom-[13px] after:h-0.5 after:rounded-full after:bg-primary'
                    : 'text-foreground/80 hover:bg-muted hover:text-foreground',
                )
              }
            >
              <Icon className="size-[18px]" />
              {t.common.nav[name]}
            </NavLink>
          ))}
          <PendingPageTab />
        </nav>

        <div className="ml-auto flex items-center gap-1">
          {/* FR-RAG-EM-3, D-82: the search model being added, or a failure; links to Settings › Search */}
          <SearchModelTaskLine compact className="mr-3 min-w-36 max-w-56" />
          <Button
            variant="outline"
            className="h-9 w-56 justify-start gap-2 rounded-xl text-muted-foreground"
            onClick={() => setCommandOpen(true)}
          >
            <Search className="size-4" />
            <span className="text-sm">{t.layout.topNav.search}</span>
            {/* i18n-ignore: key names */}
            <kbd className="ml-auto rounded border bg-muted px-1.5 font-mono text-[10px]">Ctrl K</kbd>
          </Button>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="rounded-xl"
                onClick={() => updateSettings({ theme: dark ? 'light' : 'dark' })}
              >
                {dark ? <Sun className="size-5" /> : <Moon className="size-5" />}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{dark ? t.layout.topNav.lightTheme : t.layout.topNav.darkTheme}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="rounded-xl" onClick={() => navigate('/settings')}>
                <Settings className="size-5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t.common.nav.settings}</TooltipContent>
          </Tooltip>
          <Avatar className="ml-2 size-9">
            <AvatarFallback className="bg-tint-purple font-semibold text-tint-purple-fg">{userName.slice(0, 2) || t.layout.topNav.me}</AvatarFallback>
          </Avatar>
        </div>
      </div>
    </header>
  )
}
