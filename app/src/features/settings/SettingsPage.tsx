import { Archive, Bot, Cpu, FileText, Info, Search, SlidersHorizontal, type LucideIcon } from 'lucide-react'
import { Navigate, NavLink, useParams } from 'react-router'
import { PageHeader } from '@/components/shared/PageHeader'
import { type Messages, useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { AboutTab } from './AboutTab'
import { DataTab } from './DataTab'
import { DefaultsTab } from './DefaultsTab'
import { GeneralTab } from './GeneralTab'
import { KnowledgeTab } from './KnowledgeTab'
import { ProvidersTab } from './ProvidersTab'
import { SearchTab } from './SearchTab'

/** The tabs; each one's name is `settings.tabs[slug]`. */
const TABS: { slug: keyof Messages['settings']['tabs']; icon: LucideIcon; element: React.ReactNode }[] = [
  { slug: 'general', icon: SlidersHorizontal, element: <GeneralTab /> },
  { slug: 'providers', icon: Bot, element: <ProvidersTab /> },
  { slug: 'models', icon: Cpu, element: <DefaultsTab /> },
  { slug: 'knowledge', icon: FileText, element: <KnowledgeTab /> },
  { slug: 'search', icon: Search, element: <SearchTab /> },
  { slug: 'data', icon: Archive, element: <DataTab /> },
  { slug: 'about', icon: Info, element: <AboutTab /> },
]

export default function SettingsPage() {
  const { tab } = useParams()
  const t = useT()
  if (!tab) return <Navigate to="/settings/general" replace />
  const current = TABS.find((x) => x.slug === tab)
  if (!current) return <Navigate to="/settings/general" replace />

  return (
    <div className="mx-auto max-w-[1100px]">
      <PageHeader title={t.settings.page.title} description={t.settings.page.description} icon={SlidersHorizontal} tint="slate" />
      <div className="grid gap-6 md:grid-cols-[220px_1fr]">
        <nav className="flex gap-1 self-start overflow-x-auto md:flex-col md:rounded-2xl md:border md:bg-card md:p-2">
          {TABS.map((item) => (
            <NavLink
              key={item.slug}
              to={`/settings/${item.slug}`}
              className={({ isActive }) =>
                cn(
                  'flex items-center gap-2.5 rounded-xl px-3 py-2.5 text-sm font-medium whitespace-nowrap transition-colors',
                  isActive ? 'bg-tint-blue text-tint-blue-fg' : 'text-muted-foreground hover:bg-muted hover:text-foreground',
                )
              }
            >
              <item.icon className="size-4" />
              {t.settings.tabs[item.slug]}
            </NavLink>
          ))}
        </nav>
        <div className="min-w-0">{current.element}</div>
      </div>
    </div>
  )
}
