import { FileText } from 'lucide-react'
import { Children, type ReactNode, useEffect, useMemo, useState } from 'react'
import ReactMarkdown, { type Options } from 'react-markdown'
import { Link } from 'react-router'
import remarkGfm from 'remark-gfm'
import { toast } from 'sonner'
import { openUrl } from '@/api/assistant'
import { errorMessage, isTauri } from '@/api/client'
import { attachmentUrl, usePagesByIds } from '@/api/knowledge'
import { useSettings } from '@/api/settings'
import { folderFileUrl, folderPath } from '@/api/storage'
import { useTasksByKeys } from '@/api/tasks'
import { STATUS_TINT, TINT_TEXT } from '@/lib/labels'
import { mayHaveMath, prepareMath } from '@/lib/math'
import { mentionsToLinks } from '@/lib/mentions'
import { useGlossary } from '@/api/glossary'
import { firstPerEntry, type TermMatcher } from '@/lib/glossary'
import { useT } from '@/i18n'
import { GlossaryTerm, useGlossaryMatcher } from './GlossaryTerm'
import { cn } from '@/lib/utils'

export function TaskMention({ taskKey }: { taskKey: string }) {
  // The key may be an old one (alias); Rust returns the task under its current key.
  const keys = useMemo(() => [taskKey], [taskKey])
  const { data, isPending } = useTasksByKeys(keys)
  const task = data?.[0]
  const t = useT()
  if (!task && isPending) return <span className="rounded border border-dashed px-1 text-muted-foreground">#{taskKey}</span>
  if (!task)
    return <span className="rounded border border-dashed px-1 text-muted-foreground" title={t.shared.markdown.taskNotFound}>#{taskKey}</span>
  return (
    <Link
      to={`/tasks/${task.key}`}
      className="inline-flex items-baseline gap-1 rounded-md bg-accent px-1.5 py-px text-[0.9em] font-medium text-accent-foreground no-underline hover:underline"
      title={task.title}
    >
      <span className={cn('relative top-[-1px] inline-block size-1.5 self-center rounded-full', TINT_TEXT[STATUS_TINT[task.status]])} style={{ backgroundColor: 'currentcolor' }} />
      <span className={task.status === 'done' || task.status === 'wont_do' ? 'line-through' : ''}>{task.key}</span>
      <span className="max-w-[16rem] truncate font-normal opacity-80">{task.title}</span>
    </Link>
  )
}

/** `[Title](page:<id>)`: shows the page's current title; the stored text is the fallback. */
export function PageMention({ id, fallback }: { id: string; fallback: string }) {
  const ids = useMemo(() => [id], [id])
  const { data, isPending } = usePagesByIds(ids)
  const page = data?.[0]
  const t = useT()
  if (!page)
    return (
      <span className="rounded border border-dashed px-1 text-muted-foreground" title={isPending ? undefined : t.shared.markdown.pageNotFound}>
        {fallback || t.shared.markdown.page}
      </span>
    )
  return (
    <Link
      to={`/knowledge/${page.id}`}
      className="inline-flex items-baseline gap-1 rounded-md bg-tint-purple px-1.5 py-px text-[0.9em] font-medium text-tint-purple-fg no-underline hover:underline"
    >
      <FileText className="relative top-[2px] size-3.5 self-start" />
      {page.title}
    </Link>
  )
}

/** `![alt](attachment:<name>)`: the asset URL is resolved asynchronously. */
function AttachmentImage({ name, alt }: { name: string; alt: string }) {
  const [src, setSrc] = useState<string | null>(null)
  const t = useT()
  useEffect(() => {
    let live = true
    attachmentUrl(name).then(
      (u) => live && setSrc(u),
      () => live && setSrc(''),
    )
    return () => {
      live = false
    }
  }, [name])
  if (src === null) return <span className="inline-block h-16 w-24 animate-pulse rounded-lg bg-muted align-middle" aria-label={t.shared.markdown.loadingImage} />
  if (!src) return <span className="text-muted-foreground">{t.shared.markdown.image(alt || name)}</span>
  return <img src={src} alt={alt} className="max-w-full rounded-lg" />
}

/** `![alt](folder:<path>)` (external mode, D-72): a file in the knowledge folder, by its path from the root. */
function FolderImage({ target, alt }: { target: string; alt: string }) {
  const root = useSettings().externalRoot
  const t = useT()
  if (!root || !isTauri()) return <span className="text-muted-foreground">{t.shared.markdown.image(alt || folderPath(target))}</span>
  return <img src={folderFileUrl(root, target)} alt={alt} className="max-w-full rounded-lg" />
}

/**
 * A link out of the app: the system browser opens it. The window itself never navigates away from
 * the app or opens another (D-43), so a plain link would do nothing. Only http and https addresses
 * open; anything else shows why.
 */
function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault()
        void openUrl(href).catch((err) => toast.error(errorMessage(err)))
      }}
    >
      {children}
    </a>
  )
}

function nodeText(children: ReactNode): string {
  return Children.toArray(children)
    .map((c) => (typeof c === 'string' || typeof c === 'number' ? String(c) : ''))
    .join('')
}

// FR-KN-GL-6, D-38: marks the first occurrence of each glossary term on the page. Code, inline code, links and mentions are left alone.
type MdNode = { type: string; value?: string; children?: MdNode[]; data?: Record<string, unknown> }
const SKIP_TYPES = new Set(['code', 'inlineCode', 'link', 'linkReference', 'image', 'html', 'math', 'inlineMath'])
function remarkGlossary(matcher: TermMatcher) {
  // Shared by the whole page, so a term is marked only where it first appears.
  let seen = new Set<string>()
  const walk = (node: MdNode) => {
    if (!node.children || SKIP_TYPES.has(node.type)) return
    node.children = node.children.flatMap((child): MdNode[] => {
      if (child.type !== 'text' || !child.value) {
        walk(child)
        return [child]
      }
      const hits = firstPerEntry(matcher.find(child.value), seen)
      if (!hits.length) return [child]
      const out: MdNode[] = []
      let at = 0
      for (const h of hits) {
        if (h.start > at) out.push({ type: 'text', value: child.value.slice(at, h.start) })
        // Becomes <span data-glossary="<entry id>">term</span>; see the `span` component below.
        out.push({
          type: 'emphasis',
          data: { hName: 'span', hProperties: { 'data-glossary': h.entry.id } },
          children: [{ type: 'text', value: child.value.slice(h.start, h.end) }],
        })
        at = h.end
      }
      if (at < child.value.length) out.push({ type: 'text', value: child.value.slice(at) })
      return out
    })
  }
  return (tree: MdNode) => {
    seen = new Set()
    walk(tree)
  }
}

// D-63: LaTeX formulas (lib/math.ts) render with KaTeX. It is loaded the first time a text may hold a
// formula, so start-up and memory don't pay for it; until then the text shows as written.
type MathPlugins = { remark: NonNullable<Options['remarkPlugins']>[number]; rehype: NonNullable<Options['rehypePlugins']>[number] }
let mathPlugins: MathPlugins | null = null
let mathLoading: Promise<MathPlugins> | null = null
function loadMath(): Promise<MathPlugins> {
  mathLoading ??= Promise.all([import('remark-math'), import('rehype-katex'), import('katex/dist/katex.min.css')]).then(([rm, rk]) => {
    // A formula with a mistake shows in red with KaTeX's message, instead of failing the page.
    mathPlugins = { remark: rm.default, rehype: [rk.default, { throwOnError: false, strict: 'ignore' }] }
    return mathPlugins
  })
  return mathLoading
}
function useMath(needed: boolean): MathPlugins | null {
  const [plugins, setPlugins] = useState(mathPlugins)
  useEffect(() => {
    if (!needed || plugins) return
    let live = true
    loadMath().then(
      (p) => live && setPlugins(p),
      () => undefined,
    )
    return () => {
      live = false
    }
  }, [needed, plugins])
  return needed ? plugins : null
}

/**
 * Renders Markdown (GFM) with task/page mentions shown as clickable chips, and LaTeX formulas (D-63).
 * With `glossaryCategoryId` (a category id, or null for global only), glossary terms get a dotted underline and a hover card.
 */
export function MarkdownView({
  text,
  className,
  glossaryCategoryId,
  renderLink,
}: {
  text: string
  className?: string
  glossaryCategoryId?: string | null
  /** Draws the links it returns something for (the Assistant's `[n]` citation markers); the others render as usual. */
  renderLink?: (href: string, children: ReactNode) => ReactNode
}) {
  const matcher = useGlossaryMatcher(glossaryCategoryId ?? null)
  const t = useT()
  const entries = useGlossary().data
  const math = useMath(mayHaveMath(text))
  const remarkPlugins = useMemo(
    () => [remarkGfm, ...(math ? [math.remark] : []), ...(glossaryCategoryId === undefined ? [] : [() => remarkGlossary(matcher)])],
    [glossaryCategoryId, matcher, math],
  )
  const rehypePlugins = useMemo(() => (math ? [math.rehype] : []), [math])
  return (
    <div className={cn('markdown-body', className)}>
      <ReactMarkdown
        remarkPlugins={remarkPlugins}
        rehypePlugins={rehypePlugins}
        urlTransform={(url) => url}
        components={{
          span: ({ node: _n, children, ...rest }) => {
            const id = (rest as { 'data-glossary'?: string })['data-glossary']
            const entry = id ? entries?.find((e) => e.id === id) : undefined
            return entry ? <GlossaryTerm entry={entry}>{children}</GlossaryTerm> : <span {...rest}>{children}</span>
          },
          img: ({ src = '', alt = '' }) =>
            typeof src === 'string' && src.startsWith('attachment:') ? (
              <AttachmentImage name={src.slice('attachment:'.length)} alt={alt} />
            ) : typeof src === 'string' && src.startsWith('folder:') ? (
              <FolderImage target={src} alt={alt} />
            ) : (
              <img src={typeof src === 'string' ? src : ''} alt={alt} className="max-w-full rounded-lg" />
            ),
          a: ({ href = '', children }) => {
            const custom = renderLink?.(href, children)
            if (custom != null) return custom
            if (href.startsWith('cairn-task:')) return <TaskMention taskKey={href.slice('cairn-task:'.length)} />
            if (href.startsWith('page:')) return <PageMention id={href.slice('page:'.length)} fallback={nodeText(children)} />
            // Another file in the knowledge folder (a PDF, …): named, not opened, as the window never leaves the app.
            if (href.startsWith('folder:'))
              return (
                <span className="underline decoration-dotted underline-offset-2" title={t.shared.markdown.inFolder(folderPath(href))}>
                  {children}
                </span>
              )
            return <ExternalLink href={href}>{children}</ExternalLink>
          },
        }}
      >
        {mentionsToLinks(math ? prepareMath(text) : text)}
      </ReactMarkdown>
    </div>
  )
}
