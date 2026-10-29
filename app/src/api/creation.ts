// Task → knowledge, the merge screen's help and *Suggest groups* (M5, D-41). All on the default model;
// without one they fail with `llm_unavailable` (`isLlmUnavailable`) and the screens offer the manual way.
// Plain functions, not mutation hooks: a stopped job or a missing model is not an error to toast.
import {
  commands,
  type DraftText,
  type GroupSuggestions,
  type MergeBlocksReport,
  type SuggestMergeInput,
  type TaskDraftInput,
} from './bindings'

export type { BlockMatch, DraftText, GroupProposal, GroupSuggestions, MergeBlocksReport, NewGroupName } from './bindings'

/** FR-TASK-KN-2. Pass a `streamId` (`newStreamId()`) to stream the text and to *Stop* it. */
export const draftFromTask = (input: TaskDraftInput): Promise<DraftText> => commands.draftFromTask(input)

/** FR-KN-DU-5. The existing page's title is kept. Nothing is saved. */
export const suggestMerge = (input: SuggestMergeInput): Promise<DraftText> => commands.suggestMerge(input)

/** FR-KN-DU-3. `byMeaning: false` when the search model can't be used: keep the keyword overlap. */
export const matchMergeBlocks = (existing: string[], incoming: string[]): Promise<MergeBlocksReport> =>
  commands.matchMergeBlocks({ existing, incoming })

/** FR-KN-GR-3. `jobId` is the *Stop* id. */
export const suggestGroups = (categoryId: string, jobId?: string): Promise<GroupSuggestions> =>
  commands.suggestGroups(categoryId, jobId ?? null)
