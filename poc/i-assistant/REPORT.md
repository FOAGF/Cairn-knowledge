# M6 prompt check (`poc/i-assistant`)

**Question:** do the M6 assistant prompts (D-44) work on the owner's models before the Assistant is built? The templates
live in `crates/cairn-core/prompts/` (`ask-plan`, `ask-answer`, `ask-action`, `ask-page`, `edit-page`, `doc-terms`,
`ask-web`, with the fragments `_answer-rules.md` and `_cite-rules.md`); the README there documents the variables, the
list formats, the JSON shapes and the app-side checks. This folder checks them the way `poc/h-create` checked the M5
ones: the harness renders the real template files, sends them through the real sidecar, applies the app-side checks,
and judges what the app would show.

**Files:**

| File | What |
|---|---|
| `samples.py` | A made-up workspace: 3 projects (Site operations `OPS`, Kestrel software `KSW`, Home lab `LAB`, where "MMU" means memory management unit), 12 pages (one in Chinese), 13 tasks across every status, with comments, links and due dates, a glossary with global and project entries, an attached torque sheet (two parts) and a shot report for the term check. Distractor pages share ordinary words with the right one ("setup", "reader", "use", "hose"). |
| `formats.py` | The list formats of the README (`projects`, `scope`, `history`, `cited`, `sources`, `candidates`, the notes) and the app-side checks (`check_plan`, `check_action`, `markers`). The Rust core will implement the same formats, so this file is the reference. |
| `llm_check.py` | Owner-run. 39 checks, 39 calls per target plus at most one `check-retry` per JSON check. `--dry-run` renders everything and prints the sizes without calling a model. Full replies, the messages sent and the raw web-probe shapes go to `out/<target>/` (gitignored). |
| `relevance_check.py` | The minimum relevance, calibrated locally with the real search model (no LLM; see below). |

**Run:**
```
sidecar/.venv/bin/python poc/i-assistant/llm_check.py --dry-run
sidecar/.venv/bin/python poc/i-assistant/llm_check.py poc/a-llm/config.toml [--only NAME] [--check ID ...]
```

## The checks

The judges first apply what the app will do (README "Local checks"): `ask-plan` JSON through `check_plan` (unknown kind
to `question`, unknown projects dropped, dates parsed or dropped), `ask-action` through `check_action` (unknown labels
and enum values dropped, empty changes dropped, no delete op exists), `ask-answer` markers through `markers` (numbers
that are not a source removed), terms through the M5 `filter_terms`. A check that passes only because the app repaired
the reply is reported as a failure of the prompt (the plan judge lists what was repaired).

| Check | What is judged |
|---|---|
| `plan-filler` | "how do I set up the Kestrel reader on the tablet?": kind `question`; search words keep "Kestrel" and "reader", no filler ("how", "set up", "the"), at most 6 words |
| `plan-find` | "blocked tasks in Kestrel software about licensing": `find`, project, type task, status blocked, search "licens…", one intro line |
| `plan-due` | "what's due next week?": `find`, tasks, due from next Monday to next Sunday (computed from the real date) |
| `plan-follow` | after an answer citing OPS-41 and OPS-47, "tell me more about the second one": refs `["c2"]`, no search words |
| `plan-scope` | scope = Site operations; "what do the Home lab notes say about the bootloader crash?": `outside_scope ["Home lab"]`, no project filter |
| `plan-general` | "what is the flash point of diesel?": `general`, no search |
| `plan-create` | "create a task to order a spare HX-2210 hose …": `create` |
| `plan-change` | "mark the hose task as done": `change`, search "hose" |
| `plan-file-q` / `plan-file-alone` / `plan-file-page` | torque-sheet.pdf attached with a question / alone / with "make a page from this": `file_question` / `file_draft` / `file_draft` |
| `plan-zh` | "MMU-07 的 HX-2210 软管为什么老是开裂？": search words in Chinese with "MMU-07" and "HX-2210" kept (the relevance calibration's finding: a dropped code cost 0.2 of similarity) |
| `ans-cite` | the Kestrel reader question with the barcode-reader and hose-tester pages as distractors: cites [3], not [1] or [2], no invalid number, no source list, keeps the PIN 2468 |
| `ans-gloss` | the Home lab "Board crash" note with both MMU entries in the glossary: "memory management", cites the lab note, does not apply the mining entry |
| `ans-unknown` | "What does Pelican do?" (in no note, no entry): says the notes don't cover it, no invented meaning, no citation on a sentence about Pelican (loosened after the owner's run) |
| `ans-general` | kind `general`, no sources: the "General knowledge, not from your notes" line, no markers, a plausible flash point |
| `ans-outscope` | scope = Site operations, the out-of-scope note for Home lab, no sources: says Home lab was not searched |
| `ans-follow` | the follow-up with history and `refs_note`: answers about OPS-47 (MMU-03/05, clamp) and cites [2] |
| `ans-zh` | "P3W 湿孔装药前要先做什么？" over English sources: answers in Chinese (half or more Han characters), cites the procedure, keeps "30 分钟" and dewatering |
| `ans-file` | the torque sheet's parts as sources with an EP-40 page between them: 210 Nm, cites the file part (citing the page is allowed since the owner's run: it is relevant) |
| `ans-left-out` | "#OPS-48 what did the fitter find at the PM on 16 September?" OPS-48 (a long made-up task in `llm_check.py`) is sent cut, as the app sends a referenced item that is too long (D-83). The description and four comments go, with the `LEFT_OUT` note on the last one naming the nine left out, among them the fitter's report; OPS-42 follows. Judged: says that comment was not sent, or asks for it; a sentence saying the notes or the task lack it fails only when it doesn't put that down to what was sent and the reply nowhere says a part was not sent or asks for it (relaxed after the owner's second run, 2026-10-29: both models wrote "the notes don't say" next to "were not sent … paste"); no finding stated as fact; nothing of the report (suction hose, HX-1180, liner) |
| `act-create` | "create a task in Site operations to order a spare HX-2210 hose for MMU-07, high priority, due in two weeks. Log it in SiteTrak too.": one `create_task`, the project, title, priority, type `general`, due 12–16 days from today; terms after the filter contain no everyday words or part numbers (SiteTrak as `unknown` is the good answer) |
| `act-three` | after a find list of three tasks, "set all three to high priority" with a fourth candidate: three `update_task` with only `priority: high`, targets t1–t3 |
| `act-comment` | "add a comment to OPS-41: the SL-75 sleeve held for two weeks, no wear": one `comment` on t1 with those words |
| `act-ambig` | "mark the Kestrel task as done" with four Kestrel tasks: no change, a question naming them |
| `act-delete` | "delete OPS-45, it's done anyway": no change, no delete op in the raw JSON, the reply says the chat cannot delete |
| `act-from-latest` | the owner's case (2026-10-28) on a made-up task: "Create me another task from #KSW-12 latest comment about wifi problem?", KSW-12 sent whole (an older comment touches the wifi too), KSW-10 as a search result. Judged: one `create_task` in Kestrel software whose description carries the latest comment (bay 4, AP-7, the two trucks), at most 200 characters longer than it, with no guessed cause or fix and nothing from the other sources (the wifi password, the PIN, 06:00, KSW-10); a `relates` link to KSW-12 and none to KSW-10; priority medium and no due date; no `[n]` in any field |
| `act-left-out` | the same request with KSW-12 cut and its latest comment left out (the `LEFT_OUT` note): no change (not a task from the older wifi comment); the reply says that comment was not sent, naming it, in English (DeepSeek answered the English request in Chinese on 2026-10-29; all four new `act-*` checks now judge the request's language) |
| `act-create-src` | `act-create` with the sources the app sends for it (OPS-41's description and two comments, the hose-leak page's cause and fix): `act-create`'s judge, and nothing from the sources in the title or description (SL-75, the boom clamp, 40 mm, 12 bar, the elbow), no `[n]` |
| `act-quote-page` | "add a comment to #OPS-41 with the pressure test step from [[How to use the auger hose tester]]", both sent whole: one `comment` on t1 with 12 bar and 5 minutes in the page's wording (pump up, wait, or the 0.5 bar limit), nothing from OPS-41's own comments, no `[n]` |
| `page-from-chat` | "make a page about what we know on EP-40 calibration" from the calibration page, OPS-42 and a distractor: title line, 250 h, 0.982, 2%, 1,480 h kept; no `[n]`; the barcode page left out; no torque invented |
| `page-left-out` | "#OPS-48 make a page for the operators: what the flow loss looked like, and how the flow meter and the gassing line were ruled out", with OPS-48 cut to what the request needs, without the note (as `ask-page` gets it since the owner's run of 2026-10-29, when Claude put the note into the page), then the calibration section: title line; 118 → 92 kg/min, 1%, the gassing test kept; no `[n]`; no trace of the note (its words, "not sent", "left out", "paste", a question to the user); nothing from the comments left out |
| `edit-page` | the hose-tester page with "change the test duration from 5 to 10 minutes, and add to step 2 that the gauge must be zeroed with the hose disconnected": judged by line diff, only steps 2 and 3 may change |
| `edit-left-out` | the EP-40 calibration page with "add one line after step 4 saying what OPS-48 found when the MMU-05 flow meter was checked against the calibration tank", with OPS-48 cut after that comment (the note on it): a page, not a plain reply about what is missing; 1% added; no line removed or changed, at most 3 added; no trace of the note |
| `doc-terms` | the shot report: terms after the filter are not everyday or trade words (`NOT_TERMS`); Tanager (unknown) and PDR (high, explained in the text) are the wanted ones |
| `web-plain` / `web-stream` / `web-stable` | not prompt judgements: the model called with web search on, non-streamed and streamed, and (`web-stable`, added after the owner's run) a stable question the model would normally answer from memory, to check that the revised `ask-web` makes it search |

**The web probes** build the call with the sidecar's own `cairn_sidecar.ai.build_call` (so the route and the
authentication are exactly the app's), add LiteLLM's `web_search_options={"search_context_size": "low"}` (LiteLLM 1.105
maps it to Anthropic's `web_search_20250305` tool with `max_uses` 1) and call `litellm.completion` in-process. The
question is a fixed, harmless one that needs current information; nothing from the samples is sent.
`out/<target>/web-plain.json` and `web-stream.json` record the text, `provider_specific_fields` (`citations`,
`web_search_results`, per-chunk `citation`), `usage` (with `server_tool_use.web_search_requests`), the finish reason
and, on failure, the exception class, HTTP status, error kind and message (key scrubbed). The summary line is
"searched N times, M citations", "no search was made (the provider ignored the option)" or
"refused: <status> <kind> <message>". Expected: Claude on Foundry works; DeepSeek refuses or ignores the option.

## Verification without models (2026-10-15)

- `--dry-run`: all 30 checks render (about 1,000–2,200 tokens each; `ask-web` 100).
- An end-to-end run against a local mock (an OpenAI-compatible server answering each template with a plausible canned
  reply): the harness ran to the end, failing cleanly where the canned replies were wrong (the mock's "set up" filler,
  wrong citation numbers, a missing fact) and reporting the web probes as "no search was made", which is what an
  endpoint that ignores `web_search_options` gives.
- `poc/h-create/golden.py` refreshed `crates/cairn-core/prompts/testdata/golden.json` (18 templates).

## Minimum relevance (`relevance_check.py`, 2026-10-15)

**Setup:**
- A separate made-up workspace of 18 pages and 8 tasks.
- Pages are cut at headings, and each piece gets its glossary lines, as the index does (`vectors.rs` `embed_text`).
- Embedded with the app's search model: EmbeddingGemma q4 at 256 dimensions, through `cairn_sidecar --role embed`.
- 15 answerable questions with their relevant items labelled, and 4 questions the notes can't answer.
- Each question is scored twice: with focused search words, as `ask-plan` gives them, and as typed.
- An item's score is its best piece's cosine similarity.

**Results:**
- **A fixed cutoff alone separates poorly.** Relevant tasks, which are short pieces, score 0.41–0.59, while the best unrelated item reaches 0.54. The best relevant item of each question scores 0.49–0.81.
- **A floor plus a margin below the question's best item works better.**

  | Rule (focused words) | Relevant kept | Unrelated kept (19 questions) | Unanswerable with nothing sent |
  |---|---|---|---|
  | at least 0.40 | 24/25 | 32 | 4/4 |
  | **at least 0.40, and within 0.30 of the best** | **24/25** | **19** | **4/4** |
  | at least 0.40, and within 0.20 of the best | 20/25 | 5 | 4/4 |
  | at least 0.42, and within 0.25 of the best | 22/25 | 11 | 4/4 |

- **Chosen: at least 0.40 and within 0.30 of the best** (FR-LLM-AS-1).
  - The answer's own citations are the second filter.
  - The unanswerable questions' best unrelated item scored about 0.40, so the floor is a setting (Settings › Search).
- **Glossary lines in the pieces lift unrelated items a little.** With a floor of 0.40 alone, 32 unrelated items were kept with the glossary lines and 21 without.
  - They stay in the index, because they help long-form searches (M4, `poc/g-terms`).
  - A keyword match found only through glossary lines must pass the meaning rule as well. In the keyword run, "Kestrel loading records" matched one relevant task and one unrelated page that way.
- **Focused words beat the typed question, unless they drop a code.** "流量计 校准" scored the right page 0.49; the typed question, which kept "EP-40", scored 0.69. So `ask-plan` keeps names, codes and numbers, and `plan-zh` judges it.

## Results (owner's run, 2026-10-15)

| | DeepSeek-V4.1-Flash (Foundry, Chat Completions) | Claude Opus 5.5 (Foundry, Anthropic Messages, Entra) |
|---|---|---|
| `ask-plan` (12) | 12/12 | 12/12 |
| `ask-answer` (8) | 7/8 (`ans-gloss`) | 6/8 (`ans-unknown`, `ans-file`) |
| `ask-action` (5) | 5/5 | 5/5 |
| `ask-page`, `edit-page`, `doc-terms` | 3/3 | 3/3 |
| Web probes | **no search was made**: the option was ignored, without an error | searched once, 2–3 citations |

**Time and size:**
- The first step takes 1.3–3 s on DeepSeek and 2–2.6 s on Claude.
- One DeepSeek `plan-create` call took 99 s, a provider delay. The sidecar's timeout and retry rule (M5) covers this.
- The prompts are 1,000–3,000 input tokens.

**The three failures, read in full: acceptable, no template change.**
- **`ans-unknown` (Claude):** it said the notes don't cover Pelican, then offered "The closest match is Kestrel, the shift handover app [2]". The citation is correct; the judge was too strict about citing nothing.
- **`ans-file` (Claude):** it cited the EP-40 calibration page for "after a stator change … recalibrate the EP-40 flow meter". This is true and relevant, so the page wasn't a distractor for that question. The judge was wrong.
- **`ans-gloss` (DeepSeek):** it answered with the Home lab meaning and cited correctly, then added "(Note: … in Site operations notes, MMU means the Mobile Manufacturing Unit truck.)". This is a minor breach of the scope rule. It didn't apply the wrong meaning, so it is recorded and not tuned.

**Web search:**
- **Claude:**
  - The route is `azure_ai/claude-opus-5-5`.
  - The citations come in `provider_specific_fields.citations` (`url`, `title`, `cited_text`), and in 3 streamed `citation` chunks.
  - The number of searches is in `usage.server_tool_use.web_search_requests` (1). One search cost about 18,300 input tokens, because the results count as input.
  - LiteLLM surfaces the server-side search as `tool_calls` with `srvtoolu_` ids and `provider_specific_fields.web_search_calls` in the chunks. The sidecar must not treat them as client tool calls.
- **DeepSeek:**
  - The route is `openai/DeepSeek-V4.1-Flash`. There was no error, no search, and no usage field for searches.
  - It answered from memory, "Python 3.14.0 … As of October 2026" (Claude's search found 3.15.0, released on 9 October 2026), with a python.org Markdown link that looks like a citation.
- **Resulting decision (spec FR-LLM-AS-12):**
  - A reply without a search is labelled *No web search was made*, and its links are not shown as sources.
  - On Chat Completions and local models, such a reply removes the *Web search* tick, with a warning.
  - `ask-web` now tells the model to search before answering. This was rewritten by a Fable sub-agent, with a new probe `web-stable`.

**Web probes after the `ask-web` rewrite** (owner, 2026-10-15, `--check web-plain web-stream web-stable`):
- **Claude** searched once in all three probes, with 2 citations each. That includes `web-stable` (the boiling point of water), which it would otherwise answer from memory. The rewrite works.
- **DeepSeek** reported "no search was made" in all three, so the label and untick rule (FR-LLM-AS-12) is needed.

**After the no-search mode was added** (owner, 2026-10-16, `--check web-plain web-stream web-stable web-off`):
- **Claude** searched once in each web probe, with 2, 3 and 2 citations.
- **DeepSeek** made no search again.
- **`web-off`** (the `ask-web` text used when a provider refuses web search) passed 6/6 on both models. Each reply said in its first sentence that it was not from a web search and may be out of date, and gave no web address.

