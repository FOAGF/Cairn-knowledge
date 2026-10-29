// Sending a message in the Assistant (UI-AS-3, FR-LLM-AS-11, -12). Rust adds the user and
// assistant messages, titles the conversation and runs the job (`commands/assistant.rs`); this
// file only picks the path: a plain message while a draft card is active refines the draft (M5),
// anything else is a normal message, with a file or 🌐 when asked for.
import { sendMessage } from '@/api/assistant'
import { commands } from '@/api/bindings'
import { fetchConversationMessages, reviseDraft, uploadFile } from '@/api/conversations'
import { useBusy } from './busy'
import { activeDraftMessage } from './messages'

export interface SendOptions {
  modelId: string
  effort?: string
  /** A file sent with the message (FR-LLM-AS-11). */
  file?: File | null
  /** 🌐 for this message only (FR-LLM-AS-12). */
  web?: boolean
}

export async function sendChatMessage(conversationId: string, text: string, { modelId, effort, file, web }: SendOptions) {
  if (!file && !web && activeDraftMessage(await fetchConversationMessages(conversationId))) {
    await reviseDraft({ conversationId, text, modelId, effort: effort ?? null })
    return
  }
  let uploadId: string | null = null
  if (file) {
    const { setUploading } = useBusy.getState()
    setUploading(conversationId, 0)
    try {
      uploadId = await uploadFile(file, (f) => setUploading(conversationId, f))
    } finally {
      setUploading(conversationId)
    }
  }
  try {
    await sendMessage({ conversationId, text, modelId, effort: effort ?? null, uploadId, web: !!web })
  } catch (e) {
    if (uploadId) await commands.uploadCancel(uploadId).catch(() => undefined)
    throw e
  }
}
