// Edit | Preview for Markdown (FR-KN-ED-2, D-62). Edit is the styled source (MarkdownEditor); Preview
// renders the text with MarkdownView, the same renderer as reading, so it shows exactly what will be
// saved. The editor stays mounted while previewing, so its undo history and cursor survive.
// The hints list the Markdown only Cairn understands.
import { CircleHelp } from 'lucide-react'
import { useState } from 'react'
import { useIsExternal } from '@/api/storage'
import { MarkdownView } from '@/components/shared/MarkdownView'
import { Button } from '@/components/ui/button'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { type Messages, useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { MarkdownEditor, type MarkdownEditorProps } from './MarkdownEditor'

export type EditorMode = 'edit' | 'preview'

export function EditPreviewToggle({ mode, onChange, small }: { mode: EditorMode; onChange: (m: EditorMode) => void; small?: boolean }) {
  const t = useT()
  const label: Record<EditorMode, string> = { edit: t.common.edit, preview: t.editor.mode.preview }
  return (
    <div className="inline-flex rounded-lg bg-muted p-0.5" role="group" aria-label={t.editor.mode.label}>
      {(['edit', 'preview'] as const).map((m) => (
        <button
          key={m}
          type="button"
          aria-pressed={mode === m}
          onClick={() => onChange(m)}
          className={cn(
            'rounded-md font-medium capitalize text-muted-foreground',
            small ? 'px-2 py-0.5 text-xs' : 'px-3 py-1 text-sm',
            mode === m && 'bg-card text-foreground shadow-sm',
          )}
        >
          {label[m]}
        </button>
      ))}
    </div>
  )
}

/** `code` is something to type; otherwise `label` names what to do. */
function hints(h: Messages['editor']['hints']): { code?: string; label?: string; text: string }[] {
  return [
    { code: '[[', text: h.pageLink },
    { code: '#GEN-12', text: h.taskMention },
    { label: h.images, text: h.imagesText },
    { label: h.glossaryTerms, text: h.glossaryText },
    { code: '$x^2$', text: h.latex },
  ]
}

/** The Markdown only Cairn understands, in a small popover. `pageImages`: the editor holds a page. */
export function MarkdownHints({ small, pageImages }: { small?: boolean; pageImages?: boolean }) {
  const folderImages = useIsExternal() && pageImages
  const h = useT().editor.hints
  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button type="button" variant="ghost" size={small ? 'xs' : 'sm'} className="text-muted-foreground">
          <CircleHelp /> {h.button}
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-96 space-y-3 text-sm">
        <p className="font-medium">{h.title}</p>
        <dl className="space-y-2.5">
          {hints(h).map((hint) => (
            <div key={hint.code ?? hint.label}>
              <dt>{hint.code ? <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">{hint.code}</code> : <span className="font-medium">{hint.label}</span>}</dt>
              <dd className="mt-1 text-muted-foreground">{folderImages && hint.label === h.images ? h.folderImagesText : hint.text}</dd>
            </div>
          ))}
        </dl>
        <p className="border-t pt-2.5 text-xs text-muted-foreground">{h.rest}</p>
      </PopoverContent>
    </Popover>
  )
}

/** A Markdown editor with Edit | Preview and the hints, for task descriptions, comments and drafts.
 *  `small`: editor and preview at the size the text is read at (`text-sm`). */
export function MarkdownField(props: MarkdownEditorProps) {
  const [mode, setMode] = useState<EditorMode>('edit')
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <EditPreviewToggle mode={mode} onChange={setMode} small />
        <MarkdownHints small pageImages={!!props.imageCategoryId} />
      </div>
      <div className={mode === 'preview' ? 'hidden' : undefined}>
        <MarkdownEditor {...props} />
      </div>
      {mode === 'preview' && <MarkdownPreview text={props.value} minHeight={props.minHeight} glossaryCategoryId={props.glossaryCategoryId} className={props.small ? 'text-sm' : undefined} />}
    </div>
  )
}

/** The Preview: the reading renderer, at least as tall as the editor so the layout doesn't jump. */
export function MarkdownPreview({ text, minHeight, glossaryCategoryId, className }: { text: string; minHeight?: string; glossaryCategoryId?: string | null; className?: string }) {
  const t = useT()
  return (
    <div style={{ minHeight }}>
      {text.trim() ? (
        <MarkdownView text={text} glossaryCategoryId={glossaryCategoryId} className={className} />
      ) : (
        <p className="text-sm text-muted-foreground italic">{t.editor.previewEmpty}</p>
      )}
    </div>
  )
}
