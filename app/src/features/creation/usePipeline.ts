// The save-pipeline checks one after another (§5.4, UI-KN-6):
//   0 topics + 1 group    one `check_page` call (the model);
//   2 duplicates          `find_duplicates` (also without a model);
//   3 review.
// While the user edits, `refreshDuplicates` runs step 2 again on the new text (no model), and
// `refreshTerms` asks the model for glossary terms again when the user clicks for it.
// No usable model (`llm_unavailable`) skips steps 0-1 and the page is saved "not checked"
// (FR-KN-SV-3); another error waits for *Retry* / *Skip* (skipping also means "not checked").
import { useCallback, useEffect, useRef, useState } from 'react'
import type { CheckReport, DuplicateReport, GroupPick } from '@/api/checks'
import { checkPage, findDuplicates, pageMarkdown } from '@/api/checks'
import { errorMessage } from '@/api/client'
import { isCancelled, isLlmUnavailable } from '@/api/llm'
import { readSettings } from '@/api/settings'
import type { TermSuggestion } from '@/api/bindings'
import { countWords } from '@/lib/format'

export type StepState = 'pending' | 'active' | 'done' | 'skipped'
export interface TopicResult {
  diverse: boolean
  topics: string[]
  words: number
  /** True when the rule decided no topic check is needed (small manual page). */
  notNeeded?: boolean
}
export interface Subject {
  title: string
  body: string
  categoryId: string
  source: 'manual' | 'file' | 'task'
  /** The user already picked a group: step 2 is skipped. */
  groupId?: string | null
  /** A page that must not be found as a duplicate (the page itself, or the page being merged away). */
  excludePageId?: string
}
/** Why a step did not run: by the rules, no model, the user's *Skip* after an error, or a group already picked. */
export type SkipWhy = 'rule' | 'unavailable' | 'error' | 'picked'
export interface PipelineError {
  step: 'check' | 'duplicates'
  message: string
}
/** What the pipeline lets the review screen answer when the topic check finds several topics. */
export type DiverseAnswer = 'changed' | void

const inputOf = (s: Subject) => ({ categoryId: s.categoryId, title: s.title, body: pageMarkdown(s.title, s.body) })
/** What a duplicate list was made for. */
const dupKey = (i: { categoryId: string; body: string }) => `${i.categoryId}\n${i.body}`

export function usePipeline({
  getSubject,
  onDiverse,
}: {
  getSubject: () => Subject | null | undefined
  /** Called when the topic check finds several topics; the run waits until it resolves. `'changed'`: the text was rewritten, so the check runs again. */
  onDiverse?: (t: TopicResult) => Promise<DiverseAnswer>
}) {
  // steps: 0 topic check, 1 group suggestion, 2 duplicate check, 3 review
  const [steps, setSteps] = useState<StepState[]>(['pending', 'pending', 'pending', 'pending'])
  const [topic, setTopic] = useState<TopicResult | null>(null)
  const [report, setReport] = useState<CheckReport | null>(null)
  const [duplicates, setDuplicates] = useState<DuplicateReport | null>(null)
  const [terms, setTerms] = useState<TermSuggestion[]>([])
  const [skip, setSkip] = useState<{ topic: SkipWhy | null; group: SkipWhy | null }>({ topic: null, group: null })
  const [unavailable, setUnavailable] = useState<string | null>(null)
  const [error, setError] = useState<PipelineError | null>(null)
  const [checked, setChecked] = useState(false)
  const [dupRefreshing, setDupRefreshing] = useState(false)
  const [termsChecking, setTermsChecking] = useState(false)
  const lastDup = useRef<string | null>(null)
  const runId = useRef(0)
  const decision = useRef<((a: 'retry' | 'skip') => void) | null>(null)
  const shown = useRef<string[]>([])
  const cb = useRef({ getSubject, onDiverse })
  cb.current = { getSubject, onDiverse }

  const run = useCallback(async (opts?: { fresh?: boolean; topicDone?: boolean }) => {
    const id = ++runId.current
    const alive = () => runId.current === id
    const set = (i: number, v: StepState) => setSteps((s) => s.map((x, j) => (j === i ? v : x)))
    decision.current?.('skip')
    decision.current = null
    setSteps(['active', 'pending', 'pending', 'pending'])
    setTopic(null)
    setReport(null)
    setDuplicates(null)
    setSkip({ topic: null, group: null })
    setUnavailable(null)
    setError(null)
    setChecked(false)
    if (opts?.fresh) {
      shown.current = []
      setTerms([])
    }

    const s0 = cb.current.getSubject()
    if (!s0) return
    const words = countWords(s0.body)
    // FR-KN-SV-1: manual pages only count the topic result above the word threshold.
    const topicNeeded = s0.source !== 'manual' || words > readSettings().wordWarningThreshold
    const groupNeeded = !s0.groupId
    let complete = true

    /** One call with *Retry* / *Skip* on an error; `undefined` = skipped, `null` = the run was replaced or stopped. */
    async function attempt<T>(step: PipelineError['step'], call: () => Promise<T>): Promise<T | undefined | null> {
      for (;;) {
        try {
          return await call()
        } catch (e) {
          if (!alive() || isCancelled(e)) return null
          if (step === 'check' && isLlmUnavailable(e)) {
            setUnavailable(errorMessage(e))
            throw e
          }
          setError({ step, message: errorMessage(e) })
          const answer = await new Promise<'retry' | 'skip'>((res) => (decision.current = res))
          decision.current = null
          if (!alive()) return null
          setError(null)
          if (answer === 'skip') return undefined
        }
      }
    }

    const subject = () => cb.current.getSubject() ?? s0
    const input = () => inputOf(subject())

    // Steps 0-1: one call.
    let result: CheckReport | undefined
    let why: SkipWhy = 'error'
    try {
      const r = await attempt('check', () => checkPage({ ...input(), shownTerms: shown.current }))
      if (r === null) return
      result = r
    } catch {
      why = 'unavailable' // no usable model (FR-KN-SV-3)
    }
    if (!alive()) return
    if (result) {
      setReport(result)
      const fresh = result.terms.filter((t) => !shown.current.some((x) => x.toLowerCase() === t.term.toLowerCase()))
      shown.current = [...shown.current, ...fresh.map((t) => t.term)]
      setTerms((old) => [...old, ...fresh])
      if (topicNeeded && !opts?.topicDone) {
        const t: TopicResult = { diverse: !result.oneTopic, topics: result.topics, words }
        setTopic(t)
        if (t.diverse && cb.current.onDiverse) {
          const answer = await cb.current.onDiverse(t)
          if (!alive()) return
          // The draft was rewritten: the group, tags and terms of the first check no longer fit.
          if (answer === 'changed') return void run({ topicDone: true })
        }
        set(0, 'done')
      } else {
        setTopic({ diverse: false, topics: [], words, notNeeded: true })
        set(0, 'skipped')
        setSkip((k) => ({ ...k, topic: 'rule' }))
      }
      if (groupNeeded) set(1, 'done')
      else {
        set(1, 'skipped')
        setSkip((k) => ({ ...k, group: 'picked' }))
      }
    } else {
      complete = false
      set(0, 'skipped')
      set(1, 'skipped')
      setSkip({ topic: why, group: groupNeeded ? why : 'picked' })
    }

    // Step 2: duplicates, with or without a model.
    set(2, 'active')
    const dup = await attempt('duplicates', () => {
      const i = input()
      lastDup.current = dupKey(i)
      return findDuplicates({ ...i, excludePageId: subject().excludePageId ?? null })
    })
    if (dup === null || !alive()) return
    if (dup) {
      setDuplicates(dup)
      set(2, 'done')
    } else {
      complete = false
      set(2, 'skipped')
    }
    setChecked(complete)
    set(3, 'active')
  }, [])

  useEffect(() => {
    void run({ fresh: true })
    return () => {
      runId.current++
    }
  }, [run])

  /** Step 2 again on the current text, if it changed since the list was made. No model, so it costs nothing. */
  const refreshDuplicates = useCallback(async () => {
    const s = cb.current.getSubject()
    if (!s) return
    const i = inputOf(s)
    const key = dupKey(i)
    if (key === lastDup.current) return
    const id = runId.current
    setDupRefreshing(true)
    try {
      const dup = await findDuplicates({ ...i, excludePageId: s.excludePageId ?? null })
      if (runId.current !== id) return
      lastDup.current = key
      setDuplicates(dup)
    } catch {
      // The earlier list stays; the next pause tries again.
    } finally {
      setDupRefreshing(false)
    }
  }, [])

  /** Asks the model for glossary terms in the current text (one call). Only new terms are added; resolves to how many. */
  const refreshTerms = useCallback(async () => {
    const s = cb.current.getSubject()
    if (!s) return 0
    setTermsChecking(true)
    try {
      const r = await checkPage({ ...inputOf(s), shownTerms: shown.current })
      const fresh = r.terms.filter((t) => !shown.current.some((x) => x.toLowerCase() === t.term.toLowerCase()))
      shown.current = [...shown.current, ...fresh.map((t) => t.term)]
      setTerms((old) => [...old, ...fresh])
      return fresh.length
    } finally {
      setTermsChecking(false)
    }
  }, [])

  const retry = useCallback(() => decision.current?.('retry'), [])
  const skipStep = useCallback(() => decision.current?.('skip'), [])
  const running = steps[3] !== 'active'
  const group: GroupPick | null = report?.group ?? null
  return { steps, topic, report, suggestion: group, tags: report?.tags ?? [], terms, duplicates, skip, unavailable, error, checked, running, rerun: run, retry, skipStep, refreshDuplicates, dupRefreshing, refreshTerms, termsChecking }
}
