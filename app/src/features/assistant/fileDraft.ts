// Create a page from a file inside the Assistant chat (§5.3, FR-KN-UP-1..10). The work is done by
// Rust (extraction, OCR, drafting, checks; `commands/files.rs`); this file picks the file type and
// hands the upload over.
import { getCategoriesSnapshot } from '@/api/categories'
import { type FileKind, startFileDraft, updateConversation } from '@/api/conversations'
import type { Conversation } from '@/api/conversations'
import { getT } from '@/i18n'
import { fileExt, isOcrFile } from '../creation/helpers'
import { useBusy } from './busy'

export const ATTACH_ACCEPT = '.md,.txt,.pdf,.docx,.png,.jpg,.jpeg'
export const NEW_FILE_TITLE = 'New page from a file' // i18n-ignore: stored title, compared with

export function attachmentKind(name: string): FileKind {
  const e = fileExt(name)
  if (isOcrFile(name)) return 'image'
  return e === 'pdf' ? 'pdf' : e === 'docx' ? 'docx' : 'text'
}

/** FR-KN-UP-1: null when the file is fine, else the message to show. */
export function fileTypeError(f: File): string | null {
  return ATTACH_ACCEPT.split(',').some((x) => x === '.' + fileExt(f.name))
    ? null
    : getT().assistant.file.unsupported(f.name)
}

/** The category a draft goes to: the conversation's first category, else General (changeable on the card). */
export function draftCategoryId(conv: Pick<Conversation, 'scopeCategoryIds'> | undefined): string {
  const categories = getCategoriesSnapshot()
  return conv?.scopeCategoryIds[0] ?? categories.find((p) => p.isGeneral)?.id ?? categories[0]?.id ?? ''
}

/** Sends the file and the message typed with it (the instruction, FR-KN-UP-3); Rust adds both messages and runs the draft.
 * `effort`: the level picked in the chat (D-65), for the drafting calls. */
export async function sendFile(conversationId: string, file: File, instruction: string, modelId: string, categoryId: string, effort?: string) {
  const { setUploading } = useBusy.getState()
  setUploading(conversationId, 0)
  try {
    await startFileDraft(file, { conversationId, instruction, modelId, categoryId, effort: effort ?? null }, (f) => setUploading(conversationId, f))
    await updateConversation(conversationId, { title: `Page from ${file.name}`.slice(0, 48) }) // i18n-ignore: stored title
  } finally {
    setUploading(conversationId)
  }
}
