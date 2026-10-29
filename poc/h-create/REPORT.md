# M5 prompt check (`poc/h-create`)

**Question:** do the M5 prompts work on the owner's models before the creation flows are built (D-41, Q1)? The prompts cover:
- reading scanned pages;
- drafting a page from a file;
- the checks: topics, group, tags and glossary suggestions;
- task → knowledge;
- revising a draft in the chat.

**Files:**

| File | What |
|---|---|
| `make_samples.py` | Writes `samples/`, made-up content. It needs reportlab, Pillow and python-docx, from a throwaway environment; the samples are committed. |
| `samples/` | `pump-manual.pdf` (text, with a cover, contents, legal text, headers and footers, and a table flattened into text), `handover-scan.jpg` (a "scanned" sheet with a Chinese line, a calligraphic line standing in for handwriting, and a sketch), `site-visit-scan.pdf` (one text page plus one image-only page), `pre-start-checks.docx` (headings, a numbered list, a table, a picture, a page header), `two-topics.md`, `one-topic.md`, `winter-care-zh.txt`, `task-ops-41.json` |
| `extract.py` | Prototype of the sidecar's extraction (FR-KN-UP-4b): pypdfium2, python-docx and Pillow. Runs locally. |
| `prompts.py` | The prompts under test. |
| `llm_check.py` | Owner-run. It sends the prompts through the real sidecar (`llm.complete`) and judges each reply. Full replies go to `out/<target>/`, which is gitignored. |

**Run:**
```
sidecar/.venv/bin/python poc/h-create/extract.py poc/h-create/samples/*
sidecar/.venv/bin/python poc/h-create/llm_check.py poc/a-llm/config.toml [--only NAME] [--check ID ...]
```

## Extraction (run locally, 2026-10-15)

The sidecar now pins pypdfium2 5.14.0, python-docx 1.2.0, Pillow 12.3.0 and lxml 6.1.3 (`sidecar/requirements.lock`).

| Sample | Result |
|---|---|
| Imports | 64 ms, +22 MB RSS (46 MB in total afterwards) |
| `pump-manual.pdf` | 4 text pages, 121 ms. Headers, footers, contents and legal text come through as text, so the draft has to drop them. The table arrives as flat lines ("Pump stator 1,500 hours replace; then calibrate"), so the draft has to rebuild it. |
| `site-visit-scan.pdf` | Page 1 is text. Page 2 has no text layer, so it is detected as scanned and drawn at 200 dpi as a 1414×2000 grayscale JPEG of 116 KB. Took 37 ms. |
| `pre-start-checks.docx` | Markdown in document order, 17 ms: headings, the numbered list and the table as a Markdown table. The picture and the page header are skipped. |
| `handover-scan.jpg` | Turned upright, scaled to at most 2000 px, re-encoded as JPEG: 103 KB. |
| `.md` / `.txt` | Read as UTF-8, with GB18030 as the fallback for Chinese Windows files. |

**Notes for the contract:**
- `llm.complete` should also return the **finish reason**, so a draft cut off at the token limit is noticed.
- Nothing is cut off with these small samples.

## LLM results, first run (owner, 2026-10-15)

The run covered two models through the real sidecar, with 13 calls each:
- `foundry-v1-chat-key`: DeepSeek-V4.1-Flash, Chat Completions, API key;
- `foundry-claude-entra`: claude-opus-5-5, Anthropic Messages, Entra ID.

| Check | DeepSeek | Claude | Notes |
|---|---|---|---|
| ocr-image, ocr-pdf | PASS (5.9 s, 2.8 s) | PASS (6.6 s, 3.2 s) | Both read the Chinese line and the calligraphic line, and ignored the sketch. |
| draft-full, draft-part | PASS | PASS | Every fact kept; cover, contents, legal text, headers and footers dropped; the flattened table rebuilt; the instruction respected. DeepSeek's titles name the document ("EP-40 Emulsion Pump Service Manual (Rev C)") rather than the subject. |
| draft-docx, draft-zh | PASS | PASS | The table kept. The Chinese note stayed Chinese under an English instruction, tidied into sections. |
| check-two | PASS, but **94.9 s** | PASS | DeepSeek's time looks like a timeout plus a retry (sidecar timeout 90 s, 3 retries). |
| check-one, check-docx | **FAIL**: terms | PASS | DeepSeek suggested generic component names (*swivel hose*, *boom clamp* as "unknown", *test port*) and a product model (*EP-40*). Claude suggested P3W and Kestrel (unknown), AN and gassing solution (medium), and PM (medium, borderline). |
| task | PASS | PASS | DeepSeek invented a small detail ("Hose was twice replaced"). Claude kept the dates that matter. |
| revise-change, revise-ask, revise-add | PASS | PASS | Questions got a short text answer, not a page. The safety note was added from the source. |

Speed: the first text of a streamed draft arrived after 0.9–5.3 s (DeepSeek) and 1.9–4.9 s (Claude). Whole calls took 1.3–8.5 s.

**Follow-ups:**
- **Prompts:** the term rules need to be stricter for weaker models. Also needed: "add nothing", which covers counts and inferences, and a title rule (name the subject, no revision numbers). The templates are being written as files in `crates/cairn-core/prompts/`, which the app will include and this harness will load.
- **App-side filter:** a term with confidence "unknown" that has no capital letter, digit or non-Latin character is dropped. An internal name is written like a name or a code, never as everyday lowercase words.
- **Sidecar (M5 contract):**
  - Report token usage for Anthropic streams. Claude's streamed calls showed "?".
  - Return the finish reason.
  - Look at the 90 s timeout together with the retries, so a stuck call fails sooner and visibly.

## LLM results, second run (owner, 2026-10-15): the templates

A Fable sub-agent wrote the templates in `crates/cairn-core/prompts/`. They cover every M5 call, and `prompts.py` now renders those files. The second run had 17 checks and 19 calls per target, and added these checks:
- `draft-long`: two `draft-part` calls, then `draft-combine`;
- `keep-one`;
- `merge`;
- `groups`.

| | DeepSeek-V4.1-Flash | claude-opus-5-5 |
|---|---|---|
| Result | 15 of 17 PASS | **17 of 17 PASS** |
| Glossary suggestions | Kestrel; AN tank, gassing solution | P3W, Kestrel, PM; AN, gassing solution |
| Failed | `check-docx`: "AN tank" instead of the short form "AN". `merge`: took the new content's title, and repeated facts that were worded differently (the cause twice, the sleeve twice). | – |
| Slow calls | `task` 41.8 s, with text after 41.2 s; all others 1.5–6.4 s | all 3.0–10.6 s |

**What changed since the first run:**
- DeepSeek no longer suggests generic part names (*swivel hose*, *boom clamp*, *test port*) or product models (*EP-40*).
- The invented count is gone.
- The new templates pass on both models, apart from DeepSeek's merge.

**Conclusions for M5:**
- The prompts are good enough to build on. The two DeepSeek misses are handled in the app, not by more prompt rules:
  - *Suggest merge* keeps the existing page's title. Its result is edited before saving anyway (FR-KN-DU-5).
  - "AN tank" is acceptable to show; the user can edit or ignore it.
- DeepSeek on Foundry sometimes takes 40–95 s. The UI needs to show the model is still working, with *Stop*, and the sidecar timeout needs revisiting (see above).
- Claude's streamed calls still report no token counts, as noted above.

## Similarity thresholds (run locally, 2026-10-15)

`similarity_check.py` ran the real search model (EmbeddingGemma q4, 256 dims, document prompts). It scored made-up page and block pairs the way the app does.

| Pair | Scores | Threshold |
|---|---|---|
| **Pages**, same topic reworded (incl. Chinese; a note vs a task page about the same repair) | 0.84–0.95 | "likely the same topic" = **0.78**, the default of `duplicateScore` (FR-KN-DU-1) |
| Pages, same area but another topic (pump calibration vs pump maintenance) | 0.63–0.68 | |
| Pages, unrelated | 0.31–0.37 | |
| **Blocks**, identical or a tiny wording edit | 0.997–1.0 | *Ignore* at **0.99** (`merging::IGNORE_AT`) |
| Blocks, the same fact with a **new number**; reworded; an updated step | 0.875–0.977 | *Replace* at **0.70** (`REPLACE_AT`) |
| Blocks, other facts or unrelated | 0.41–0.48 | *Add* |

A changed number still scores 0.977. So a block whose numbers differ from its target is never *Ignore*: it becomes *Replace*.
