"""Role `ai`: LLM calls through LiteLLM, plus the Azure CLI sign-in check.

LiteLLM and azure-identity are imported on first use, so `ping` stays fast.
The route (which LiteLLM provider prefix, which auth) comes from the target's
`provider_kind`, `format` and `endpoint`, never from the model name (FR-LLM-PV-7).
"""
import base64
import json
import os
import struct
import threading
import time
import zlib

from .errors import FATAL_KINDS, InvalidParams, SidecarError, from_exception, from_status, is_timeout
from .extract import file_extract
from . import deepseek, model_download
from .web import Evidence, parse_web_search, request_params

AZURE_SCOPE = "https://cognitiveservices.azure.com/.default"
DEFAULT_API_VERSION = "2025-04-01-preview"
TIMEOUT = 90
NUM_RETRIES = 3  # rate limits and connection errors; tests lower this to keep 429 cases fast
TIMEOUT_RETRIES = 1  # a timeout is retried at most once: a stuck call must fail visibly (M5)
BACKOFF = 1.0  # seconds before the first retry, doubled each time; tests set it to 0
IMAGE_MIME = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif",
              ".webp": "image/webp"}

KINDS = ("azure_foundry", "anthropic", "openai_compatible", "local", "deepseek")
AUTHS = ("api_key", "azure_cli", "none")
FORMATS = ("chat_completions", "responses", "anthropic_messages")

_litellm = None
_import_lock = threading.Lock()
_register_lock = threading.Lock()
_registered = set()
_provider = None
_provider_lock = threading.Lock()


# ---------------------------------------------------------------- lazy libraries
def litellm():
    """Import LiteLLM once, with the environment settings from PROTOCOL.md."""
    global _litellm
    with _import_lock:
        if _litellm is None:
            os.environ.setdefault("LITELLM_LOCAL_MODEL_COST_MAP", "True")  # no GitHub fetch (workaround 1)
            os.environ.setdefault("LITELLM_LOG", "ERROR")
            # No `.env` lookup at import: a stray `.env` must never set keys or proxies, and the
            # lookup walks the call stack for a source file, which the compiled-only Windows bundle
            # doesn't have, so it failed every call with a bare AssertionError (M8).
            os.environ["LITELLM_MODE"] = "PRODUCTION"
            # The bundle leaves out LiteLLM's Rust extension (D-82): every route stays on Python.
            os.environ["LITELLM_RUST"] = "false"
            # LiteLLM's Azure handlers silently prefer these over Entra ID
            for k in ("AZURE_OPENAI_API_KEY", "AZURE_API_KEY", "AZURE_AI_API_KEY"):
                os.environ.pop(k, None)
            import litellm as mod
            mod.suppress_debug_info = True
            mod.telemetry = False
            _litellm = mod
    return _litellm


def token_provider():
    """Entra ID token provider (Azure CLI sign-in), created once, cached by azure-identity."""
    global _provider
    with _provider_lock:
        if _provider is None:
            from azure.identity import AzureCliCredential, get_bearer_token_provider
            _provider = get_bearer_token_provider(AzureCliCredential(), AZURE_SCOPE)
    return _provider


# ---------------------------------------------------------------- the target
class Target:
    """The `target` object of PROTOCOL.md, checked."""

    def __init__(self, d):
        if not isinstance(d, dict):
            raise InvalidParams("target must be an object")
        self.kind = d.get("provider_kind")
        self.format = d.get("format") or "chat_completions"
        self.auth = d.get("auth") or "none"
        self.endpoint = (d.get("endpoint") or "").strip()
        self.api_version = d.get("api_version")
        self.api_key = d.get("api_key")
        self.model = d.get("model") or ""
        self.vision = bool(d.get("vision"))
        self.web_search = bool(d.get("web_search"))  # M6: the model's Web search tick (llm.test only)
        eff = d.get("effort") or {}
        self.effort_enabled = bool(eff.get("enabled"))
        self.levels = list(eff.get("levels") or ["low", "medium", "high"])
        self.default_level = eff.get("default_level")
        self.custom = eff.get("custom") or None
        for name, value, allowed in (("provider_kind", self.kind, KINDS), ("auth", self.auth, AUTHS),
                                     ("format", self.format, FORMATS)):
            if value not in allowed:
                raise InvalidParams(f"target.{name} must be one of {', '.join(allowed)}")
        if self.kind == "azure_foundry" and not self.endpoint:
            raise InvalidParams("target.endpoint is required for azure_foundry")

    def route_model(self):
        """The LiteLLM model name this target would call, without any prefix for the Responses API."""
        prefix = {"azure_ai": "azure_ai/", "anthropic_foundry": "anthropic/", "anthropic": "anthropic/",
                  "azure": "azure/"}.get(self.style(), "openai/")
        return prefix + self.model

    def secrets(self):
        return (self.api_key,)

    def style(self):
        """Which LiteLLM route family this target uses (never looks at anything but the settings)."""
        if self.kind == "azure_foundry":
            if self.format == "anthropic_messages":
                # LiteLLM sends azure_ai/ to the Claude handler only when the name has "claude".
                return "azure_ai" if "claude" in self.model.lower() else "anthropic_foundry"
            return "openai" if "/openai/v1" in self.endpoint else "azure"
        if self.kind == "anthropic" or self.format == "anthropic_messages":
            return "anthropic"
        return "openai"  # openai_compatible, local and deepseek


def resolve_level(t, effort):
    """FR-LLM-EF-1/3: the level to send, or None."""
    if not t.effort_enabled:
        return None
    return effort or t.default_level or None


# ---------------------------------------------------------------- building a call
def _is_claude_style(style):
    return style in ("azure_ai", "anthropic", "anthropic_foundry")


def _image_url(b):
    """The data URL of an image block: given as `data_url`, or read from `path` (M5)."""
    if b.get("data_url"):
        return b["data_url"]
    path = b.get("path")
    if not isinstance(path, str) or not path:
        raise InvalidParams("an image block needs data_url or path")
    mime = IMAGE_MIME.get(os.path.splitext(path)[1].lower())
    if mime is None:
        raise InvalidParams(f"unsupported image type: {os.path.splitext(path)[1] or '(none)'}")
    try:
        with open(path, "rb") as f:
            raw = f.read()
    except OSError:
        raise SidecarError("bad_file", "The image file could not be read.")
    return f"data:{mime};base64,{base64.b64encode(raw).decode()}"


def _messages_chat(messages):
    out = []
    for m in messages:
        c = m.get("content")
        if isinstance(c, list):
            blocks = []
            for b in c:
                if b.get("type") == "image":
                    blocks.append({"type": "image_url", "image_url": {"url": _image_url(b)}})
                else:
                    blocks.append({"type": "text", "text": b.get("text", "")})
            c = blocks
        out.append({"role": m["role"], "content": c})
    return out


def _messages_responses(messages):
    out = []
    for m in messages:
        c = m.get("content")
        if isinstance(c, list):
            text_type = "output_text" if m["role"] == "assistant" else "input_text"
            c = [{"type": "input_image", "image_url": _image_url(b)} if b.get("type") == "image"
                 else {"type": text_type, "text": b.get("text", "")} for b in c]
        out.append({"role": m["role"], "content": c})
    return out


def effort_params(t, style, api, level):
    """FR-LLM-EF-2/4: how the level is sent. `custom` wins over the Auto mapping."""
    value = (t.custom or {}).get(level)
    extra = {}
    if value is not None and _is_claude_style(style) and str(value).strip().isdigit():
        budget = int(str(value).strip())  # Claude thinking budget in tokens
        return {"thinking": {"type": "enabled", "budget_tokens": budget}}, budget + 1024
    value = str(value) if value is not None else level
    if api == "responses":
        return {"reasoning": {"effort": value}}, None
    if style == "azure":
        extra["allowed_openai_params"] = ["reasoning_effort"]  # workaround 3: LiteLLM doesn't know deployment names
    extra["reasoning_effort"] = value
    return extra, None


def _register_model(prefix_model):
    """Workaround 2: LiteLLM fakes Responses streaming for models missing from its cost map."""
    prov, _, name = prefix_model.partition("/")
    name = name.removeprefix("responses/")
    key = f"{prov}/{name}"
    with _register_lock:
        if key not in _registered:
            litellm().register_model({key: {"litellm_provider": prov, "supports_native_streaming": True,
                                            "supports_reasoning": True, "supports_vision": True}})
            _registered.add(key)


def build_call(t, messages, level=None, stream=False, max_tokens=None, timeout=None, web_search=None):
    """Return (api, kwargs) for litellm.completion / litellm.responses. Fetches an Entra token if needed."""
    style = t.style()
    api = "responses" if t.format == "responses" and style in ("openai", "azure") else "chat"
    kw = {"num_retries": 0, "timeout": timeout or TIMEOUT}  # retries are ours (see _with_retries)

    if style == "azure_ai":
        prefix, kw["api_base"] = "azure_ai/", t.endpoint
    elif style == "anthropic_foundry":
        prefix, kw["api_base"] = "anthropic/", t.endpoint  # workaround 5: the name has no "claude"
    elif style == "anthropic":
        prefix = "anthropic/"
        if t.endpoint:
            kw["api_base"] = t.endpoint
    elif style == "azure":
        prefix = "azure/responses/" if api == "responses" else "azure/"
        kw["api_base"] = t.endpoint
        kw["api_version"] = t.api_version or DEFAULT_API_VERSION
    else:
        prefix = "openai/"
        base = t.endpoint or (deepseek.DEFAULT_BASE if t.kind == "deepseek" else "")
        if base:
            kw["api_base"] = base
    model = prefix + t.model
    kw["model"] = model

    # authentication
    if t.auth == "azure_cli":
        if style in ("azure", "azure_ai"):
            kw["azure_ad_token_provider"] = token_provider()  # refreshed by LiteLLM on every call
        else:  # workaround 4: no provider hook on these routes, so fetch a fresh token each call
            token = token_provider()()
            kw["api_key"] = token
            if style == "anthropic_foundry":
                kw["extra_headers"] = {"Authorization": f"Bearer {token}"}
    elif t.auth == "api_key" and t.api_key:
        kw["api_key"] = t.api_key
        if style == "anthropic_foundry":
            kw["extra_headers"] = {"api-key": t.api_key}
    else:
        kw["api_key"] = "none"  # servers without auth: LiteLLM insists on a value

    if style != "anthropic":
        _register_model(model)

    # effort
    if level:
        params, min_tokens = effort_params(t, style, api, level)
        kw.update(params)
        if min_tokens:
            max_tokens = max(min_tokens, max_tokens or 0)

    if api == "responses":
        kw["input"] = _messages_responses(messages)
        if max_tokens:
            kw["max_output_tokens"] = max_tokens
    else:
        kw["messages"] = _messages_chat(messages)
        if max_tokens:
            kw["max_tokens"] = max_tokens
        if stream:  # LiteLLM adds a usage chunk on every route, Anthropic included (M5)
            kw["stream_options"] = {"include_usage": True}
    if web_search:
        kw.update(request_params(api, _is_claude_style(style), web_search))
    kw["stream"] = bool(stream)
    return api, kw


# ---------------------------------------------------------------- running a call
def _field(obj, key):
    return obj.get(key) if isinstance(obj, dict) else getattr(obj, key, None)


def _usage(u):
    if u is None:
        return None
    get = lambda k: _field(u, k)  # noqa: E731
    i = get("prompt_tokens") if get("prompt_tokens") is not None else get("input_tokens")
    o = get("completion_tokens") if get("completion_tokens") is not None else get("output_tokens")
    if i is None and o is None:
        return None
    out = {"input_tokens": i or 0, "output_tokens": o or 0}
    # D-65: reasoning tokens, where LiteLLM puts them: `completion_tokens_details` (Chat Completions,
    # and Claude's thinking) or `output_tokens_details` (Responses API). Only a count above zero:
    # LiteLLM fills in 0 on some routes when the provider reported nothing.
    for details in ("completion_tokens_details", "output_tokens_details"):
        n = _field(get(details), "reasoning_tokens") if get(details) is not None else None
        if isinstance(n, int) and not isinstance(n, bool) and n > 0:
            out["reasoning_tokens"] = n
            break
    return out


FINISH = {"stop": "stop", "end_turn": "stop", "stop_sequence": "stop", "tool_calls": "stop", "tool_use": "stop",
          "length": "length", "max_tokens": "length", "max_output_tokens": "length"}


def _finish(reason):
    """Provider finish reasons -> stop | length | None."""
    return FINISH.get(reason) if isinstance(reason, str) else None


def _responses_finish(r):
    """Finish reason of a Responses API object: incomplete with max_output_tokens means length."""
    status = getattr(r, "status", None)
    if status == "incomplete":
        return _finish(getattr(getattr(r, "incomplete_details", None), "reason", None))
    return "stop" if status == "completed" else None


def _sleep(seconds, cancelled):
    if cancelled is not None:
        cancelled.wait(seconds)
    else:
        time.sleep(seconds)


def _with_retries(attempt, sent, cancelled=None):
    """Run `attempt()`; retry rate limits and connection errors (NUM_RETRIES times) and timeouts (TIMEOUT_RETRIES
    times) with a short backoff. Nothing is retried once `sent()` says text went out (a stream)."""
    retries = timeouts = 0
    while True:
        try:
            return attempt()
        except Exception as e:  # noqa: BLE001
            kind = from_exception(e).kind
            timed_out = is_timeout(e)
            if sent() or kind not in ("rate_limit", "network") or (cancelled is not None and cancelled.is_set()):
                raise
            if timed_out:
                timeouts += 1
                if timeouts > TIMEOUT_RETRIES:
                    raise
            else:
                retries += 1
                if retries > NUM_RETRIES:
                    raise
            _sleep(BACKOFF * 2 ** (retries + timeouts - 1), cancelled)


def run_completion(t, messages, level=None, stream=False, max_tokens=None, on_delta=None, cancelled=None, timeout=None,
                   web_search=None, evidence=None):
    """One LLM call. Returns {"text", "model", "usage", "finish_reason"}, plus "web" when `web_search` is given
    ({"max_uses": N}). `on_delta(text)` gets each streamed piece. `evidence` (an Evidence) lets the caller see
    the search evidence of a call that was cancelled."""
    if web_search and t.kind == "deepseek":
        # D-69: DeepSeek has no built-in search; Cairn runs it through DeepSeek's Search and Fetch API.
        ev = evidence if evidence is not None else Evidence()
        return deepseek.run(t, messages, level, stream, max_tokens, on_delta, cancelled, timeout, web_search, ev)
    lm = litellm()
    api, kw = build_call(t, messages, level, stream, max_tokens, timeout, web_search)
    fn = lm.responses if api == "responses" else lm.completion
    sent = []  # streamed pieces already delivered

    claude = _is_claude_style(t.style())
    ev = evidence if evidence is not None else Evidence()

    def attempt():
        ev.reset()  # a retry starts from nothing
        r = fn(**kw)
        out = _read_whole(t, api, r, ev) if not stream else _read_stream(t, api, r, sent, on_delta, cancelled, ev)
        if web_search:
            out["web"] = ev.result(claude)
        return out

    return _with_retries(attempt, lambda: bool(sent), cancelled)


def _read_whole(t, api, r, ev):
    ev.add_usage(getattr(r, "usage", None))
    if api == "responses":
        ev.read_responses(r)
        text = "".join(c.text for o in r.output if getattr(o, "type", "") == "message" for c in o.content
                       if getattr(c, "type", "") == "output_text")
        finish = _responses_finish(r)
    else:
        text = r.choices[0].message.content or ""
        ev.read_message(r.choices[0].message)
        finish = _finish(r.choices[0].finish_reason)
    return {"text": text, "model": getattr(r, "model", None) or t.model, "usage": _usage(getattr(r, "usage", None)),
            "finish_reason": finish}


def _read_stream(t, api, r, parts, on_delta, cancelled, evid):
    usage, model, finish, stopped = None, None, None, False
    try:
        for ev in r:
            if cancelled is not None and cancelled.is_set():
                stopped = True
                break
            delta = ""
            if api == "responses":
                kind = getattr(ev, "type", "")
                if kind == "response.output_text.delta":
                    delta = ev.delta
                elif kind in ("response.completed", "response.incomplete"):
                    usage = _usage(getattr(ev.response, "usage", None)) or usage
                    model = getattr(ev.response, "model", None) or model
                    finish = _responses_finish(ev.response) or finish
                    evid.read_responses(ev.response)
                elif kind == "response.output_item.done":
                    evid.read_responses({"output": [ev.item]})
                elif kind == "response.output_text.annotation.added":
                    evid.add_annotations([getattr(ev, "annotation", None)])
            else:
                if ev.choices:
                    d = ev.choices[0].delta
                    # Claude's server-side search arrives as tool_calls and provider_specific_fields: only
                    # `content` is text, and nothing here treats those calls as client tool calls.
                    delta = d.content or ""
                    evid.read_delta(d)
                    finish = _finish(ev.choices[0].finish_reason) or finish
                usage = _usage(getattr(ev, "usage", None)) or usage
                evid.add_usage(getattr(ev, "usage", None))
                model = getattr(ev, "model", None) or model
            if delta:
                parts.append(delta)
                if on_delta:
                    on_delta(delta)
    finally:
        _close_stream(r)
    return {"text": "".join(parts), "model": model or t.model, "usage": usage,
            "finish_reason": "cancelled" if stopped else finish}


def _close_stream(r):
    """Closes a stream at once, in this thread (2026-10-18). A half-read HTTP response left to the garbage
    collector can be closed while httpcore's connection pool holds its lock; that close then waits for the
    same lock forever, so a call through the shared client hangs and no timeout fires. LiteLLM's
    CustomStreamWrapper has only `aclose`, so the stream inside it (for Claude, the `iter_lines` generator
    of the HTTP response) is closed here as well. Closing an already finished stream does nothing."""
    inner = getattr(r, "completion_stream", None)
    for obj in (r, inner, getattr(inner, "streaming_response", None), getattr(inner, "response_iterator", None)):
        close = getattr(obj, "close", None)
        if callable(close):
            try:
                close()
            except Exception:  # noqa: BLE001
                pass


# ---------------------------------------------------------------- RPC methods
def llm_complete(req):
    p = req.params
    t = Target(p.get("target"))
    messages = p.get("messages")
    if not isinstance(messages, list) or not messages:
        raise InvalidParams("messages must be a non-empty list")
    for m in messages:
        if not isinstance(m, dict) or m.get("role") not in ("system", "user", "assistant"):
            raise InvalidParams("each message needs a role: system, user or assistant")
    stream = bool(p.get("stream"))
    timeout = p.get("timeout_s")
    if timeout is not None and (isinstance(timeout, bool) or not isinstance(timeout, (int, float)) or timeout <= 0):
        raise InvalidParams("timeout_s must be a positive number")
    web_search = parse_web_search(p.get("web_search"))
    evidence = Evidence()
    partial = []

    def on_delta(d):
        partial.append(d)
        req.stream(d)

    def cancel_result():
        out = {"text": "".join(partial), "model": t.model, "usage": None, "finish_reason": "cancelled",
               "cancelled": True}
        if web_search:
            out["web"] = evidence.result(_is_claude_style(t.style()))
        return out

    req.cancel_result = cancel_result
    level = resolve_level(t, p.get("effort"))
    return run_completion(t, messages, level, stream, p.get("max_tokens"),
                          on_delta if stream else None, req.cancelled, timeout, web_search, evidence)


def red_png(n=64):
    raw = b"".join(b"\x00" + b"\xff\x00\x00" * n for _ in range(n))

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))
    return "data:image/png;base64," + base64.b64encode(png).decode()


def _user(text, image=None):
    if image is None:
        return [{"role": "user", "content": text}]
    return [{"role": "user", "content": [{"type": "text", "text": text}, {"type": "image", "data_url": image}]}]


def llm_test(req):
    t = Target(req.params.get("target"))
    secrets = t.secrets()
    checks = []

    def record(name, fn):
        """Run one check; fn returns (ok, detail). Returns the error kind if it raised."""
        t0 = time.time()
        try:
            ok, detail = fn()
            checks.append({"name": name, "ok": ok, "detail": detail.replace("{t}", f"{time.time() - t0:.1f}")})
            return None
        except Exception as e:  # noqa: BLE001
            err = from_exception(e, secrets)
            hint = err.data().get("hint")
            checks.append({"name": name, "ok": False, "detail": err.message + (f" [{hint}]" if hint else "")})
            return err.kind

    def basic():
        r = run_completion(t, _user("Reply with exactly: pong"))
        return bool(r["text"].strip()), "replied in {t} s" if r["text"].strip() else "empty reply"

    def stream():
        stamps = []
        t0 = time.time()
        run_completion(t, _user("Count from 1 to 40, one number per line."), stream=True,
                       on_delta=lambda d: stamps.append(time.time() - t0))
        total = time.time() - t0
        if len(stamps) > 1:
            # Real streaming: the first piece arrives well before the end. A faked stream
            # (buffered, then replayed) shows first == total.
            return True, f"{len(stamps)} text chunks, first after {stamps[0]:.2f} s of {total:.2f} s"
        return False, f"{len(stamps)} text chunk: the stream is probably not real (the reply came in one piece)"

    def effort(level):
        def run():
            r = run_completion(t, _user("Reply with exactly: pong"), level=level)
            return bool(r["text"].strip()), "accepted" if r["text"].strip() else "empty reply"
        return run

    def image():
        r = run_completion(t, _user("What colour is this image? Answer with one word.", red_png()))
        return "red" in r["text"].lower(), r["text"].strip()[:60] or "empty reply"

    def web():
        r = run_completion(t, _user("Search the web: what is today's date?"), web_search={"max_uses": 1})
        n = r["web"]["searches"]
        if n <= 0:
            return False, "no search was made: the provider ignores web search"
        return True, f"{n} search{'es' if n > 1 else ''}, {len(r['web']['sources'])} sources, in " + "{t} s"

    kind = record("basic", basic)
    if kind in FATAL_KINDS:
        later = ["stream"] + [f"effort:{lv}" for lv in t.levels] + ["image", "web"]
        for name in later:
            checks.append({"name": name, "ok": None, "detail": "skipped: basic call failed"})
        return {"checks": checks}
    record("stream", stream)
    if t.effort_enabled:
        for lv in t.levels:
            record(f"effort:{lv}", effort(lv))
    else:
        for lv in t.levels:
            checks.append({"name": f"effort:{lv}", "ok": None, "detail": "skipped: effort is off for this model"})
    if t.vision:
        record("image", image)
    else:
        checks.append({"name": "image", "ok": None, "detail": "skipped: the model is not marked as vision-capable"})
    if t.web_search:
        record("web", web)
    else:
        checks.append({"name": "web", "ok": None, "detail": "skipped: web search is off for this model"})
    return {"checks": checks}


def llm_model_info(req):
    """Token limits from LiteLLM's local model map (no network); null when unknown."""
    t = Target(req.params.get("target"))
    out = {"max_input_tokens": None, "max_output_tokens": None}
    try:
        info = litellm().get_model_info(model=t.route_model())
    except Exception:  # noqa: BLE001 - unknown models raise
        return out
    for k in out:
        v = info.get(k) if isinstance(info, dict) else None
        out[k] = v if isinstance(v, int) and not isinstance(v, bool) and v > 0 else None
    return out


def llm_list_models(req):
    t = Target(req.params.get("target"))
    if t.kind == "azure_foundry":
        raise SidecarError("unsupported", "Azure deployments can't be listed (that needs Azure admin rights). "
                           "Type the deployment name.", hint="Type the deployment name from the Azure portal.")
    import httpx
    headers = {}
    if t.kind == "anthropic":
        base = (t.endpoint or "https://api.anthropic.com").rstrip("/")
        url = base + "/v1/models?limit=1000"
        headers = {"x-api-key": t.api_key or "", "anthropic-version": "2023-06-01"}
    elif t.kind == "deepseek":
        url = (t.endpoint or deepseek.DEFAULT_BASE).rstrip("/") + "/models"  # DeepSeek's documented path
        headers = {"Authorization": f"Bearer {t.api_key or ''}"}
    else:
        base = t.endpoint.rstrip("/")
        if not base:
            raise InvalidParams("target.endpoint is required")
        url = base + ("/models" if base.endswith("/v1") else "/v1/models")
        if t.api_key and t.auth == "api_key":
            headers = {"Authorization": f"Bearer {t.api_key}"}
    try:
        resp = httpx.get(url, headers=headers, timeout=30)
    except Exception as e:  # noqa: BLE001
        raise from_exception(e, t.secrets())
    if resp.status_code >= 400:
        raise from_status(resp.status_code, resp.text)
    try:
        data = resp.json().get("data") or []
    except ValueError:
        raise SidecarError("internal", "The server's model list was not valid JSON.")
    models = [{"id": m["id"], "label": m.get("display_name")} for m in data if isinstance(m, dict) and m.get("id")]
    return {"models": models}


def _jwt_account(token):
    try:
        payload = token.split(".")[1]
        claims = json.loads(base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4)))
        return claims.get("upn") or claims.get("preferred_username") or claims.get("unique_name") or claims.get("email")
    except Exception:  # noqa: BLE001
        return None


def azure_check_signin(req):
    try:
        from azure.identity import AzureCliCredential
        tok = AzureCliCredential().get_token(AZURE_SCOPE)
    except Exception as e:  # noqa: BLE001
        why = " ".join(str(e).split())[:200]
        return {"signed_in": False, "account": None,
                "detail": "Run `az login` in a terminal, then check again." + (f" ({why})" if why else "")}
    until = time.strftime("%H:%M", time.localtime(tok.expires_on))
    return {"signed_in": True, "account": _jwt_account(tok.token), "detail": f"token valid until {until}"}


def methods():
    return {
        "llm.complete": (llm_complete, None),
        "llm.test": (llm_test, None),
        "llm.list_models": (llm_list_models, None),
        "llm.model_info": (llm_model_info, None),
        "file.extract": (file_extract, None),
        "azure.check_signin": (azure_check_signin, None),
        **model_download.methods(),
    }


def secrets_of(params):
    t = params.get("target") if isinstance(params, dict) else None
    return (t.get("api_key"),) if isinstance(t, dict) else ()
