import { File as FileIcon, FileImage, FileText, TriangleAlert, X } from 'lucide-react'
import type { FileKind } from '@/api/conversations'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { formatSize } from '../creation/helpers'

type Kind = FileKind

const KIND_ICON = { text: FileText, pdf: FileIcon, docx: FileText, image: FileImage }
const KIND_STYLE: Record<Kind, string> = {
  text: 'bg-tint-blue text-tint-blue-fg',
  pdf: 'bg-tint-rose text-tint-rose-fg',
  docx: 'bg-tint-purple text-tint-purple-fg',
  image: 'bg-tint-green text-tint-green-fg',
}

/** A file attached to the composer or to a user message (FR-KN-UP-1..3). */
export function FileChip({
  name,
  size,
  kind,
  large,
  onRemove,
  className,
}: {
  name: string
  size: number
  kind: Kind
  large?: boolean
  onRemove?: () => void
  className?: string
}) {
  const Icon = KIND_ICON[kind]
  const t = useT()
  return (
    <div className={cn('flex max-w-full flex-col gap-1', className)}>
      <div className="inline-flex max-w-full items-center gap-2.5 rounded-xl border bg-card py-1.5 pr-2 pl-1.5 text-foreground">
        <span className={cn('flex size-8 shrink-0 items-center justify-center rounded-lg', KIND_STYLE[kind])}>
          <Icon className="size-4" />
        </span>
        <span className="min-w-0 text-left">
          <span className="block max-w-56 truncate text-sm font-medium">{name}</span>
          <span className="block text-xs text-muted-foreground">{formatSize(size)}</span>
        </span>
        {onRemove && (
          <button type="button" onClick={onRemove} aria-label={t.assistant.file.remove} className="rounded-full p-1 text-muted-foreground hover:bg-muted hover:text-foreground">
            <X className="size-3.5" />
          </button>
        )}
      </div>
      {large && (
        <p className="flex items-center gap-1.5 text-xs text-tint-amber-fg">
          <TriangleAlert className="size-3.5 shrink-0" /> {t.assistant.file.large}
        </p>
      )}
    </div>
  )
}
