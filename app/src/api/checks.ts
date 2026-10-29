// The save-pipeline checks (M5, FR-KN-SV-1..4): the `check` call, the duplicate check and
// *Keep one topic only*. Plain calls; the pipeline (features/creation/usePipeline.ts) orders them.
import { commands, type CheckInput, type CheckReport, type DuplicateInput, type DuplicateReport, type DraftText, type KeepTopicInput } from './bindings'
import { newStreamId } from './llm'

export type { CheckReport, DuplicateHit, DuplicateReport, GroupPick } from './bindings'

/** The page Markdown with its `# title` line, as the checks expect it. */
export function pageMarkdown(title: string, body: string): string {
  return /^#\s/.test(body) ? body : `# ${title.trim()}\n\n${body}`
}

/** Steps 1-2: topics, group, tags and glossary suggestions in one model call. Rejects with `llm_unavailable` without a default model. */
export const checkPage = (input: CheckInput): Promise<CheckReport> => commands.checkPage(input)

/** Step 3: pages that may cover the same topic (works without a model: keyword fallback). */
export const findDuplicates = (input: DuplicateInput): Promise<DuplicateReport> => commands.findDuplicates(input)

/** *Keep one topic only*; the text streams under `input.streamId` (`useLlmStream`) and *Stop* is `cancelJob(streamId)`. */
export const keepOneTopic = (input: KeepTopicInput): Promise<DraftText> => commands.keepOneTopic(input)

export { newStreamId }
