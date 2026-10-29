// The Assistant (M6, D-44, spec §6.4). A message runs as a background job in Rust
// (`commands/assistant.rs`): its steps, answer, find list or proposal are stored on the assistant
// message, so the UI only reads `useMessages` (refreshed by `DataChanged { conversations }`) and
// the streamed text (`useLlmStream(assistantMessageId)`). *Stop* is `cancelJob(assistantMessageId)`.
import { useQuery } from '@tanstack/react-query'
import { commands, type ChatJobStarted, type ChatMessage, type HistoryUsage, type SendInput } from './bindings'

export type {
  AnswerBasis,
  AnswerPart,
  FindPart,
  FoundItem,
  HistoryUsage,
  LinkTo,
  Proposal,
  ProposalRow,
  ProposedChange,
  RowState,
  SendInput,
  SourceKind,
  SourceRef,
} from './bindings'

/** Opens a web source in the system browser (the window never leaves the app, D-43). */
export const openUrl = (url: string): Promise<null> => commands.openUrl(url)

/** Sends a message in a normal conversation (also with a file: `uploadFile` first, or 🌐 `web`). */
export const sendMessage = (input: SendInput): Promise<ChatJobStarted> => commands.sendMessage(input)

/** *Search all categories* under an answer that named categories outside the scope (FR-LLM-AS-4). */
export const searchAllCategories = (messageId: string): Promise<string> => commands.searchAllCategories(messageId)

/** What the composer shows: history left for this conversation with this model (FR-LLM-AS-10). */
export function useHistoryUsage(conversationId: string | undefined, modelId: string | null) {
  return useQuery<HistoryUsage>({
    // Under ["conversations", ...] so a new message refreshes it.
    queryKey: ['conversations', 'history', conversationId, modelId],
    queryFn: () => commands.historyUsage(conversationId ?? '', modelId),
    enabled: !!conversationId,
  })
}

/** *Widen for this conversation*. */
export const widenHistory = (conversationId: string, modelId: string | null): Promise<HistoryUsage> =>
  commands.widenHistory(conversationId, modelId)

// ------------------------------------------------------------ the proposal card (FR-LLM-AS-3)

export const setProposalRow = (messageId: string, row: number, ticked: boolean): Promise<ChatMessage> =>
  commands.setProposalRow(messageId, row, ticked)

/** Applies the ticked rows; each row records its result. */
export const confirmProposal = (messageId: string): Promise<ChatMessage> => commands.confirmProposal(messageId)

export const rejectProposal = (messageId: string): Promise<ChatMessage> => commands.rejectProposal(messageId)
