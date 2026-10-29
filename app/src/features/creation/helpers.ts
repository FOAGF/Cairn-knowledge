// Pure helpers for the creation flows: Markdown sections and blocks, file names and sizes.
import type { PageOrigin } from '@/api/bindings'
import { tokenize } from '@/lib/search'
import type { PageDraft } from '@/store/creation'

/** A saved page as a draft's source. A page found in the knowledge folder (D-72) was written by hand. */
export function draftSource(origin: PageOrigin): PageDraft['source'] {
  return origin === 'folder' ? 'manual' : origin
}

// ------------------------------------------------------------------ Sections

export interface Section {
  heading: string
  /** Full Markdown of the section, including its `## heading` line. */
  text: string
}

/** Splits Markdown at `## ` headings (outside code fences). */
export function splitSections(md: string): { preamble: string; sections: Section[] } {
  const lines = md.split('\n')
  const preamble: string[] = []
  const sections: { heading: string; lines: string[] }[] = []
  let fence = false
  for (const line of lines) {
    if (/^\s*```/.test(line)) fence = !fence
    const m = !fence && /^##\s+(.+?)\s*$/.exec(line)
    if (m) sections.push({ heading: m[1], lines: [line] })
    else if (sections.length) sections[sections.length - 1].lines.push(line)
    else preamble.push(line)
  }
  return {
    preamble: preamble.join('\n').trim(),
    sections: sections.map((s) => ({ heading: s.heading, text: s.lines.join('\n').trim() })),
  }
}

export function removeSection(body: string, heading: string): string {
  const { preamble, sections } = splitSections(body)
  return [preamble, ...sections.filter((x) => x.heading !== heading).map((x) => x.text)].filter(Boolean).join('\n\n')
}

// ----------------------------------------------------------------- Files

export const fileExt = (name: string) => name.split('.').pop()?.toLowerCase() ?? ''
export const isOcrFile = (name: string) => ['png', 'jpg', 'jpeg'].includes(fileExt(name))

export function formatSize(bytes: number) {
  return bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`
}

// -------------------------------------------------------------------- Blocks

export interface Block {
  text: string
  list: boolean
  heading: boolean
}

/** Splits Markdown into blocks: headings, paragraphs and single list items (FR-KN-DU-3). */
export function splitBlocks(md: string): Block[] {
  const blocks: Block[] = []
  let para: string[] = []
  let fence: string[] | null = null
  const flush = () => {
    if (para.length) blocks.push({ text: para.join('\n'), list: false, heading: false })
    para = []
  }
  for (const line of md.split('\n')) {
    if (fence) {
      fence.push(line)
      if (/^\s*```/.test(line)) {
        blocks.push({ text: fence.join('\n'), list: false, heading: false })
        fence = null
      }
    } else if (/^\s*```/.test(line)) {
      flush()
      fence = [line]
    } else if (!line.trim()) flush()
    else if (/^#{1,6}\s/.test(line)) {
      flush()
      blocks.push({ text: line, list: false, heading: true })
    } else if (/^\s*([-*+]|\d+\.)\s/.test(line)) {
      flush()
      blocks.push({ text: line, list: true, heading: false })
    } else if (/^\s+\S/.test(line) && blocks.length && blocks[blocks.length - 1].list && !para.length) {
      blocks[blocks.length - 1].text += '\n' + line
    } else para.push(line)
  }
  flush()
  if (fence) blocks.push({ text: fence.join('\n'), list: false, heading: false })
  return blocks
}

export function joinBlocks(blocks: Block[]): string {
  return blocks.map((b, i) => (i === 0 ? '' : b.list && blocks[i - 1].list ? '\n' : '\n\n') + b.text).join('')
}

/** Share of the block's terms that already appear in `existing` (0..1). */
export function overlap(block: string, existing: string): number {
  const terms = [...new Set(tokenize(block))]
  if (!terms.length) return 0
  const have = new Set(tokenize(existing))
  return terms.filter((t) => have.has(t)).length / terms.length
}

