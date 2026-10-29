// Saved conversations, their messages and files (FR-LLM-AS-5, FR-KN-UP-1..10). Rust emits
// DataChanged { conversations } after every change, which refetches everything under
// ['conversations'] (the list and every conversation's messages), so the mutations here do not
// invalidate anything themselves.
import { useMutation, useQuery } from '@tanstack/react-query'
import { getT } from '@/i18n'
import {
  type ChatMessage,
  type Conversation,
  type ConversationPatch,
  commands,
  type FileDraftInput,
  type MessagePatch,
  type NewConversation,
  type NewMessage,
  type ReviseInput,
} from './bindings'

export type {
  ChatAttachment,
  ChatMessage,
  ChatStep,
  Conversation,
  DraftCard,
  FileKind,
  MessageError,
  MessagePatch,
  NewMessage,
  StepState,
} from './bindings'

const NONE_C: Conversation[] = []
const NONE_M: ChatMessage[] = []

/** Newest activity first. */
export function useConversations() {
  return useQuery({ queryKey: ['conversations'], queryFn: () => commands.listConversations() })
}

/** The list for screens that only read it: empty until loaded, never a new array. */
export function useConversationList(): Conversation[] {
  return useConversations().data ?? NONE_C
}

/** The messages of one conversation, oldest first. */
export function useMessages(conversationId: string | undefined) {
  return useQuery({
    queryKey: ['conversations', conversationId, 'messages'],
    queryFn: () => commands.listMessages(conversationId ?? ''),
    enabled: !!conversationId,
  })
}

export function useMessageList(conversationId: string | undefined): ChatMessage[] {
  return useMessages(conversationId).data ?? NONE_M
}

export function useCreateConversation() {
  return useMutation({ mutationFn: (input: NewConversation) => commands.createConversation(input) })
}

export function useUpdateConversation() {
  return useMutation({ mutationFn: ({ id, patch }: { id: string; patch: ConversationPatch }) => commands.updateConversation(id, patch) })
}

export function useDeleteConversation() {
  return useMutation({ mutationFn: (id: string) => commands.deleteConversation(id) })
}

export const createConversation = (input: NewConversation) => commands.createConversation(input)
export const updateConversation = (id: string, patch: ConversationPatch) => commands.updateConversation(id, patch)
/** FR-LLM-AS-13: archived conversations are read-only and deleted for good after three days. */
export const archiveConversation = (id: string) => commands.archiveConversation(id)
export const restoreConversation = (id: string) => commands.restoreConversation(id)
export const addMessage = (conversationId: string, input: NewMessage) => commands.addMessage(conversationId, input)
export const updateMessage = (id: string, patch: MessagePatch) => commands.updateMessage(id, patch)

/** The conversation as it is now in the database; `undefined` when it was deleted. For code outside React. */
export async function fetchConversation(id: string): Promise<Conversation | undefined> {
  return (await commands.listConversations()).find((c) => c.id === id)
}

/** The messages as they are now in the database. For code outside React. */
export const fetchConversationMessages = (conversationId: string) => commands.listMessages(conversationId)

/** One message with its card, from the database; for code that has only ids (saving a page). */
export async function fetchMessage(conversationId: string, messageId: string): Promise<ChatMessage | undefined> {
  return (await commands.listMessages(conversationId)).find((m) => m.id === messageId)
}

/** *Retry* on a failed assistant message. */
export function useRetryChatJob() {
  return useMutation({ mutationFn: ({ messageId, modelId }: { messageId: string; modelId: string | null }) => commands.retryChatJob(messageId, modelId) })
}

/** *Use the extracted text as is* (FR-KN-UP-10). */
export function useDraftAsIs() {
  return useMutation({ mutationFn: (messageId: string) => commands.draftAsIs(messageId) })
}

/** A chat message while the conversation has a draft card (FR-KN-UP-7). */
export function reviseDraft(input: ReviseInput) {
  return commands.reviseDraft(input)
}

/** Size of one upload chunk: about 2 MB (as bytes, before base64). */
const CHUNK = 2 * 1024 * 1024

/** The part of a data URL after the comma: the base64 text. */
function readBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error(getT().assistant.file.readFailed))
    reader.onload = () => resolve(String(reader.result).split(',', 2)[1] ?? '')
    reader.readAsDataURL(blob)
  })
}

/** Sends a file to Rust in chunks (FR-KN-UP-2: no size limit) and returns the upload id. `onProgress` gets 0..1. */
export async function uploadFile(file: File, onProgress?: (fraction: number) => void): Promise<string> {
  const id = await commands.uploadBegin(file.name, file.size)
  try {
    for (let at = 0; at < file.size; at += CHUNK) {
      await commands.uploadChunk(id, await readBase64(file.slice(at, at + CHUNK)))
      onProgress?.(Math.min(1, (at + CHUNK) / Math.max(1, file.size)))
    }
  } catch (e) {
    await commands.uploadCancel(id).catch(() => undefined)
    throw e
  }
  return id
}

/** Uploads `file` and starts the file-draft job (FR-KN-UP-7); the messages arrive through the queries. */
export async function startFileDraft(file: File, input: Omit<FileDraftInput, 'uploadId'>, onProgress?: (fraction: number) => void) {
  const uploadId = await uploadFile(file, onProgress)
  try {
    return await commands.startFileDraft({ ...input, uploadId })
  } catch (e) {
    await commands.uploadCancel(uploadId).catch(() => undefined)
    throw e
  }
}
