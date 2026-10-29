# Cairn sidecar

The Python side of Cairn: LLM calls through LiteLLM (`--role ai`) and embeddings with
EmbeddingGemma on ONNX Runtime (`--role embed`). The app starts it as
`python -I -m cairn_sidecar --role ai|embed` and talks JSON-RPC over stdio.
The contract is [`PROTOCOL.md`](PROTOCOL.md).

## Layout
- `cairn_sidecar/__main__.py` starts a role.
- `rpc.py` reads requests, dispatches (thread per request, or a priority queue for `embed`), writes answers, handles `cancel`.
- `ai.py` picks the LiteLLM route from the target settings and implements `llm.*` (including `llm.model_info`) and `azure.check_signin`.
  It does the retries itself (LiteLLM gets `num_retries=0`): up to 3 for rate limits and connection errors, at most 1 for a timeout, none once a stream has sent text.
- `extract.py` implements `file.extract` (text, PDF, DOCX, images). pypdfium2, python-docx and Pillow are imported on first use.
- `embed.py` loads the ONNX model once and implements `embed.*`.
- `errors.py` maps library errors to the protocol's error kinds, with plain-language messages and hints.
- `requirements.lock` has exact pins (installed with `--no-deps`); `requirements-dev.txt` adds pytest.
- `tests/` has pytest tests and a local mock of the OpenAI Chat, OpenAI Responses and Anthropic APIs.
- `tools/live_check.py` is the owner-run check against real endpoints.

## Development setup
```
nice -n 10 scripts/setup-sidecar-dev.sh
```
It creates `sidecar/.venv`, installs the pins and the package (editable), and links the embedding model into
`sidecar/models/embeddinggemma-300m/` from `$CAIRN_EMBED_MODEL_SRC` (see `scripts/setup-sidecar-dev.sh`).
It says clearly if model files are missing; the embedding tests are then skipped.

## Tests
```
nice -n 10 sidecar/.venv/bin/python -m pytest sidecar/tests -q
```
Nothing here touches the internet: the tests talk to a mock on 127.0.0.1, and one test checks
that no other host is contacted. The first LLM call in a process takes a few seconds (importing LiteLLM).

## Live check (you run it)
```
sidecar/.venv/bin/python sidecar/tools/live_check.py poc/a-llm/config.toml [--only NAME]
```
It reads the M0 config format, starts the real sidecar and runs `llm.test` per target.
Add `--m5` to run the M5 checks instead, per target: an image sent by `path` (vision targets), a streamed call that must report usage,
a call with `max_tokens: 5` whose `finish_reason` must be `length`, and `llm.model_info`.
Keys come from the env var named by `key_env`, else a hidden prompt; Entra targets need `az login`.
On the `stream` row, "first after X s of Y s" shows whether streaming is real: X close to Y means the stream is buffered.
