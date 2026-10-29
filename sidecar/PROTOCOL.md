# Sidecar protocol (M4, extended in M5 and M6)

The app (Rust, `src-tauri/src/sidecar.rs`) talks to the Python sidecar (`sidecar/cairn_sidecar/`) over **stdio** only (NFR-SEC-5). Two processes run from the same package (TC-12):

| Role | Started as | Does |
|---|---|---|
| `ai` | `python -I -m cairn_sidecar --role ai` | LLM calls through LiteLLM, Azure CLI sign-in check, file extraction (M5) |
| `embed` | `python -I -m cairn_sidecar --role embed` | EmbeddingGemma on ONNX Runtime (FR-RAG-EM-1). Runs at low priority (`os.nice(10)`, FR-RAG-IX-5) |

## Process rules
- **Format:** newline-delimited JSON-RPC 2.0, UTF-8, one JSON object per line.
  - **stdout** carries protocol messages only, flushed after every message.
  - **stderr** is free-form log text. The app copies it into its own log at debug level.
  - Logs never contain API keys or page/task content (NFR-SEC-3).
- **Lifetime:**
  - The process exits with code 0 when **stdin closes**. The app closes stdin after the idle timeout (FR-RAG-EM-1a), so the sidecar keeps no timer of its own.
  - On `shutdown` it answers, then exits.
- **Concurrency:**
  - Requests are handled concurrently: a reader thread dispatches each request.
  - **ai:** each request runs in its own worker thread, so a long chat stream doesn't block a connection test.
  - **embed:** one worker thread with a **priority queue**, so `embed.query` always goes before waiting `embed.documents` batches. The app sends documents in small batches (at most 8 pieces), so a search waits for at most one batch.
- **Lazy imports:** LiteLLM, azure-identity, onnxruntime, tokenizers and numpy are imported on first use, not at start (TC-12). `ping` must answer within ~50 ms of start.
- **Environment:** the app starts the sidecar with these settings:
  - `LITELLM_LOCAL_MODEL_COST_MAP=True` and `LITELLM_LOG=ERROR` (M0-a workaround 1), and `LITELLM_MODE=PRODUCTION`: no `.env` lookup at import (M8; the lookup fails without source files).
  - `LITELLM_RUST=false`: the bundle leaves out LiteLLM's Rust extension, so every route runs in Python (D-82; the build checks LiteLLM's route table).
  - `PYTHONUNBUFFERED=1`.
  - `AZURE_OPENAI_API_KEY`, `AZURE_API_KEY` and `AZURE_AI_API_KEY` removed (M0-a: LiteLLM would prefer them over Entra ID).

  The sidecar sets these defaults itself too, before importing LiteLLM.
  It keeps `PATH`, so that `az` is found, and `HOME`, and any `HTTPS_PROXY`/CA variables (NFR-NET-1).
- **Secrets:** API keys arrive inside each request (`target.api_key`) and are never written anywhere or logged (NFR-SEC-1).

## Messages
```
request       {"jsonrpc":"2.0","id":7,"method":"llm.complete","params":{...}}
result        {"jsonrpc":"2.0","id":7,"result":{...}}
error         {"jsonrpc":"2.0","id":7,"error":{"code":-32000,"message":"...","data":{"kind":"auth","status":401,"hint":"..."}}}
stream chunk  {"jsonrpc":"2.0","method":"stream","params":{"id":7,"delta":"partial text"}}      (notification, no "id" key)
cancel        {"jsonrpc":"2.0","method":"cancel","params":{"id":7}}                             (app -> sidecar notification; stop a stream)
```

**Error kinds** (`error.data.kind`):

| Kind | Meaning |
|---|---|
| `auth` | 401: key or token rejected |
| `forbidden` | 403: signed in, but not allowed |
| `not_found` | 404: endpoint, deployment or model |
| `rate_limit` | 429 after LiteLLM's retries |
| `network` | the endpoint could not be reached, or timed out |
| `bad_request` | 400: the model refused a parameter |
| `unsupported` | the provider can't do this (e.g. list Azure deployments) |
| `signin` | the Azure CLI isn't signed in: run `az login` |
| `model_missing` | the embedding model files were not found |
| `too_long` | the input is longer than the model accepts (LiteLLM `ContextWindowExceededError`, or a 400 that says so). Rust then sends less (FR-KN-UP-4a) |
| `bad_file` | `file.extract` could not read the file: damaged, password-protected, or not what its extension says |
| `web_search_unsupported` | (M6) the provider refused the request **because of web search**: a 400 that names the web search tool or option, an unknown tool type, or "web search is not enabled" for the organisation. Rust removes the model's *Web search* tick and answers without the web (FR-LLM-AS-12). Any other failure keeps its usual kind |
| `internal` | anything else |

- `message` is plain language for the user.
- `hint` is optional: a short next step, using the hints from `poc/a-llm/live_check.py`.
- JSON-RPC parse and method errors use the standard codes (-32700, -32601, -32602).

## Common methods
| Method | Params | Result |
|---|---|---|
| `ping` | `{}` | `{"pong":true,"role":"ai","pid":123,"python":"3.12.15","version":"0.1.0"}` |
| `shutdown` | `{}` | `{}` (then exits) |

## `ai` methods

### The `target` object
Rust builds it from a provider and a model (`cairn_core::providers::call_target`). Field names are snake_case:

```json
{
  "provider_kind": "azure_foundry",
  "endpoint": "https://res.services.ai.azure.com/openai/v1",
  "api_version": null,
  "auth": "azure_cli",
  "api_key": null,
  "model": "my-deployment",
  "format": "chat_completions",
  "vision": true,
  "web_search": false,
  "effort": {"enabled": true, "levels": ["low","medium","high"], "default_level": null, "custom": null}
}
```

**Fields:**

| Field | Values |
|---|---|
| `provider_kind` | `azure_foundry` · `anthropic` · `openai_compatible` · `local` (`local` behaves as `openai_compatible`; Ollama / LM Studio) · `deepseek` (D-69: DeepSeek's own API, Chat Completions; an empty endpoint is `https://api.deepseek.com`) |
| `auth` | `api_key` · `azure_cli` · `none` |
| `format` | `chat_completions` · `responses` · `anthropic_messages` |
| `effort.levels` | always filled (Rust applies the default `low/medium/high`) |
| `effort.custom` | `null` = Auto mapping (FR-LLM-EF-2), or `{level: value}` where the value is a string. A whole number means a Claude thinking budget in tokens (FR-LLM-EF-4) |

**Choosing the LiteLLM route.** The sidecar picks it from these fields, **never from the model name** (FR-LLM-PV-7):

| Case | Route |
|---|---|
| `azure_foundry` + `anthropic_messages` | `azure_ai/<model>` with `api_base` = the endpoint (`…/anthropic`). LiteLLM needs "claude" in the name to route there; work around it if the deployment name lacks it (e.g. pass `custom_llm_provider`, or register the model) |
| `azure_foundry`, endpoint containing `/openai/v1` | `openai/<model>` with `api_base` (Chat), or the Responses API through the same prefix |
| other `azure_foundry` | `azure/<model>` (Chat) or `azure/responses/<model>` (Responses), with `api_version` (default `2025-04-01-preview` if unset) |
| `anthropic` | `anthropic/<model>` (optional `api_base`) |
| `openai_compatible` / `local` | `openai/<model>` with `api_base`. A missing key is sent as a placeholder (`none` auth) |
| `deepseek` | `openai/<model>` with `api_base` = the endpoint, or `https://api.deepseek.com` when empty |

**Entra ID** (`auth: azure_cli`), following M0-a:
- `AzureCliCredential` + `get_bearer_token_provider`, scope `https://cognitiveservices.azure.com/.default`, cached for the process lifetime.
- The `azure/` and `azure_ai/` routes take `azure_ad_token_provider`.
- The `openai/` route gets a fresh token as `api_key` before each call (workaround 4).

Every call also applies the other M0-a workarounds:
- `register_model` for each model before use, so Responses streaming is real (workaround 2).
- `allowed_openai_params=["reasoning_effort"]` for Azure Chat (workaround 3).
- `num_retries=3` for rate limits and connection errors. **A timeout is retried at most once** (M5: a stuck call must fail visibly instead of waiting 4 × the timeout). The timeout is `timeout_s` from the request, default 90 s.

### `llm.complete`
Params:
```json
{"target": {...}, "messages": [{"role":"system|user|assistant","content":"text" | [{"type":"text","text":"..."},{"type":"image","data_url":"data:image/png;base64,..."}]}],
 "stream": false, "effort": "medium" | null, "max_tokens": null, "timeout_s": null,
 "web_search": null | {"max_uses": 5}}
```
- **Images:** an image block is `{"type":"image","data_url":"data:…"}` or (M5) `{"type":"image","path":"/abs/page.jpg"}`. With `path`, the sidecar reads the file and builds the data URL itself (the MIME type from the extension), so page images from `file.extract` never travel through stdio twice. An unreadable file gives the error kind `bad_file`; an extension other than `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp` is an invalid-params error (-32602).
- **`timeout_s`:** seconds per attempt; `null` = 90. Rust passes about 60 for checks, 120 for OCR and 300 for streamed drafts.
- **`effort`:** `null` means: use `target.effort.default_level` if set, otherwise send nothing (FR-LLM-EF-1). A level is sent only when `target.effort.enabled` (FR-LLM-EF-3), and only by the mapping for the format (FR-LLM-EF-2) or by `custom`.
- **Streaming:** with `"stream": true`, each text delta is sent as a `stream` notification.
- **`web_search`** (M6, FR-LLM-AS-12): turn on the provider's built-in web search, with at most `max_uses` searches. By `target.format`:
  - `anthropic_messages`: Anthropic's server tool `{"type": "web_search_20250305", "name": "web_search", "max_uses": N}` (the only version Foundry deployments hosted on Azure accept). Pass the tool itself rather than LiteLLM's `web_search_options`, which only offers 1, 5 or 10 searches. (Checked in LiteLLM 1.105: `AnthropicConfig._map_tool_helper` forwards a hosted-tool dict whose `type` starts with `web_search` unchanged, `max_uses` included, on both the `anthropic/` and `azure_ai/` Claude routes, so no fallback is needed.)
  - `responses`: the tool `{"type": "web_search"}`.
  - `chat_completions`: LiteLLM's `web_search_options` (`{"search_context_size": "medium"}`); OpenAI's search models answer with `url_citation` annotations. Other models usually ignore it without an error (the owner's run, 2026-10-15: DeepSeek on Foundry), which the result shows as `searches: 0`.
  - **`deepseek` (D-69): Cairn runs the search.** DeepSeek's Chat Completions API has no built-in search, so `deepseek.py` gives the model two function tools, `web_search(query)` and `web_fetch(url)`, and answers each call through DeepSeek's Search API (`POST /v1/tools/search`, `count` 8, `ori_question` = the user's last message) or Fetch API (`POST /v1/tools/fetch`, content cut to 12,000 characters), with the same key; a trailing `/v1` of the endpoint is dropped. The first round has `tool_choice: "required"` (`auto` if the server refuses that with a 400); at most `max_uses` calls, then `tool_choice: "none"`. The assistant message with the tool calls goes back with its `reasoning_content`. A 401 or 402 (balance used up) stops the call; any other failure goes back to the model as the tool result. Text from every round is streamed and joined. `searches` counts the searches and page reads that returned something; `sources` are the pages given to the model whose URL the answer contains, or the first 5 when it contains none. The tool texts were written by a Fable sub-agent.
  - **Server-side search is not a client tool call.** LiteLLM surfaces Claude's searches as `tool_calls` with `srvtoolu_` ids and, when streaming, `provider_specific_fields.web_search_calls`. They are never streamed as text and never fail the call as an unknown tool.
- **Result:** `{"text": "full text", "model": "...", "usage": {"input_tokens": 12, "output_tokens": 34, "reasoning_tokens": 20} | null, "finish_reason": "stop" | "length" | "cancelled" | null, "web": null | {"searches": 1, "sources": [{"url": "https://…", "title": "…", "cited_text": "…"}]}}`.
  - `web` (M6) is present whenever `web_search` was asked for. `searches` is how many searches ran: Anthropic `usage.server_tool_use.web_search_requests` (LiteLLM also mirrors it as `prompt_tokens_details.web_search_requests`), Responses `web_search_call` output items, Chat Completions 1 when there are `url_citation` annotations, else 0. `sources` are the cited pages, de-duplicated by URL in first-cited order: Anthropic citations (`provider_specific_fields.citations`, per chunk `citation` when streaming) with their `cited_text`, Responses / Chat `url_citation` annotations (`cited_text` empty). `searches: 0` with no sources means no search was made.
  - A `web_search` value that is not null or an object with a positive integer `max_uses` (default 5 when missing) is an invalid-params error (-32602). A `cancel` answer carries `web` too, with what was seen so far (usually `searches: 0` and no sources).
  - A refusal (error kind `web_search_unsupported`) keeps the provider's reason in the message (LiteLLM's class prefix is removed) and is never retried.
  - `finish_reason` (M5): `length` means the reply was cut off at the token limit, so Rust can mark a draft as incomplete; `cancelled` means a `cancel` stopped the stream (`text` is the part received); `null` when the provider didn't say.
  - `usage` is reported for streamed calls on every route, Anthropic included (M5; ask LiteLLM for usage in the stream).
  - `usage.reasoning_tokens` (D-65) is there only when the provider reports a count above zero (LiteLLM fills in 0 on some routes when nothing was reported): LiteLLM's `completion_tokens_details.reasoning_tokens` (Chat Completions; Claude's thinking) or `output_tokens_details.reasoning_tokens` (Responses API). It is part of `output_tokens`.

### `llm.test`
Used by *Test connection* (UI-SET-2, FR-LLM-EF-4).

Params: `{"target": {...}}`. Result: `{"checks": [{"name": "basic", "ok": true, "detail": "replied in 0.8 s"}, ...]}`.

Checks, in order. `ok: null` means skipped, with the reason in `detail`:
1. **`basic`:** "Reply with exactly: pong".
2. **`stream`:** counts the text chunks. A single chunk means the stream is probably not real; report it in `detail`.
3. **`effort:<level>`:** one entry per level in `target.effort.levels`. Skipped when effort is disabled.
4. **`image`:** a 64×64 red PNG, "What colour is this image?". It passes if the reply contains "red". Skipped unless `vision`.
5. **`web`** (M6): one small search with `web_search: {"max_uses": 1}` ("Search the web: what is today's date?"). `ok: true` when `web.searches > 0`; `ok: false` with "no search was made" or the refusal's message. Skipped ("skipped: web search is off for this model") unless `target.web_search`; also skipped when `basic` fails fatally. `ok: false` with "no search was made: the provider ignores web search" or the refusal's message. The UI then offers to correct the model's *Web search* tick.

A check that fails puts the error message and hint in `detail` and doesn't stop the later checks. A failure that makes everything else pointless (`auth`, `signin`, `network`, `not_found` on `basic`) skips the remaining checks with "skipped: basic call failed".

### `llm.list_models`
Used by *Fetch models* (UI-SET-2).

- Params: `{"target": {...}}` (`target.model` may be empty).
- Result: `{"models": [{"id": "claude-sonnet-4-5", "label": "Claude Sonnet 4.5" | null}]}`.
- Supported for `anthropic` (Models API), for `openai_compatible` and `local` (`GET /v1/models`), and for `deepseek` (`GET /models`, DeepSeek's documented path).
- `azure_foundry` returns the error kind `unsupported`.

### `llm.model_info` (M5)
How much the model accepts, from LiteLLM's local model map (no network call). Used when a model has no *Context size* set (FR-KN-UP-4a).
- Params: `{"target": {...}}`.
- Result: `{"max_input_tokens": 200000 | null, "max_output_tokens": 64000 | null}`. `null` when LiteLLM doesn't know the model (typical for Azure deployment names).

### `file.extract` (M5)
Reads an uploaded file locally (FR-KN-UP-1, FR-KN-UP-4, FR-KN-UP-4b; prototype: `poc/h-create/extract.py`). No LLM call: pages that need the OCR model come back as image files, and Rust sends them to `llm.complete`.

Params: `{"path": "/abs/data/tmp/uploads/<id>.pdf", "out_dir": "/abs/data/tmp/uploads/<id>/", "max_edge": 2000, "dpi": 200}`. `max_edge` and `dpi` are optional (these defaults).

Result:
```json
{"kind": "pdf", "pages": 2,
 "parts": [{"page": 1, "text": "Site visit notes …"},
           {"page": 2, "image_path": "/abs/data/tmp/uploads/<id>/p0002.jpg", "width": 1414, "height": 2000}]}
```
- `kind`: `text` (`.md`, `.txt`), `pdf`, `docx` or `image` (`.png`, `.jpg`, `.jpeg`), from the extension.
- **`.md` / `.txt`:** one part with the text, read as UTF-8 (a BOM is dropped), else GB18030, else Latin-1.
- **PDF (pypdfium2):** one part per page, in order. A page with fewer than 25 non-space characters in its text layer counts as **scanned**: it is drawn at `dpi` in grayscale and written to `out_dir` as a JPEG (quality 85) scaled to fit `max_edge`. Other pages give their text (line ends `\n`). A password-protected or damaged PDF fails with `bad_file`.
- **DOCX (python-docx):** one part with the body as Markdown, in document order: `Title`/`Heading N` styles become `#` headings, `List Number` items `1.`, other `List` styles `-`, tables Markdown tables (`|` escaped, line breaks in cells become spaces). Pictures, headers, footers and text boxes are skipped.
- **Images (Pillow):** one part with an `image_path`: turned upright from EXIF, converted to RGB or grayscale, scaled to fit `max_edge`, written as JPEG to `out_dir`.
- A missing file or an extension other than the ones above is not a `file.extract` success: a missing or damaged file gives `bad_file`; an unsupported extension is an invalid-params error (-32602).
- `out_dir` is created if needed. Rust deletes the upload and `out_dir` after the OCR step (FR-KN-UP-6).

### `azure.check_signin`
- Params: `{}`.
- Result: `{"signed_in": true, "account": "name@example.com" | null, "detail": "token valid until 14:05"}`.
- It gets a token with `AzureCliCredential`. If that fails, the result is `{"signed_in": false, "detail": "Run `az login` …"}`, as a result rather than an error.

### `model.download` (D-82)
- Params: `{"dest_dir": "/…/data/models/.download", "files": [{"url": "https://huggingface.co/<repo>/resolve/<rev>/onnx/model_q4.onnx", "path": "onnx/model_q4.onnx", "size": 519322, "sha256": "…"}, …]}`. The app takes the files from `packaging/model-pins.sh`. `path` is relative, with `/` and no `..`.
- Downloads each file to `dest_dir/<path>`, through `<path>.part`. **Resumes:** a finished file is skipped; a part on disk is hashed first and the rest asked for with a range request (a `200` answer instead of `206` starts that file again).
- **Progress:** `stream` notifications whose `delta` is JSON text, `{"done": <bytes>, "total": <bytes>}`, at most about four a second.
- Result: `{"done": total, "total": total}`, or `{"cancelled": true}` after `cancel` (files kept).
- Errors: `network` (unreachable, timeouts, an HTTP status other than 200/206, a file that ends early; finished files are kept) and `model_check_failed` (a file longer than its size or with another hash; it is deleted). Bad params are an invalid-params error (-32602). The app checks the files again when it puts them in place (FR-RAG-EM-3).

## `embed` methods
`model_dir` holds `onnx/model_q4.onnx` (and its `.onnx_data` file) and `tokenizer.json` (M0-b). The session is loaded on first use and kept for the life of the process: unloading means exiting (TC-12).

Vectors are **base64 of little-endian float32**, truncated to `dims` and re-normalised (FR-RAG-EM-1). This is the same layout the Rust core stores as a BLOB.

Prompts (M0-b, the model card):
- Documents: `title: {title or "none"} | text: {text}`.
- Queries: `task: search result | query: {text}`.

Tokens are truncated at 2048.

| Method | Params | Result |
|---|---|---|
| `embed.query` | `{"model_dir": "...", "dims": 256, "threads": 4, "text": "..."}` | `{"vector": "<base64>"}` |
| `embed.documents` | `{"model_dir": "...", "dims": 256, "threads": 4, "items": [{"title": "...", "text": "..."}]}` | `{"vectors": ["<base64>", ...]}` (same order) |
| `embed.info` | `{"model_dir": "...", "dims": 256, "threads": 4}` | `{"loaded": true, "model": "embeddinggemma-300m-q4", "dims": 256, "load_ms": 950}` (loads the model if needed) |

- `threads` sets ONNX Runtime's `intra_op_num_threads`. The app passes `min(4, max(1, cores / 2))` (FR-RAG-IX-5). `inter_op_num_threads` is 1.
- Missing model files give the error kind `model_missing`, with the path in `message`.
