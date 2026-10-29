// Shared Markdown editor (CodeMirror 6): styled Markdown source (FR-KN-ED-2, D-62; sourceStyle.ts).
// Tasks, comments and the creation flow use it too, through MarkdownField (Edit | Preview).
import { autocompletion } from '@codemirror/autocomplete'
import { defaultKeymap, history, historyKeymap, isolateHistory } from '@codemirror/commands'
import { markdown } from '@codemirror/lang-markdown'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { EditorState, Transaction } from '@codemirror/state'
import { drawSelection, EditorView, keymap, placeholder as cmPlaceholder } from '@codemirror/view'
import { tags } from '@lezer/highlight'
import { useEffect, useLayoutEffect, useRef } from 'react'
import { toast } from 'sonner'
import { commands } from '@/api/bindings'
import { errorMessage } from '@/api/client'
import { saveImage } from '@/api/knowledge'
import { useCategoryList } from '@/api/categories'
import { useSettings, useUpdateSettings } from '@/api/settings'
import { useGlossaryMatcher } from '@/components/shared/GlossaryTerm'
import { getT } from '@/i18n'
import { droppedLink, fileFromLink, isExternalFileDrag } from '@/lib/dropGuard'
import { cn } from '@/lib/utils'
import { glossaryMarks } from './glossaryMarks'
import { harperLint, relint } from './harperLint'
import { mentionSources } from './mentionComplete'
import { sourceStyle } from './sourceStyle'

export interface MarkdownEditorProps {
  value: string
  onChange: (value: string) => void
  placeholder?: string
  /** CSS length, e.g. '8rem'. */
  minHeight?: string
  autoFocus?: boolean
  /** Harper spelling check (FR-KN-ED-6). Default: the Settings value. */
  spellcheck?: boolean
  /** Harper grammar check. Default: the Settings value. */
  grammar?: boolean
  /** FR-KN-GL-6. Category whose glossary marks terms (null = global only). Omit to switch it off. */
  glossaryCategoryId?: string | null
  /** Text at the size of `text-sm`, for fields read at that size (task descriptions, comments). */
  small?: boolean
  /**
   * The editor holds a page of this category: pasted images are stored for it (in external mode in
   * the category folder's `attachments`, FR-KN-EXT-3). Leave it out for tasks and comments.
   */
  imageCategoryId?: string
  className?: string
}

let imageSeq = 0

/** Saves a pasted or dropped image and puts its Markdown at `pos` (a placeholder shows meanwhile). */
function insertImage(view: EditorView, file: File, pos: number, pageCategoryId: string | undefined) {
  const token = getT().editor.image.saving(++imageSeq)
  // The placeholder never enters the undo history; the image's link is one step of its own, so undo
  // takes out exactly the image and never brings the placeholder back.
  const outOfHistory = Transaction.addToHistory.of(false)
  view.dispatch({ changes: { from: pos, insert: token }, selection: { anchor: pos + token.length }, annotations: outOfHistory })
  const swap = (insert: string) => {
    const at = view.state.doc.toString().indexOf(token)
    if (at < 0) return
    view.dispatch({ changes: { from: at, to: at + token.length }, annotations: outOfHistory })
    if (insert) view.dispatch({ changes: { from: at, insert }, annotations: isolateHistory.of('full') })
  }
  saveImage(file, '', pageCategoryId).then(
    (markdown) => swap(markdown + '\n'),
    (e) => {
      swap('')
      toast.error(getT().editor.image.saveFailed(e instanceof Error ? e.message : String(e)))
    },
  )
}

/** An image file in a paste or drop: in `files`, or only in `items` (as some webviews offer it). */
function imageFile(data: DataTransfer | null): File | null {
  const file = [...(data?.files ?? [])].find((f) => f.type.startsWith('image/'))
  if (file) return file
  const item = [...(data?.items ?? [])].find((i) => i.kind === 'file' && i.type.startsWith('image/'))
  return item?.getAsFile() ?? null
}

/** A `file://` link to a PNG or JPEG (a file manager's drag or *Copy*, lib/dropGuard.ts), or null. */
function imageLink(data: DataTransfer | null): string | null {
  const link = droppedLink(data)
  return link && /\.(png|jpe?g)$/i.test(link) ? link : null
}

/**
 * Paste of images; `pageCategory` is read at that moment (`imageCategoryId`): an image file, a
 * `file://` link, ordinary text as usual; and when the paste holds neither text nor a file
 * (WebKitGTK may offer a pasted screenshot or a copied file that way), Rust reads the system
 * clipboard (`read_clipboard_image`).
 */
const pasteHandler = (pageCategory: () => string | undefined) =>
  EditorView.domEventHandlers({
    paste(e, view) {
      const pos = view.state.selection.main.head
      const file = imageFile(e.clipboardData)
      if (file) {
        e.preventDefault()
        insertImage(view, file, pos, pageCategory())
        return true
      }
      const link = imageLink(e.clipboardData)
      if (link) {
        e.preventDefault()
        void fileFromLink(link).then((f) => insertImage(view, f, pos, pageCategory()), (err) => toast.error(errorMessage(err)))
        return true
      }
      if (e.clipboardData?.getData('text/plain')) return false
      e.preventDefault()
      void commands.readClipboardImage().then(
        (image) => {
          if (!image) return
          const bytes = Uint8Array.from(atob(image.dataBase64), (c) => c.charCodeAt(0))
          insertImage(view, new File([bytes], 'pasted', { type: image.mime }), pos, pageCategory())
        },
        (err) => toast.error(errorMessage(err)),
      )
      return true
    },
  })

/**
 * Drops of files anywhere on the editor box, caught before CodeMirror sees them: its text element
 * is only as tall as the text, and the padding and the space below the last line belong to its
 * scroller, where CodeMirror's own handlers never run (and the app's drop guard would refuse the
 * drag). An image file, or a file manager's `file://` link, is saved and inserted at the drop point
 * (the end, below the text); any other file inserts nothing.
 */
function fileDropListeners(view: EditorView, pageCategory: () => string | undefined) {
  const claim = (e: DragEvent) => {
    if (!isExternalFileDrag(e)) return
    e.preventDefault()
    e.stopPropagation()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy'
  }
  const drop = (e: DragEvent) => {
    if (!isExternalFileDrag(e)) return
    e.preventDefault()
    e.stopPropagation()
    const pos = view.posAtCoords({ x: e.clientX, y: e.clientY }) ?? view.state.doc.length
    const file = imageFile(e.dataTransfer)
    if (file) return insertImage(view, file, pos, pageCategory())
    const link = imageLink(e.dataTransfer)
    if (link) return void fileFromLink(link).then((f) => insertImage(view, f, pos, pageCategory()), (err) => toast.error(errorMessage(err)))
    toast.info(getT().editor.image.dropOnlyImages)
  }
  return { dragenter: claim, dragover: claim, drop } as const
}

/**
 * Keeps a key press inside the editor's box. On Linux, Cairn turns WebKitGTK's DMA-BUF renderer off
 * (src-tauri/src/main.rs), and that renderer repaints the whole window whenever a change reaches the
 * page's layout: about 25 ms on the owner's machine, so a held Backspace (a repeat every 30 ms) fell
 * behind and went on deleting after the key was released. The box is `contain: size layout`, a
 * boundary the page's layout stops at, with a height set from the editor's own height whenever that
 * changes (a line added or removed). Unlike `contain: paint`, it doesn't clip the editor's popups.
 * Returns the cleanup.
 */
function followHeight(box: HTMLElement, view: EditorView): () => void {
  const sync = () => {
    box.style.height = `${view.dom.getBoundingClientRect().height}px`
  }
  sync()
  box.style.contain = 'size layout style'
  // Runs after layout and before paint, so the box never shows at an old height.
  const observer = new ResizeObserver(sync)
  observer.observe(view.dom)
  return () => observer.disconnect()
}

const highlight = HighlightStyle.define([
  { tag: tags.heading, fontWeight: '650' },
  { tag: tags.strong, fontWeight: '700' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.link, color: 'var(--primary)' },
  { tag: tags.url, color: 'var(--muted-foreground)' },
  { tag: tags.monospace, fontFamily: 'ui-monospace, monospace', color: 'var(--tint-purple-fg)' }, // i18n-ignore: CSS
  { tag: tags.quote, color: 'var(--muted-foreground)', fontStyle: 'italic' },
  { tag: tags.processingInstruction, color: 'var(--muted-foreground)' },
])

const baseTheme = EditorView.theme({
  '&': { backgroundColor: 'transparent', color: 'var(--foreground)', fontSize: '0.95rem' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--app-font)', lineHeight: '1.7' },
  '.cm-content': { padding: '0.25rem 0' },
  // The cursor is drawn by CodeMirror (drawSelection): WebKit's own caret took the height of the
  // placeholder, up to several lines when it wrapped. No negative margin: the text starts at the
  // scroller's edge, so a cursor at the start of a line would be clipped.
  '.cm-cursor, .cm-dropCursor': { borderLeft: '1.5px solid var(--primary)', marginLeft: '0' }, // i18n-ignore: CSS
  '.cm-line': { padding: '0' },
  '.cm-placeholder': { color: 'var(--muted-foreground)' },
  '.cm-tooltip': {
    border: '1px solid var(--border)', // i18n-ignore: CSS
    borderRadius: '0.75rem',
    backgroundColor: 'var(--popover)',
    color: 'var(--popover-foreground)',
    boxShadow: '0 8px 24px rgba(16,24,40,0.12)', // i18n-ignore: CSS
    overflow: 'hidden',
    fontFamily: 'var(--app-font)',
  },
  '.cm-tooltip.cm-tooltip-autocomplete > ul': { fontFamily: 'var(--app-font)', maxHeight: '16rem', padding: '0.25rem' },
  '.cm-tooltip.cm-tooltip-autocomplete > ul > li': { padding: '0.3rem 0.6rem', borderRadius: '0.5rem', display: 'flex', gap: '0.25rem' },
  '.cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]': { backgroundColor: 'var(--accent)', color: 'var(--accent-foreground)' },
  '.cm-completionDetail': { color: 'var(--muted-foreground)', fontStyle: 'normal', marginLeft: '0.25rem' },
  '.cm-lintRange-warning': { backgroundImage: 'none', textDecoration: 'underline wavy #d99a00', textUnderlineOffset: '3px' }, // i18n-ignore: CSS
  '.cm-lintRange-error': { backgroundImage: 'none', textDecoration: 'underline wavy #d33', textUnderlineOffset: '3px' }, // i18n-ignore: CSS
  '.cm-tooltip-lint': { padding: '0.25rem' },
  '.cm-diagnostic': { padding: '0.25rem 0.5rem', borderLeftWidth: '3px', fontSize: '0.85rem' },
  '.cm-diagnosticAction': { backgroundColor: 'var(--accent)', color: 'var(--accent-foreground)', borderRadius: '0.375rem', padding: '0 0.5rem', margin: '0.25rem 0.25rem 0 0' },
  '.cm-diagnosticSource': { display: 'none' },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: 'var(--accent) !important' },
})

export function MarkdownEditor({
  value,
  onChange,
  placeholder,
  minHeight = '6rem',
  autoFocus,
  spellcheck,
  grammar,
  glossaryCategoryId,
  small,
  imageCategoryId,
  className,
}: MarkdownEditorProps) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange
  // The last texts this editor reported: `value` coming back as one of them is the parent catching
  // up, never a new text to load (an image swapped in meanwhile would otherwise be undone).
  const reported = useRef<string[]>([])
  const settings = useSettings()
  const updateSettings = useUpdateSettings()
  // Read by the lint source on every run, so toggles and dictionary changes need no rebuild.
  const lintState = useRef({ spelling: false, grammar: false, dictionary: [] as string[] })
  lintState.current.spelling = spellcheck ?? settings.spellcheck
  lintState.current.grammar = grammar ?? settings.grammar
  lintState.current.dictionary = settings.personalDictionary
  // Read by the glossary marks on every update.
  const categories = useCategoryList()
  const glossary = useRef({ matcher: null as ReturnType<typeof useGlossaryMatcher> | null, categoryName: (_id: string): string => '' })
  const matcher = useGlossaryMatcher(glossaryCategoryId ?? null)
  glossary.current.matcher = glossaryCategoryId === undefined ? null : matcher
  glossary.current.categoryName = (id) => categories.find((p) => p.id === id)?.name ?? getT().common.category
  const updateSettingsRef = useRef(updateSettings)
  updateSettingsRef.current = updateSettings
  const imageCategory = useRef(imageCategoryId)
  imageCategory.current = imageCategoryId

  useEffect(() => {
    const v = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          history(),
          drawSelection(),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          markdown(),
          syntaxHighlighting(highlight),
          EditorView.lineWrapping,
          baseTheme,
          EditorView.theme({ '.cm-content': { minHeight }, ...(small && { '&.cm-editor': { fontSize: '0.875rem' } }) }),
          cmPlaceholder(placeholder ?? ''),
          EditorView.contentAttributes.of({ spellcheck: 'false' }),
          harperLint({
            options: () => lintState.current,
            addWord: (word, done) => {
              const dict = lintState.current.dictionary
              if (dict.some((w) => w.toLowerCase() === word.toLowerCase())) return done()
              updateSettingsRef.current({ personalDictionary: [...dict, word] }, { onSuccess: done })
            },
          }),
          glossaryMarks({
            matcher: () => glossary.current.matcher,
            scopeName: (e) => (e.categoryId ? glossary.current.categoryName(e.categoryId) : getT().editor.glossary.global),
          }),
          sourceStyle,
          autocompletion({ override: mentionSources, icons: false }),
          pasteHandler(() => imageCategory.current),
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return
            const text = u.state.doc.toString()
            reported.current = [...reported.current.slice(-19), text]
            onChangeRef.current(text)
          }),
        ],
      }),
    })
    view.current = v
    if (autoFocus) v.focus()
    // Capture phase on the whole box (see `fileDropListeners`).
    const box = host.current!
    const drops = Object.entries(fileDropListeners(v, () => imageCategory.current))
    for (const [type, listener] of drops) box.addEventListener(type, listener as EventListener, true)
    const stopFollowing = followHeight(box, v)
    return () => {
      stopFollowing()
      for (const [type, listener] of drops) box.removeEventListener(type, listener as EventListener, true)
      v.destroy()
    }
    // The editor is created once; value changes are synced below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // A toggle changed: check again now.
  const wantSpelling = spellcheck ?? settings.spellcheck
  const wantGrammar = grammar ?? settings.grammar
  useEffect(() => {
    if (view.current) relint(view.current)
  }, [wantSpelling, wantGrammar])

  // Sync external value changes (e.g. switching pages) into the editor. A layout effect, run as the
  // render commits: a plain effect runs later, and a change made in between (a pasted image swapped
  // in) would be overwritten by this older value.
  useLayoutEffect(() => {
    const v = view.current
    if (v && value !== v.state.doc.toString() && !reported.current.includes(value)) {
      reported.current = []
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } })
    }
  }, [value])

  return <div ref={host} className={cn('w-full', className)} />
}
