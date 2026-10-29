import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import type { TintColor } from '@/lib/types'
import { TintIcon } from './tint'

export function PageHeader({
  title,
  description,
  icon,
  tint = 'blue',
  actions,
}: {
  title: ReactNode
  description?: ReactNode
  icon?: LucideIcon
  tint?: TintColor
  actions?: ReactNode
}) {
  return (
    <div className="mb-6 flex flex-wrap items-start justify-between gap-4">
      <div className="flex items-start gap-3">
        {icon && <TintIcon icon={icon} tint={tint} />}
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
          {description && <p className="mt-1 text-sm text-muted-foreground">{description}</p>}
        </div>
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  )
}

/** White rounded card with an optional heading row, like the dashboard sections. */
export function SectionCard({
  title,
  description,
  icon,
  tint = 'blue',
  action,
  children,
  className = '',
}: {
  title?: ReactNode
  description?: ReactNode
  icon?: LucideIcon
  tint?: TintColor
  action?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    // i18n-ignore: class list
    <section className={`rounded-2xl border bg-card p-5 shadow-[0_1px_2px_rgba(16,24,40,0.04)] ${className}`}>
      {(title || action) && (
        <div className="mb-4 flex items-start justify-between gap-3">
          <div className="flex items-start gap-3">
            {icon && <TintIcon icon={icon} tint={tint} />}
            <div>
              {title && <h2 className="text-base font-semibold">{title}</h2>}
              {description && <p className="text-xs text-muted-foreground">{description}</p>}
            </div>
          </div>
          {action}
        </div>
      )}
      {children}
    </section>
  )
}

export function EmptyState({ icon: Icon, title, children }: { icon: LucideIcon; title: string; children?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed p-10 text-center">
      <Icon className="mb-3 size-8 text-muted-foreground" />
      <p className="font-medium">{title}</p>
      {children && <div className="mt-1 text-sm text-muted-foreground">{children}</div>}
    </div>
  )
}
