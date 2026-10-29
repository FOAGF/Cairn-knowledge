import type { LucideIcon } from 'lucide-react'
import type { CSSProperties } from 'react'
import { priorityLabel, PRIORITY_TINT, statusLabel, STATUS_TINT, TINT_CLASS, TINT_SOFT_BG, TINT_TEXT, typeLabel, TYPE_TINT, tintForTag } from '@/lib/labels'
import type { Priority, Category, CategoryColor, TaskStatus, TaskType, TintColor } from '@/lib/types'
import { useLang, useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { Bug, Flag, Lightbulb, Minus, SquareCheck, X } from 'lucide-react'

/** Pastel rounded-square icon, as used on the dashboard cards. */
export function TintIcon({
  icon: Icon,
  tint,
  size = 'md',
  className,
}: {
  icon: LucideIcon
  tint: TintColor
  size?: 'sm' | 'md' | 'lg'
  className?: string
}) {
  const box = { sm: 'size-7 rounded-lg', md: 'size-10 rounded-xl', lg: 'size-12 rounded-2xl' }[size]
  const ico = { sm: 'size-3.5', md: 'size-5', lg: 'size-6' }[size]
  return (
    <span className={cn('inline-flex shrink-0 items-center justify-center', box, TINT_CLASS[tint], className)}>
      <Icon className={ico} strokeWidth={2} />
    </span>
  )
}

/** Small pill for a tag or label. Colour is derived from the text. */
/** The labels a short list leaves out: a round badge such as "+5", their names on hover. */
export function MoreTagsBadge({ hidden }: { hidden: string[] }) {
  const t = useT()
  if (hidden.length === 0) return null
  return (
    <span
      title={hidden.join(', ')}
      aria-label={t.shared.tint.more(hidden)}
      className="inline-flex h-5 min-w-5 shrink-0 items-center justify-center rounded-full bg-primary px-1 text-[10px] leading-none font-semibold text-primary-foreground tabular-nums"
    >
      +{hidden.length}
    </span>
  )
}

/** A tag pill; with `onRemove`, its × sits inside the pill. */
export function TagChip({ tag, tint, className, onRemove }: { tag: string; tint?: TintColor; className?: string; onRemove?: () => void }) {
  const t = useT()
  return (
    // A long tag is shortened with "…" (the whole tag on hover), never wider than its space.
    <span
      title={tag}
      className={cn(
        'inline-flex max-w-full items-center rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap',
        onRemove && 'gap-0.5 pr-1',
        TINT_CLASS[tint ?? tintForTag(tag)],
        className,
      )}
    >
      <span className="truncate">{tag}</span>
      {onRemove && (
        <button type="button" aria-label={t.shared.tint.remove(tag)} onClick={onRemove} className="rounded-full p-0.5 opacity-60 hover:bg-foreground/10 hover:opacity-100">
          <X className="size-3" />
        </button>
      )}
    </span>
  )
}

export function CategoryBadge({ category, className }: { category: Category | undefined; className?: string }) {
  if (!category) return null
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground', className)}>
      <CategoryDot color={category.color} className="size-2" />
      {category.name}
    </span>
  )
}

/** The preset category colours, in the order the colour picker shows them (same as Rust `CategoryColor::PRESETS`). */
export const PRESET_COLORS: TintColor[] = ['blue', 'green', 'purple', 'amber', 'rose', 'slate']

/** True for a preset name; false for a custom `#rrggbb` colour (D-40). */
export function isPresetColor(color: CategoryColor): color is TintColor {
  return (PRESET_COLORS as string[]).includes(color)
}

/**
 * Class and style that give a category colour the role of `TINT_CLASS` (pastel background + text),
 * or with `soft` of `TINT_SOFT_BG` (large surfaces). Custom colours use the `tint-custom*` classes
 * in index.css, which read the colour from the `--category` variable.
 */
export function categoryTint(color: CategoryColor, soft = false): { className: string; style?: CSSProperties } {
  if (isPresetColor(color)) return { className: soft ? TINT_SOFT_BG[color] : TINT_CLASS[color] }
  return { className: soft ? 'tint-custom-soft' : 'tint-custom', style: { '--category': color } as CSSProperties }
}

/** Coloured dot used inside CategoryBadge-like places. A custom colour is shown exactly as chosen. */
export function CategoryDot({ color, className }: { color: CategoryColor; className?: string }) {
  if (!isPresetColor(color)) return <span className={cn('inline-block size-2.5 shrink-0 rounded-full', className)} style={{ backgroundColor: color }} />
  return <span className={cn('inline-block size-2.5 shrink-0 rounded-full', TINT_TEXT[color], className)} style={{ backgroundColor: 'currentcolor' }} />
}

/** The marker for "Global" in glossary scopes: a hollow ring, so it never looks like a category's dot (D-40). */
export function GlobalDot({ className }: { className?: string }) {
  return <span className={cn('inline-block size-2.5 shrink-0 rounded-full border-[1.5px] border-muted-foreground', className)} />
}

/** Like `TintIcon`, in a category's colour (preset or custom): the folder icons on category cards. */
export function CategoryIcon({
  icon: Icon,
  color,
  size = 'md',
  className,
}: {
  icon: LucideIcon
  color: CategoryColor
  size?: 'sm' | 'md' | 'lg'
  className?: string
}) {
  const box = { sm: 'size-7 rounded-lg', md: 'size-10 rounded-xl', lg: 'size-12 rounded-2xl' }[size]
  const ico = { sm: 'size-3.5', md: 'size-5', lg: 'size-6' }[size]
  const tint = categoryTint(color)
  return (
    <span className={cn('inline-flex shrink-0 items-center justify-center', box, tint.className, className)} style={tint.style}>
      <Icon className={ico} strokeWidth={2} />
    </span>
  )
}

export function StatusBadge({ status, className }: { status: TaskStatus; className?: string }) {
  useLang()
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap',
        TINT_CLASS[STATUS_TINT[status]],
        status === 'wont_do' && 'line-through decoration-1',
        className,
      )}
    >
      {statusLabel(status)}
    </span>
  )
}

/** Flag for high priority, lines for medium/low (like the design reference). */
export function PriorityIcon({ priority, withLabel, className }: { priority: Priority; withLabel?: boolean; className?: string }) {
  const tint = TINT_TEXT[PRIORITY_TINT[priority]]
  const t = useT()
  return (
    <span className={cn('inline-flex items-center gap-1 text-xs', tint, className)} title={t.shared.tint.priority(priorityLabel(priority))}>
      {priority === 'high' ? (
        <Flag className="size-3.5 fill-current" />
      ) : (
        <span className="flex flex-col items-center gap-[2px]">
          <Minus className="-my-1.5 size-3.5" strokeWidth={3} />
          {priority === 'medium' && <Minus className="-my-1.5 size-3.5" strokeWidth={3} />}
        </span>
      )}
      {withLabel && <span className="font-medium">{priorityLabel(priority)}</span>}
    </span>
  )
}

const TYPE_ICON: Record<TaskType, LucideIcon> = { bug: Bug, feature: Lightbulb, general: SquareCheck }

/** Jira-style issue-type icon: bug (rose), feature (green), general (blue). */
export function TaskTypeIcon({ type, withLabel, className }: { type: TaskType; withLabel?: boolean; className?: string }) {
  const Icon = TYPE_ICON[type]
  useLang()
  return (
    <span className={cn('inline-flex items-center gap-1.5 text-xs font-medium', className)} title={typeLabel(type)}>
      <span className={cn('inline-flex size-5 shrink-0 items-center justify-center rounded-md', TINT_CLASS[TYPE_TINT[type]])}>
        <Icon className="size-3.5" strokeWidth={2.25} />
      </span>
      {withLabel && typeLabel(type)}
    </span>
  )
}
