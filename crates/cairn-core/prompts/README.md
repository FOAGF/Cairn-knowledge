# Prompt templates

The text of every LLM call Cairn makes in M5 (D-41) and in the M6 assistant (D-44), one file per call. The
Rust core includes them with `include_str!` and renders them with plain string replacement;
`poc/h-create/llm_check.py` (M5) and `poc/i-assistant/llm_check.py` (M6) load the same files, so the
harnesses test exactly what the app sends. The M6 templates are `ask-plan`, `ask-answer`, `ask-action`,
`ask-page`, `edit-page`, `doc-terms` and `ask-web`; they reuse `_glossary.md`, `_term-rules.md` and
`_page-rules.md`, and add `_answer-rules.md` and `_cite-rules.md`.

**How templates are written (owner's rule, 2026-10-15; changed 2026-10-26):**
1. A new template, or a rewrite of one, is written by an Opus sub-agent at max effort (a Fable sub-agent until 2026-10-26).
2. The sub-agent adds judges for it to `poc/h-create/llm_check.py` (M5) or `poc/i-assistant/llm_check.py` (M6).
3. The owner runs that check on their models before the template is used.

## Format

- `<id>.md` is a template; `_<name>.md` is a fragment that templates include.
- A template starts with front matter between two `---` lines:

  | Key | Meaning |
  |---|---|
  | `id` | the file name without `.md` |
  | `purpose` | one line, with the spec IDs it serves |
  | `model` | `conversation` (the model picked in the chat, FR-LLM-ST-2), `default` (background model, FR-LLM-AS-6) or `ocr` |
  | `output` | `page` (Markdown starting with `# `), `json`, `text`, or `page-or-text` |
  | `stream` | `true` when the text is shown as it arrives |
  | `variables` | a list of `name: one-line meaning, and what the app passes when it is absent`; `[]` when there are none |

- After the front matter come the messages. A message starts with a marker line `<<<system>>>` or
  `<<<user>>>` and runs to the next marker or the end of the file. Headings are never markers, because
  the prompts contain `## ` examples. A template has at most one system message, then one user message;
  `check-retry` has only a user message.
- `{{name}}` is a placeholder for a variable. `{{> name}}` includes the fragment `_name.md`.
- There are no conditionals or loops. Lists are formatted by the app (below), and every template reads
  correctly when an optional variable is `""`.

## Rendering rules

1. Expand the includes: replace each `{{> name}}` with the whole content of `_name.md`. Fragments contain
   placeholders but no includes.
2. Replace the placeholders in one pass: scan the expanded text for `{{name}}` and insert the value as it
   is. Values are never scanned again, so `{{` inside a source text is harmless. Do not loop over the
   variables with a global replace.
3. A placeholder that is not in the template's `variables`, a variable that is declared but not passed,
   and a passed variable that is not declared are all errors. A template declares every variable its
   messages use, including those inside the fragments it includes.
4. Each message's text is trimmed of leading and trailing blank lines. The front matter and the marker
   lines are not sent.
5. What the app appends after the rendered messages:
   - `ocr`: the rendered user text is the first content block, followed by one `{"type": "image"}`
     block per page image (PROTOCOL.md `llm.complete`); several pages per call are allowed.
   - `revise`: `system`, `user` (source) from the template, then `assistant` (the current draft), then
     the earlier turns of the chat about this draft in order, then the new user message as it was typed.
   - `check-retry`: appended to the `check`, `suggest-groups`, `ask-plan`, `ask-action` or `doc-terms`
     conversation after the assistant's unreadable reply, once (FR-KN-SV-1).
   - `ask-plan`, `doc-terms`, `ask-web`: nothing; the earlier turns reach `ask-plan` as the `history` text.
   - `ask-answer`, `ask-action`, `ask-page`, `edit-page`: `system` from the template, then the earlier turns of
     the conversation in order, oldest first, as far back as the history budget allows (FR-LLM-AS-10), then
     the rendered `user` message. An earlier user turn is the message as typed (a file sent with it is not
     repeated: its parts are sources). An earlier assistant turn is its text with the `[n]` markers removed
     (the item names are in this turn's sources); a find answer is its intro line followed by its rows, one
     per line as `- Task KEY: title` or `- Page: title`; a proposal card is its reply line followed by one
     line per change ("Created OPS-48", "Marked OPS-41 as done", "Rejected: ..."). The turn being answered
     is never in the history.
   - `ask-web`: sent with the provider's web search tool on (LiteLLM `web_search_options`, mapped to
     Anthropic's `web_search_20250305` tool; `search_context_size` low / medium / high gives `max_uses`
     1 / 5 / 10, so the "web searches per message" setting picks the size), and nothing else: no notes, no
     glossary, no earlier turns (FR-LLM-AS-12). The `search` variable holds the instruction for the mode
     (Other conventions): with the tool on, the prompt tells the model to search before it answers, because
     the user asked for a search by turning the switch on; a provider without a built-in search tool cannot
     obey, and the app detects that after the reply (Local checks). When the provider refused the request
     because of the tool, the same message is sent again without the tool and with the no-search text, so
     the model answers from what it knows and says so, instead of claiming or attempting a search.

## Templates

| id | Feature | Model | Output | Stream | Spec |
|---|---|---|---|---|---|
| `ocr` | transcribe scanned pages and images | ocr | text | no | FR-KN-UP-4, FR-KN-UP-4b, FR-KN-UP-9 |
| `draft-file` | draft from a file, with an optional instruction; also `.md`/`.txt` with an instruction or on request; excerpts when the text is too long and there is an instruction | conversation | page | yes | FR-KN-UP-3, 5, 8, 4a |
| `draft-part` | one part of a long text, no instruction | conversation | page | no | FR-KN-UP-4a |
| `draft-combine` | the part drafts into one page | conversation | page | yes | FR-KN-UP-4a, FR-KN-DOC-1 |
| `revise` | a chat message about the draft | conversation | page-or-text | yes | FR-KN-UP-7, 6, 8 |
| `task-draft` | task to knowledge page | default | page | yes | FR-TASK-KN-2 |
| `check` | topics, group, tags, term suggestions; also on a fresh file draft in the chat | default | json | no | FR-KN-SV-1, FR-KN-GR-2, FR-KN-GL-7, FR-KN-DV-1 |
| `check-retry` | the one retry after unreadable JSON | default | json | no | FR-KN-SV-1 |
| `keep-one-topic` | keep only the clicked topic | default | page | yes | FR-KN-DV-3 |
| `suggest-merge` | fill the merge result | default | page | yes | FR-KN-DU-5 |
| `suggest-groups` | groups for the ungrouped pages, in batches | default | json | no | FR-KN-GR-3 |
| `ask-plan` | the first step of every assistant message: kind of request, search words, filters, references, out-of-scope categories, the intro line of a find | conversation | json | no | FR-LLM-AS-1, 2, 3, 4, 9, 10, 11 |
| `ask-answer` | the answer from the numbered sources with `[n]` citations, or labelled general knowledge; earlier turns as chat messages | conversation | text | yes | FR-LLM-AS-1, 4, 8, 9, 10, 11 |
| `ask-action` | the proposed changes of a create or change request, for the confirmation card, with content from the sources where the request points at it (D-83); never deletes | conversation | json | no | FR-LLM-AS-3, FR-LLM-AS-14, FR-KN-GL-7 |
| `ask-page` | a new page from the sources and the conversation, into the draft card | conversation | page | yes | FR-LLM-AS-3, FR-KN-UP-7 |
| `edit-page` | the whole page with one change made, for the diff; a plain reply when the change can't be made | conversation | page-or-text | yes | FR-LLM-AS-3, FR-KN-ED-5 |
| `doc-terms` | glossary suggestions from an attached document, once per document | default | json | no | FR-KN-GL-7, FR-LLM-AS-11 |
| `ask-web` | the one message sent with web search on; carries only what the user typed | conversation | text | yes | FR-LLM-AS-12 |

Fragments: `_page-rules.md` (how a page is written), `_glossary.md` (the glossary block, FR-LLM-AS-8),
`_term-rules.md` (conservative glossary suggestions, FR-KN-GL-7), `_answer-rules.md` (language, glossary
scope, "notes don't cover this", labelled general knowledge, no invented meanings; FR-LLM-AS-9),
`_cite-rules.md` (the `[n]` markers; FR-LLM-AS-1).

Which M6 template runs when (D-44): every message goes through `ask-plan` first. Then, by its `kind`:
`question`, `file_question` and `general` run `ask-answer` (`general` with no sources, and no search);
`find` runs nothing more (Rust lists the matches under the plan's `intro`); `create` and `change` run
`ask-action`, with the turn's sources (D-83); its `create_page` changes then run `ask-page` and its `update_page` changes run `edit-page`
before the card is shown, so the card holds the draft page or the diff and one confirmation is enough;
`file_draft` runs the M5 `draft-file` flow. `doc-terms` runs once per attached document after the first
answer about it. A message with the web switch on runs `ask-web` only. The retry template is `check-retry`.

## How the app formats the list variables

Lines are joined with `\n`; an empty list is `""`.

| Variable | One line per item |
|---|---|
| `glossary` | `- Term (Other name, Other name) [global]: description` or `[category: Name]` for a category entry. The parenthesis is left out when there are no other names. Which entries: those whose term or other name occurs in the text by the FR-KN-GL-4 rules, plus the entries related to those, one step (FR-KN-GL-3). A line break inside a description becomes a space. |
| `groups` | `- Name: description`, or `- Name` when the description is empty. In `suggest-groups`, a name proposed by an earlier batch of the same run is added as `- Name: description (proposed in this run)`. |
| `comments` | `[YYYY-MM-DD] text`, oldest first; a comment's own line breaks are kept. |
| `topics` | `- topic`, as the check returned them. |
| `parts` | `Part i of n:` then `<part>`, the part draft, `</part>`, with a blank line between parts. |
| `pages` | `- [id] Title: start of body`, where the id is a short label the app makes for this batch (`p1`, `p2`, ...) and maps back to the page, and the start of the body is its first 300 characters or so with line breaks turned into spaces. |
| `categories` | `- Name (KEY): in scope` or `- Name (KEY): not in scope`, every category of the workspace. |
| `history` | `ask-plan` only: the last 6 turns, oldest first, each as `User: text` or `Assistant: text`, with a blank line between turns. An assistant text has its `[n]` markers removed; a text longer than about 600 characters is cut there with ` [...]`. |
| `cited` | `- c1: task OPS-41 "title" (Category)` or `- c1: page "Title" (Category)`: the items shown under the previous assistant message, in the order shown (the cited chips, or the rows of a find list), labelled `c1`, `c2`, ... in that order. The app maps the labels back. |
| `sources` | One block per source, blank line between blocks, numbered from 1 in the order sent (the best first). Line 1 is the head: `[n] Task KEY "Title" (Category; status: todo; priority: high; due: 2026-10-20)` (`due: none` without one; the status and priority ids as the plan uses them), `[n] Page "Title" (Category; group: Name)` (`group: none`), or `[n] File "name.pdf" (attached to this message; part 2 of 5)`. Line 2 says which piece: `Description:` or `Comment 2026-09-10:` for a task, `Section: Heading > Subheading` for a page (`Section: (top)` before the first heading); a file part has no line 2. Then the piece's text as it is. A glossary entry found by the search is not a source: it joins the `glossary` block instead. The items cited in the previous answer are sent again, as sources of this turn, with new numbers. The items the message refers to (FR-LLM-AS-14) come first, each sent whole, its pieces in order. One longer than about half the source budget is cut (D-83): its first piece and, of the others, the ones that best match the message, as many as fit, in order; the text of its last piece sent ends, after a blank line, with exactly `[Note from the app: this item is too long to send in full, so it was sent without {count} of its parts: {parts}. Do not guess what is missing; if it may hold what you need, say what was not sent, so the user can paste it.]` (`retrieve.rs` `LEFT_OUT`), where `{parts}` names the pieces left out as line 2 does (`Description`, `Comment 2026-09-01`, `Section: (top)`, `Section: Setup > Proxy`), each name once, and `{count}` counts those names. `ask-answer`, `ask-action` and `edit-page` get the same block; `ask-page` gets it without the note (`retrieve::without_left_out_notes`), because a page writer has no reply to say it in. |
| `candidates` | `ask-action` only. One block per item the change may target, blank line between blocks: the items of this turn's search, the items shown under the previous answer, and the plan's `refs`, one block per item. Tasks are labelled `t1`, `t2`, ..., pages `p1`, `p2`, ..., in that order. A task's head: `t1: Task KEY "Title" (Category; status: todo; priority: high; type: bug; due: 2026-10-20; labels: a, b; parent: KEY; links: KEY, KEY)` (`none` for an empty field), then the start of its description (about 600 characters, line breaks turned into spaces) or `(no description)`. A page's head: `p1: Page "Title" (Category; group: Name)`, then the start of its body without the title line. |

Other conventions:
- `today` is `YYYY-MM-DD (Weekday)` in the computer's local time zone, e.g. `2026-10-15 (Thursday)`.
- `scope` is `The conversation searches all categories.` or `The conversation searches only these categories: A, B.`
- `attachment` (`ask-plan`) is the attached file's name, or `none`.
- `out_of_scope_note` is `""` or exactly: `Not searched, because outside this conversation's scope: A, B. Say so in the answer; the user can widen the search with "Search all categories".` with the category names the plan returned in `outside_scope`.
- `refs_note` is `""` or `The message refers to these earlier items: source 2 (page "Title"); source 5 (task KEY).` for the plan's `refs`, which the app sends as sources of this turn.
- `message`, `question` and `request` are the user's message as typed, trimmed. `message` is `""` when only a file was sent.
- `search` (`ask-web`) is exactly one of two texts; the Rust core holds them as constants (`prompts::WEB_SEARCH_ON`, `prompts::WEB_SEARCH_OFF`). With web search on: `The user turned web search on for this message, so search the web before you answer, even when you think you already know the answer, and even for a simple question: the user wants the answer checked against current pages, not recalled. Base the answer on the pages the search returned and cite the ones you used. Do not write web addresses from memory: the citations are attached to your answer for you. When the search finds nothing reliable, say so plainly instead of guessing.` After the provider refused the search: `Web search is not available for this message, so answer from what you already know. Say plainly in your first sentence that this answer is not from a web search and may be out of date. Do not write web addresses from memory. When the question needs current information you cannot know, say so instead of guessing.`
- `content` (`ask-page`) and `change` (`edit-page`) come from the confirmed proposal (`create_page.content`, `update_page.change`); `title` (`ask-page`) from the proposal or `""`; `category` (`ask-page`) is the category on the card after the user's edits.
- `excerpt_note` is `""` or exactly this sentence: `These are excerpts of a longer text, picked by meaning for the instruction or request because the whole text is too long to send; "[...]" marks where text was left out. Work from the excerpts only.` The app joins the excerpts with a line containing `[...]`.
- `instruction` is the user's message as typed, trimmed; `""` when the file came without one.
- `source`, `page`, `draft`, `existing` and `new_content` are the Markdown as it is, title line included.

## JSON shapes

`check`:
```json
{"topics": ["..."], "one_topic": true,
 "group": {"name": "...", "existing": true} | null,
 "tags": ["..."],
 "terms": [{"term": "...", "meaning": "...", "confidence": "high" | "medium" | "low" | "unknown"}]}
```

`suggest-groups`:
```json
{"pages": [{"id": "p1", "group": "..." | null, "existing": true}],
 "new_groups": [{"name": "...", "description": "..."}]}
```

`ask-plan`:
```json
{"kind": "question" | "find" | "create" | "change" | "general" | "file_question" | "file_draft",
 "search": "focused words, names and codes kept, in the message's language; \"\" when nothing to search",
 "filters": {"categories": ["Name"], "types": ["task" | "page"],
             "statuses": ["todo" | "in_progress" | "blocked" | "done" | "wont_do"],
             "priorities": ["high" | "medium" | "low"],
             "due": {"from": "YYYY-MM-DD" | null, "to": "YYYY-MM-DD" | null} | null,
             "updated": {"from": ..., "to": ...} | null},
 "refs": ["c2"], "outside_scope": ["Name"], "intro": "one line for a find, else \"\""}
```

`ask-action`:
```json
{"changes": [
   {"op": "create_task", "category": "Name", "title": "...", "type": "bug" | "feature" | "general",
    "priority": "high" | "medium" | "low", "due": "YYYY-MM-DD" | null, "labels": ["..."], "description": "...",
    "links": [{"to": "t1" | "p1", "kind": "relates" | "parent" | "page"}],
    "terms": [{"term": "...", "meaning": "...", "confidence": "high" | "medium" | "low" | "unknown"}]},
   {"op": "update_task", "target": "t1", "status": "...", "priority": "...", "type": "...", "due": "..." | null,
    "labels": ["..."], "description": "...", "links": [...]},
   {"op": "comment", "target": "t1", "text": "..."},
   {"op": "create_page", "category": "Name", "title": "...", "content": "what it should contain"},
   {"op": "update_page", "target": "p1", "change": "what to change, in words"}],
 "reply": "one sentence shown with the card, a question, or why nothing is proposed"}
```
In `update_task` only the fields that change are present; `"due": null` clears the date; `labels` replaces
the list; `links` adds. There is no delete op of any kind, so a delete cannot be proposed.

`doc-terms`:
```json
{"terms": [{"term": "...", "meaning": "...", "confidence": "high" | "medium" | "low" | "unknown"}]}
```

Reading a JSON reply: take the text between the first `{` and the last `}`, after stripping a code fence if
the model added one, and parse it. When that fails or is not an object, send `check-retry` once; when
the retry fails too, the step fails with *Retry* / *Skip* (FR-KN-SV-1). For `ask-plan` a second failure
falls back to `{"kind": "question", "search": <the message's words>}` so the message is still answered.

## Local checks

What the prompt asks for, and what the app checks or repairs after the model has answered. The prompt-side
rules reduce bad answers; the app-side checks make sure a bad answer cannot reach the user unchanged.

### Pages (`draft-*`, `revise`, `task-draft`, `keep-one-topic`, `suggest-merge`, `ask-page`, `edit-page`)

Prompt-side: title on line 1, every fact kept, nothing added, filler dropped, no pictures, the source's
language, page only, no code fences.

App-side:
- Strip a code fence that wraps the whole reply, and blank lines around it.
- Line 1 must be `# title`. If not, the reply is still shown, with the title taken from the first heading
  anywhere in the text, else from the file name (`draft-*`), the task title (`task-draft`), the chosen
  topic (`keep-one-topic`) or the existing page (`suggest-merge`).
- `revise`: a reply starting with `# ` is a new draft card; anything else is a plain chat answer and the
  draft stays as it was.
- A reply cut off by the token limit (the sidecar's finish reason) is marked in the card, so the user does
  not save half a page.
- `draft-combine`: when the combined page is much shorter than the part drafts together (say, under half
  their length), show a warning on the card: the model probably summarised.
- `suggest-merge`: the result goes into the editable result area only; nothing is saved until the user
  clicks Save (NG-4). Lines containing `**Conflict:**` can be highlighted.
- `ask-page`: the reply goes into the M5 draft card (then *Review* and the save checks, which suggest the
  glossary terms: this template does not). `[n]` markers left in the page are removed as in `ask-answer`
  (the numbers mean nothing outside the chat). When line 1 is not `# title`, the title is the proposal's
  title, else the first heading, else the request's first words.
- `edit-page`: a reply that does not start with `# ` is a plain chat answer and the page stays as it was
  (like `revise`). Otherwise: strip markers as above; when line 1 is not the existing title, keep the
  existing title (a page change never renames: renaming is a form action); show the line diff against
  the current page and save through the page history on *Confirm* (FR-KN-ED-5). An identical page is shown
  as "No change was made". A reply cut off by the token limit is not offered for saving.

### `check`

Prompt-side: one JSON object; `one_topic` false only for unrelated subjects; an existing group spelled as
listed; tags not equal to the group; the term rules (`_term-rules.md`).

App-side:
- Parse as above, one retry with `check-retry`.
- `topics`: strings only, trimmed, empty ones dropped; when the list is empty, the page counts as one topic.
- `one_topic`: anything but `false` counts as true (the dialog only opens for a clear "no").
- `group`: a name that matches an existing group case-insensitively is that group (its canonical name),
  whatever `existing` says. An unmatched name with `existing: true` is treated as a new name. Ignored when
  the user already picked a group (FR-KN-SV-1).
- `tags`: trimmed, lowercased, de-duplicated; drop tags equal to the group name or the title; keep at most
  the first 4.
- `terms`, in this order; each dropped term is logged with the reason, so the harness and the debug log show
  what the filter did:
  1. `term` trimmed; empty, or not a string, is dropped. `confidence` outside the four values becomes `low`;
     `meaning` is `""` when missing.
  2. Drop a term that does not occur in the page text by the FR-KN-GL-4 rules: whole words only; a term
     written in capitals matches only in capitals; other terms ignore case; codes and plurals count
     (`MMU-07`, `MMUs` match `MMU`); Chinese terms match anywhere in Chinese text.
  3. Drop a term that is, or contains as a whole word, a glossary entry (term or other name) that applies to
     the page's category, by the same matching rules: `MMUs` and `AN tank` are dropped when `MMU` and `AN`
     are entries, because the entry already covers the term.
  4. Drop a term the user ignored in this category (FR-KN-GL-7).
  5. Drop an `unknown`-confidence term that has no capital letter, digit or non-Latin character. An
     internal name the model cannot know is written like a name or a code, never as everyday lowercase
     words; such a term is a generic component name (`boom clamp`) the prompt rules failed to stop.
  6. Drop a term that is a number with at most a short unit after it (`40 mm`, `2026-09-10`), and a term of
     more than 4 words (a phrase, not a term).
  7. De-duplicate case-insensitively, keeping the first.
  8. Drop a term already shown for this draft (FR-KN-GL-7: a term shown once is not shown again).
- Terms that survive go to the "N possible glossary terms" card. Nothing is added to the glossary without
  the user.

### `suggest-groups`

Prompt-side: one JSON object; ids as given; existing names as listed; null allowed; new names consistent.

App-side:
- Parse as above, one retry.
- Drop entries whose `id` is not in the batch; a page without an entry gets no suggestion.
- `group`: as for `check` (case-insensitive match to an existing group wins; unmatched names are new).
  A new name that matches a name proposed by an earlier batch, case-insensitively, is that name.
- `new_groups`: only names that some page in the batch uses are kept; descriptions are trimmed.
- The user approves each suggestion (FR-KN-GR-3); a new group is created when its first suggestion is
  approved.

### `ask-plan`

Prompt-side: one JSON object; the first kind that fits; focused search words that keep every name, code and
number of the message, in the message's language; filters only from what the message states; relative
dates from `today` with weeks starting on Monday; refs by label; out-of-scope categories by name; an intro
line for a find only. Seven worked examples are in the prompt.

App-side (`poc/i-assistant/formats.py`, `check_plan`):
- Parse as above, one retry with `check-retry`; a second failure falls back to a `question` whose search
  words are the message.
- `kind`: outside the seven values becomes `question`. `file_question` / `file_draft` without an attached
  file becomes `question`; any other kind with a file attached becomes `file_question` (the kind only steers
  the search; the file's parts are sources either way).
- `search`: a string, whitespace collapsed; anything else is `""`. The hybrid search ANDs the keyword
  words; when that finds nothing, the keyword side is retried with any word (OR) before giving up, and the
  meaning side always runs, so over-eager words cost recall but never an empty answer. An empty `search`
  with `question` means: search with the message itself.
- `filters.categories`: names matched to the categories case-insensitively; unknown names are dropped; a
  known name outside the scope moves to `outside_scope`. `types`, `statuses`, `priorities`: values outside
  the enums are dropped. `due` / `updated`: each end parsed as `YYYY-MM-DD` or dropped; a range with no
  valid end is `null`; a reversed range is swapped.
- `refs`: only labels that exist in `cited`; the items go into this turn's sources (and `refs_note`).
- `outside_scope`: only names of real categories outside the scope. The answer gets `out_of_scope_note`, and
  the reply offers *Search all categories* (FR-LLM-AS-4).
- `intro`: kept for `find` only, trimmed; when empty the app uses a default ("Matching items:").
- A `find` for tasks that names no status searches open tasks, as the prompt says; the user can change the
  filters on the result.

### `ask-answer`

Prompt-side: the answer rules (language, glossary scope, "Your notes don't cover this.", general knowledge
under a "General knowledge, not from your notes:" line, no invented meanings) and the citation rules (one
number per bracket, only existing numbers, only sources used, no source list at the end).

App-side (`formats.py`, `markers`):
- `[n]` markers whose `n` is not a source number are removed from the text; `[1, 2]` is split into
  `[1] [2]`. The surviving markers become links to the items (UI-AS-2).
- The chips under the answer show only the cited items, in order of first citation; the rest of the
  sources are under a collapsed *N more searched*.
- An answer with no valid marker is labelled *Not from your notes*. An answer that contains the line
  `General knowledge, not from your notes:` (or its Chinese form) is labelled *General knowledge* from that
  line on; a `general` kind is labelled so as a whole. The labels are UI only: the text is shown as written.
- A reply cut off by the token limit is marked (as drafts are).
- Nothing is checked about meaning: the prompt check (`poc/i-assistant`) is what guards that.

### `ask-action`

Prompt-side: one JSON object; labels only from the candidate list; one change per item; values only from
the enums; relative dates from `today`; a question instead of a guess when the target is ambiguous; no
delete (the shape has no delete op; the model explains instead); conservative `terms` on `create_task`.
The sources are material, never targets (D-83): their content goes into a description, a comment's text or
a page's content or change only where the request points at it ("from #KEY's latest comment"; "latest" is
the newest by date), and nothing else from them, not even as a reason or background; a task made from an
item's content relates to it; no `[n]` in any field. When the content pointed at is missing or may be in a
part the app left out (the `sources` note), nothing is proposed and the reply says what was not sent. Every
text field, the reply included, is in the request's language, whatever the language of the sources or of
the earlier turns.

App-side (`formats.py`, `check_action`), each dropped change logged with its reason:
- Parse as above, one retry with `check-retry`.
- `op` outside the five values: dropped. `target` (and each link's `to`) not in the candidate labels:
  dropped; a target of the wrong type for the op (a page label on `update_task`): dropped.
- `create_task` / `create_page`: a category that matches no category is replaced by the first category in
  scope (the user can change it on the card); an empty title drops the change. `type` outside the enum
  becomes `general`, `priority` outside the enum `medium`, an unparsable `due` `null`; labels are trimmed
  and empty ones dropped; links keep only valid labels with a kind that fits the target type (`page` for a
  page label, `relates` / `parent` for a task label) and never the task itself.
- `update_task`: only enum values are kept; a `due` that is neither a date nor `null` is ignored; a change
  that changes nothing after this is dropped; a duplicate (same target, same fields) is dropped.
- `comment` with an empty text, `update_page` with an empty change: dropped.
- No `[n]` handling: the fields are shown as written, so the prompt forbids source numbers in them.
- `terms` on `create_task` go through the `check` term filter above (`filter_terms`), against the new task's
  title and description, the glossary of its category and the category's ignored terms; survivors show as the
  "N possible glossary terms" card under the proposal.
- No changes and a non-empty `reply`: shown as a plain assistant message (the question, or why nothing can be
  done). No changes and no reply: "Nothing to change was found." Changes are shown on one card, one ticked
  row per change, with *Confirm*, *Edit…* and *Reject* (FR-LLM-AS-3); nothing is saved before *Confirm*,
  and the Rust commands that apply a confirmed card have no delete operation.

### `doc-terms`

Prompt-side: one JSON object; the term rules, with "the page" meaning the document.

App-side: parse as above, one retry; then exactly the `check` term filter (`filter_terms`) against the
document text (or its excerpts), the glossary entries of the conversation's scope and the ignored terms
of the category the document was attached in (the first in-scope category when there are several). Runs
once per attached document, on the default model, after the first answer about it; survivors show as the
collapsed "N possible glossary terms" card under that answer. Never run for a plain question (FR-KN-GL-7).

### `ask-web`

Prompt-side: short and generic; nothing from the notes is in the message. With web search on (`search` =
`WEB_SEARCH_ON`), the model must search before it answers (the user asked for a search by turning 🌐 on, so
"search when needed" is not enough: a stable question would be answered from memory), cite the pages it
used, and never write web addresses from memory. After a refusal (`search` = `WEB_SEARCH_OFF`, the tool not
sent), the model answers from what it knows, says in its first sentence that the answer is not from a web
search and may be out of date, writes no web addresses, and says so when the question needs current
information it cannot know.

App-side:
- **Was a search made?** Anthropic Messages (Claude API, Foundry Claude): yes when
  `usage.server_tool_use.web_search_requests` > 0 (LiteLLM also mirrors it as
  `prompt_tokens_details.web_search_requests`), or when the reply carries citations or
  `web_search_tool_result` blocks. Responses API (OpenAI, Azure OpenAI): yes when the output has a
  `web_search_call` item. Anything else, and a reply with none of these, counts as no search. The owner's
  run (2026-10-15) showed why this is checked: DeepSeek-V4.1-Flash on Foundry (Chat Completions) ignored
  `web_search_options` silently, with no error, and answered from memory with an outdated fact and a
  Markdown link that looked like a citation.
- **A search was made:** the provider's citations come back with the reply (LiteLLM: `provider_specific_fields.citations`
  and `web_search_results` on the whole reply, a `citation` per streamed chunk) and become link chips that
  open in the browser; the answer is labelled *From the web*.
- **No search was made:** the answer is never shown as a web answer. It is labelled "No web search was made",
  and links in its text are not shown as web sources (they are the model's memory, not pages it read; the
  text itself is shown as written). Then, by the model's request format: on Chat Completions and local
  servers Cairn has no built-in search tool to offer, so the model's *Web search* tick is removed with the
  warning "<model> ignored the web search request" (the tick was wrong, as in a refusal); on Anthropic
  Messages and the Responses API the tool exists and the model simply chose not to use it, so the tick stays
  and the answer is only labelled.
- **A refusal** that names web search (a 400 whose message mentions the tool or the parameter) removes the
  model's *Web search* tick, and the same message is answered again without the tool, rendered with the
  no-search text (`WEB_SEARCH_OFF`). That answer is labelled *No web search was made*, with the warning that
  the tick was removed and the provider's reason; links in its text are not web sources, as above. Other
  errors change nothing (FR-LLM-AS-12).
- **Not client tool calls:** LiteLLM surfaces Claude's server-side search as `tool_calls` with `srvtoolu_`
  ids (and `provider_specific_fields.web_search_calls` in streamed chunks). These are the provider's own tool
  use, already executed; the app must not treat them as a tool call it has to answer, must not fail the reply
  on an "unknown tool", and must not show them as text. Per-search cost on Claude is about 18,000 input
  tokens (the search results are in the context).
- The exact shapes are recorded by the web probes of `poc/i-assistant/llm_check.py` (`web-plain`,
  `web-stream`, and `web-stable`, a question a model would answer from memory, to see that the prompt makes
  it search anyway). `web-off` renders the no-search mode and calls the model without the tool, as the app
  does after a refusal, and judges that the reply says it is not from a web search and contains no URL.

### `ocr`

Prompt-side: exact text, no translation, no picture descriptions, `[unreadable]`, `[page break]` between
pages.

App-side:
- Strip a wrapping code fence.
- `[no text]` becomes an empty page.
- With several images in one call, split on a line that is exactly `[page break]`; when the count does not
  match the number of images, keep the text as one block rather than guessing which page is which.
- A reply cut off by the token limit is retried with fewer pages per call.
