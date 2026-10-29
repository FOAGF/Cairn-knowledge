# M0-a: does LiteLLM cover the Must provider requirements?

## Question
Can LiteLLM alone cover FR-LLM-PV-1/1a/2/3, FR-LLM-EF-1..3, streaming, image input and NFR-NET-2, and what does it cost in RAM and import time?
**Short answer: yes, functionally, with 6 concrete gotchas (below) that the sidecar must handle. Cost: about 1.8 s import and about 190 MB RSS (up to about 255 MB after every provider path has been exercised).** Everything was verified against a localhost mock only; real-service behaviour is for `live_check.py`.

## Versions (see requirements.txt)
Python 3.12.3, litellm 1.105.0, openai 2.54.0, azure-identity 1.26.0, msal 1.39.0, httpx 0.28.1, pydantic 2.14.1, tenacity (needed, see gaps), pytest 9.1.1.
Note: the `anthropic` SDK is NOT needed; LiteLLM talks to Anthropic via httpx.

## What was run
`mock_server.py` (stdlib; records path/query/headers/body; OpenAI chat, Responses, Anthropic Messages, each normal + SSE; 401/403; "429, 429, 200").
`test_providers.py`: 137 tests, all pass (`.venv/bin/python -m pytest -q`). Matrix = 11 provider/format/auth combos x {effort unset, low, medium, high} x {stream, no stream}, plus image input x {stream, no stream}, plus auth/retry/risk tests. Entra uses a fake token provider only.

## Results: which LiteLLM call to use

| Requirement | LiteLLM call | Path / auth seen by mock | Result |
|---|---|---|---|
| Azure classic, Chat | `litellm.completion(model="azure/<deployment>", api_base="https://res.openai.azure.com", api_version=..., api_key=...)` | `/openai/deployments/<dep>/chat/completions?api-version=...`, `api-key:` | PASS |
| Azure classic, Responses | `litellm.responses(model="azure/responses/<dep>", api_base, api_version, api_key)` (plain `azure/<dep>` also works) | `/openai/responses?api-version=...`, `api-key:` | PASS |
| Foundry v1, Chat | `litellm.completion(model="openai/<dep>", api_base="https://res.openai.azure.com/openai/v1", api_key=...)` | `/openai/v1/chat/completions`, `Authorization: Bearer` | PASS |
| Foundry v1, Responses | `litellm.responses(model="openai/<dep>", api_base=".../openai/v1", api_key=...)` | `/openai/v1/responses`, Bearer | PASS |
| Claude API | `litellm.completion(model="anthropic/<id>", api_key=...)` | `/v1/messages`, `x-api-key:` | PASS |
| Any OpenAI-format (Ollama/LM Studio) | `litellm.completion(model="openai/<name>", api_base="http://host:port/v1", api_key=...)`; Responses also works if server supports it | `/v1/chat/completions` (`/v1/responses`), Bearer | PASS |
| Wrong prefixes (do not use) | `azure/` + `/openai/v1` base: path doubles (`/openai/v1//openai/v1/chat/completions`) and auth is Bearer; `azure_ai/` + `litellm.responses`: silently routed to `/chat/completions` | | AVOID |

Every cell below passed for **all 11 combos** unless noted: basic text, stream (native `stream:true` in body, deltas assembled), image input (chat: `image_url` data-URI block; Responses: `input_image`; Claude: `image` block with `source.type=base64`, `media_type`), effort unset, effort low/medium/high.

### Exact effort parameter sent (FR-LLM-EF-1..3)
| Format | effort unset | effort = low / medium / high |
|---|---|---|
| Chat Completions (azure/, openai/) | none of `reasoning_effort`, `reasoning`, `thinking`, `output_config` in body | `"reasoning_effort": "<level>"` (pass-through) |
| Responses API (azure/responses/, openai/) | no `reasoning` key | `"reasoning": {"effort": "<level>"}`; pass via `reasoning={"effort": ...}` (litellm.responses) |
| Claude, older models (sonnet-4-5, opus-4-1, haiku-4-5) | no `thinking` | `"thinking": {"type":"enabled","budget_tokens": 1024 / 2048 / 4096}` and `max_tokens` raised to 5120 / 6144 / 8192 (LiteLLM adds budget + 4096 if caller sets no max_tokens) |
| Claude, newer models (sonnet-4-6, opus-4-7, also unknown names) | no `thinking` | `"thinking": {"type":"adaptive","display":"summarized"}` plus `"output_config": {"effort": "<level>"}`; `max_tokens` 128000 (opus-4-7/sonnet-4-6) or 4096 (unknown name) |
| claude-3-5-haiku | | `UnsupportedParamsError` (model cannot think; correct behaviour) |

LiteLLM picks adaptive-vs-budget by model name from its bundled model table, so a new Claude release needs a LiteLLM upgrade to get the right form.

### Auth (FR-LLM-PV-1a)
- API key: Azure classic sends `api-key`; Foundry v1 / OpenAI-compat send `Authorization: Bearer`; Claude sends `x-api-key`.
- Entra, Azure classic style (chat and responses): `azure_ad_token_provider=<callable>` works, header is `Authorization: Bearer <token>` and **the provider is invoked on every request** (3 calls -> 3 tokens), so `get_bearer_token_provider(AzureCliCredential(), "https://cognitiveservices.azure.com/.default")` (which caches and refreshes) gives automatic refresh. Tested with a fake provider only.
- Entra, Foundry v1 style: **LiteLLM's `openai/` provider does not accept a callable `api_key`** (chat: `'function' object has no attribute 'strip'`; responses: pydantic validation error) and has no token-provider hook. Workaround that passes tests: call the token provider ourselves right before each request and pass the string as `api_key` (the sidecar is stateless per request, so this is natural; the provider caches internally). A static token in `extra_headers` also works but would not refresh.
- Real `AzureCliCredential` not exercised here; `live_check.py` does it.

### Errors and retries (NFR-NET-2)
- 401: `litellm.AuthenticationError` with `status_code=401` and the server message, for all of azure chat/responses, openai chat/responses, Claude. Not retried (1 request).
- 403: Claude -> `PermissionDeniedError`; Azure and OpenAI paths -> generic `litellm.APIError` with `status_code=403` and the message. Check `status_code`, not the class.
- 429: with `num_retries=3` all five call types retried twice and succeeded (429, 429, 200). After exhaustion raises `litellm.RateLimitError`. Observed back-off: about 0.6-1.3 s for the OpenAI/Azure chat path; with a mock `retry-after: 0` the Claude/Responses path retried immediately (it honours Retry-After). Real exponential timing not measured. Note chat via the OpenAI SDK retries 429 twice by default even without `num_retries`.

## Cost
| Measure | Value (3 runs, fresh process, nice 10) |
|---|---|
| `import litellm` wall time | 1.6-1.9 s (about 2.4 s observed once on a busy machine) |
| RSS before / after import | 10 MB -> 190-193 MB (about +180 MB), 2,624-2,686 modules loaded |
| After first OpenAI-style call | about 210 MB; first call 0.2 s, later calls 0.04 s |
| After also calling Anthropic, Azure and Responses paths | about 255 MB |
| + `import azure.identity` | +0.2 MB / 0.04 s (cheap) |
| venv site-packages | 355 MB (litellm 165 MB, botocore 31, openai 20, cryptography 16, tokenizers 12, hf_xet 11) |

Lazy import: `import litellm` has no provider-level laziness to exploit (it eagerly imports openai, pydantic types, tokenizers, botocore, etc.). The only lever is deferring the whole import to the first LLM request (or importing in a background thread at sidecar start): that moves 1.8 s and about 180 MB out of app start-up and out of sessions that never call an LLM, but does not reduce the figures once used. `azure.identity` is cheap enough to import eagerly or lazily.

## Gaps and risks (what the sidecar must handle)
1. **Network at import (privacy / FR-LLM-LIB-1 relevant).** By default `import litellm` fetches `model_prices_and_context_window.json` and `anthropic_beta_headers_config.json` from raw.githubusercontent.com (source: `litellm/__init__.py`, `get_model_cost_map.py`). **Set `LITELLM_LOCAL_MODEL_COST_MAP=True` before import** so the sidecar makes no outbound call except to the user's configured endpoints. I did not check for any other outbound traffic (telemetry callbacks are off by default; not audited exhaustively). Please have the owner confirm with the live run under a firewall or `strace`/proxy.
2. **Responses streaming is silently faked for unknown models.** If the model is not in LiteLLM's cost map (custom Azure deployment names, Ollama models, anything new), `litellm.responses(stream=True)` sends a NON-streaming request and replays events locally (`MockResponsesAPIStreamingIterator`, no `stream` in body). Verified. Fix: before the call `litellm.register_model({"<provider>/<name>": {"litellm_provider": "<provider>", "supports_native_streaming": True, "supports_reasoning": True, "supports_vision": True}})` (done in tests and `live_check.py`). Do NOT put `"mode": "responses"` in it: that reroutes chat calls for the same name into the Responses bridge. Chat streaming is native for unknown models.
3. **Azure deployment names are arbitrary, capabilities are looked up by model name.** `reasoning_effort` on `azure/my-custom-deployment` raises `UnsupportedParamsError`; works for names such as `gpt-5`, `o3-mini`. Fix: pass `allowed_openai_params=["reasoning_effort"]` when effort is set (works), or `base_model`. Responses `reasoning.effort` on registered models also needs `supports_reasoning: True`. Net effect: unset effort is always safe; set effort needs the override.
4. **Entra on Foundry v1** needs the per-call token workaround above (no callable `api_key`).
5. **`tenacity` is not installed by `pip install litellm`.** `num_retries` on the Anthropic, Responses and Azure-responses paths raises "tenacity import failed" without it. It is pinned in requirements.txt; the sidecar bundle must include it. FR-LLM-LIB-1: this is a missing-dependency risk, not a licence/design violation.
6. **Footprint**: about 255 MB RSS and 355 MB on disk for the sidecar is large for a "personal desktop app" sidecar; flagged for the owner's decision. Nothing here conflicts with a stateless sidecar design. A thin custom client (httpx; the formats are simple) would be far smaller (not measured) but would mean owning the effort-mapping table and Claude model-generation logic that LiteLLM provides.
7. Claude effort mapping changes `max_tokens` implicitly (budget + 4096, or 128000 on 4.6/4.7). Set `max_tokens` explicitly if the app needs a cap. Unknown future Claude names default to the adaptive/`output_config.effort` form.
8. Not verifiable on a mock: real response shapes (reasoning items in Responses output, Claude `thinking` blocks in streams), real Entra token acquisition/expiry, real 429 timing, whether Azure accepts `/openai/v1/` with Entra bearer tokens, model-specific image limits.

## Recommendation
Adopt LiteLLM for the sidecar provided that it (a) sets `LITELLM_LOCAL_MODEL_COST_MAP=True` before import, (b) registers each configured model for native streaming (and reasoning/vision) at call time, (c) passes `allowed_openai_params=["reasoning_effort"]` for Azure chat when effort is set, (d) uses `openai/` + `/openai/v1` for Foundry v1 (not `azure/` or `azure_ai/`) and fetches the Entra token per request, (e) bundles tenacity and sets `num_retries`, (f) defers `import litellm` until the first LLM request. Then run `live_check.py` once with real endpoints before relying on it.

## How the owner runs live_check.py
```
cd poc/a-llm
cp config.example.toml config.toml        # edit endpoints, deployment names, models; no secrets in the file
export AZURE_OPENAI_API_KEY=...           # or just wait for the hidden prompt; same for ANTHROPIC_API_KEY
az login                                  # only for auth = "entra" targets (you run this yourself)
.venv/bin/python live_check.py config.toml            # all targets
.venv/bin/python live_check.py config.toml --only claude --verbose
```
Prints a PASS/FAIL/skip table per target x {basic, stream, effort_unset, effort_low/medium/high, image, bad_key}. `stream` fails if only one text chunk arrives (likely not real streaming). `image` sends a 64x64 solid red PNG and expects "red". `bad_key` sends a bogus key and expects HTTP 401/403 (disable with `--no-auth-test`). Exit code 1 if any FAIL. The script was smoke-tested against the local mock (only `image` fails there, as expected, since the mock answers "Hello from mock"). Keep config.toml out of git if it contains your resource names.

## Files
`requirements.txt`, `mock_server.py`, `conftest.py`, `test_providers.py`, `measure.py`, `live_check.py`, `config.example.toml`, `REPORT.md` (all under `poc/a-llm/`; `.venv/` is local).

## Live check (2026-10-14, the owner's Foundry endpoint)

Target: `DeepSeek-V4.1-Flash` on a Foundry `services.ai.azure.com/openai/v1` endpoint, Entra ID through `az login`.

| Format | basic | stream | effort unset | effort low/med/high | image |
|---|---|---|---|---|---|
| Chat Completions | PASS | PASS (6 chunks, first after 0.33 s) | PASS | PASS | PASS |
| Responses API | PASS | 6 chunks, but the first came after 0.00 s | PASS | FAIL: `reasoning.effort` not supported by this model | FAIL: model doesn't accept images in this format |

- **Entra ID on Foundry `/openai/v1` works.** This was risk 6, which the mock couldn't test.
- **Bug fixed in `live_check.py`:** the Entra path passed the token function instead of the token (`self.provider()` → `self.provider()()`). That caused the confusing first error. The script now also prints a plain-language hint next to each failure.
- **Effort and image support can differ per format for the same model.** Per-model settings (FR-LLM-EF-3) cover this, because the format is also chosen per model.
- **Check in M4:** Responses streaming here may be LiteLLM's faked streaming (the first chunk arrived instantly), even though the model was registered.
