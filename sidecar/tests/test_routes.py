"""Every route x format against the local mock (no real network)."""
import json

import pytest

from cairn_sidecar import ai
from cairn_sidecar.errors import from_exception
from helpers import FakeReq, PNG, target, user

TOKENS = []


@pytest.fixture(autouse=True)
def fake_entra(monkeypatch):
    TOKENS.clear()

    def provider():
        def get():
            TOKENS.append(1)
            return f"fake-entra-token-{len(TOKENS)}"
        return get
    monkeypatch.setattr(ai, "token_provider", provider)


# name -> (target overrides given the mock url, expected path, auth header kind)
def cases(url):
    return {
        "foundry_v1_chat": (dict(provider_kind="azure_foundry", endpoint=url + "/openai/v1"), "/openai/v1/chat/completions", "bearer"),
        "foundry_v1_resp": (dict(provider_kind="azure_foundry", endpoint=url + "/openai/v1", format="responses"), "/openai/v1/responses", "bearer"),
        "azure_chat": (dict(provider_kind="azure_foundry", endpoint=url, api_version="2025-04-01-preview"), "/openai/deployments/dep1/chat/completions", "api-key"),
        "azure_chat_default_version": (dict(provider_kind="azure_foundry", endpoint=url), "/openai/deployments/dep1/chat/completions", "api-key"),
        "azure_resp": (dict(provider_kind="azure_foundry", endpoint=url, format="responses"), "/openai/responses", "api-key"),
        "foundry_claude": (dict(provider_kind="azure_foundry", endpoint=url + "/anthropic", format="anthropic_messages", model="claude-sonnet-4-5"), "/anthropic/v1/messages", "api-key"),
        "foundry_claude_other_name": (dict(provider_kind="azure_foundry", endpoint=url + "/anthropic", format="anthropic_messages", model="my-sonnet"), "/anthropic/v1/messages", "x-api-key"),
        "anthropic": (dict(provider_kind="anthropic", endpoint=url, format="anthropic_messages", model="claude-sonnet-4-5"), "/v1/messages", "x-api-key"),
        "openai_compat_chat": (dict(provider_kind="openai_compatible", endpoint=url + "/v1"), "/v1/chat/completions", "bearer"),
        "openai_compat_resp": (dict(provider_kind="openai_compatible", endpoint=url + "/v1", format="responses"), "/v1/responses", "bearer"),
        "local_chat": (dict(provider_kind="local", endpoint=url + "/v1"), "/v1/chat/completions", "bearer"),
        "deepseek_chat": (dict(provider_kind="deepseek", endpoint=url), "/chat/completions", "bearer"),
    }


NAMES = list(cases("x"))


def make(mock, name, **over):
    ov, path, hdr = cases(mock.url)[name]
    return target(**{**ov, **over}), path, hdr


def run(t, **kw):
    return ai.run_completion(ai.Target(t), kw.pop("messages", user()), **kw)


def effort_keys(body):
    return {k for k in ("reasoning_effort", "reasoning", "thinking", "output_config") if k in body}


@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("name", NAMES)
def test_basic_path_and_key_auth(name, stream, mock):
    t, path, hdr = make(mock, name)
    deltas = []
    r = run(t, stream=stream, on_delta=deltas.append)
    assert r["text"] == "Hello from mock"
    assert deltas == (["Hello ", "from mock"] if stream else [])
    req = mock.requests[-1]
    assert req["path"] == path
    h = req["headers"]
    if hdr == "api-key":
        assert h["api-key"] == "SECRET" and "authorization" not in h
    elif hdr == "x-api-key":
        assert h["x-api-key"] == "SECRET"
    else:
        assert h["authorization"] == "Bearer SECRET"
    assert effort_keys(req["body"]) == set(), "no effort unless asked"
    assert bool(req["body"].get("stream")) == stream
    assert req["body"]["model"] == t["model"]
    if name.startswith("azure_chat") or name == "azure_resp":
        assert req["query"].startswith("api-version=")
        assert ("2025-04-01-preview" in req["query"]) or name == "azure_chat_default_version"
    if name == "azure_chat_default_version":
        assert req["query"] == "api-version=2025-04-01-preview"


def test_result_has_model_and_usage(mock):
    t, *_ = make(mock, "openai_compat_chat")
    r = run(t)
    assert r["model"] == "dep1" and r["usage"] == {"input_tokens": 5, "output_tokens": 3}
    t, *_ = make(mock, "foundry_v1_resp")
    assert run(t)["usage"] == {"input_tokens": 5, "output_tokens": 3}


def test_local_without_key_sends_placeholder(mock):
    t, path, _ = make(mock, "local_chat", auth="none", api_key=None)
    assert run(t)["text"] == "Hello from mock"
    assert mock.requests[-1]["headers"]["authorization"] == "Bearer none"


# ---- Foundry Claude: the route comes from the settings, not from the name (FR-LLM-PV-7)
def test_foundry_claude_without_claude_in_name_uses_messages_route(mock):
    t, *_ = make(mock, "foundry_claude_other_name")
    run(t)
    req = mock.requests[-1]
    assert req["path"] == "/anthropic/v1/messages"
    assert req["body"]["model"] == "my-sonnet"
    assert req["headers"]["api-key"] == "SECRET" and req["headers"]["x-api-key"] == "SECRET"


@pytest.mark.parametrize("name", ["foundry_claude", "foundry_claude_other_name"])
def test_foundry_claude_entra(name, mock):
    t, path, _ = make(mock, name, auth="azure_cli", api_key=None)
    run(t)
    run(t)
    reqs = mock.requests[-2:]
    assert [r["path"] for r in reqs] == [path, path]
    assert reqs[0]["headers"]["authorization"] == "Bearer fake-entra-token-1"
    assert reqs[1]["headers"]["authorization"] == "Bearer fake-entra-token-2"
    assert "api-key" not in reqs[0]["headers"] or name.endswith("other_name")


# ---- Entra on the other routes
@pytest.mark.parametrize("name", ["azure_chat", "azure_resp", "foundry_v1_chat", "foundry_v1_resp"])
def test_entra_token_fresh_per_call(name, mock):
    t, path, _ = make(mock, name, auth="azure_cli", api_key=None)
    run(t)
    run(t, stream=True, on_delta=lambda d: None)
    a, b = mock.requests[-2:]
    assert a["headers"]["authorization"] == "Bearer fake-entra-token-1"
    assert b["headers"]["authorization"] == "Bearer fake-entra-token-2"
    assert "api-key" not in a["headers"]


# ---- effort
def level_body(mock, name, level, effort=None, **kw):
    t, *_ = make(mock, name, effort=effort or {})
    run(t, level=level, **kw)
    return mock.requests[-1]["body"]


@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("level", ["low", "medium", "high"])
@pytest.mark.parametrize("name", ["foundry_v1_chat", "azure_chat", "openai_compat_chat", "local_chat"])
def test_effort_chat_uses_reasoning_effort(name, level, stream, mock):
    body = level_body(mock, name, level, stream=stream, on_delta=lambda d: None)
    assert body["reasoning_effort"] == level and "reasoning" not in body


@pytest.mark.parametrize("level", ["low", "medium", "high"])
@pytest.mark.parametrize("name", ["foundry_v1_resp", "azure_resp", "openai_compat_resp"])
def test_effort_responses_uses_reasoning_effort_object(name, level, mock):
    body = level_body(mock, name, level)
    assert body["reasoning"] == {"effort": level} and "reasoning_effort" not in body


@pytest.mark.parametrize("level", ["low", "medium", "high"])
@pytest.mark.parametrize("name", ["foundry_claude", "foundry_claude_other_name", "anthropic"])
def test_effort_claude_auto_is_native(name, level, mock):
    body = level_body(mock, name, level)
    assert "thinking" in body or "output_config" in body
    assert "reasoning_effort" not in body


def test_effort_custom_numeric_is_claude_thinking_budget(mock):
    eff = {"custom": {"low": "2000", "high": "9000"}}
    body = level_body(mock, "foundry_claude", "high", eff)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 9000}
    assert body["max_tokens"] > 9000
    body = level_body(mock, "foundry_claude_other_name", "low", eff)
    assert body["thinking"] == {"type": "enabled", "budget_tokens": 2000}
    body = level_body(mock, "anthropic", "low", eff)
    assert body["thinking"]["budget_tokens"] == 2000


def test_effort_custom_text_value_replaces_level(mock):
    eff = {"custom": {"high": "xhigh"}}
    assert level_body(mock, "openai_compat_chat", "high", eff)["reasoning_effort"] == "xhigh"
    assert level_body(mock, "openai_compat_resp", "high", eff)["reasoning"] == {"effort": "xhigh"}
    # a level without a custom value falls back to Auto
    assert level_body(mock, "openai_compat_chat", "low", eff)["reasoning_effort"] == "low"


def test_effort_level_not_sent_when_none_or_disabled(mock):
    for name in ("openai_compat_chat", "foundry_v1_resp", "foundry_claude"):
        t, *_ = make(mock, name)
        assert ai.resolve_level(ai.Target(t), None) is None
        run(t, level=None)
        assert effort_keys(mock.requests[-1]["body"]) == set()
    off = ai.Target(make(mock, "openai_compat_chat", effort={"enabled": False, "default_level": "high"})[0])
    assert ai.resolve_level(off, "high") is None and ai.resolve_level(off, None) is None


def test_effort_default_level_used_when_chat_sends_none(mock):
    t = ai.Target(make(mock, "openai_compat_chat", effort={"default_level": "medium"})[0])
    assert ai.resolve_level(t, None) == "medium"
    assert ai.resolve_level(t, "high") == "high"
    r = FakeReq({"target": t_dict(mock, "openai_compat_chat", effort={"default_level": "medium"}), "messages": user()})
    ai.llm_complete(r)
    assert mock.requests[-1]["body"]["reasoning_effort"] == "medium"


# ---- reasoning tokens (D-65): read from what LiteLLM makes of the provider's usage
@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
def test_reasoning_tokens_reported_on_chat(stream, mock):
    t, *_ = make(mock, "openai_compat_chat")
    r = run(t, level="high", stream=stream, on_delta=lambda d: None)
    assert r["usage"]["reasoning_tokens"] == 2


def test_reasoning_tokens_reported_on_responses(mock):
    t, *_ = make(mock, "openai_compat_resp")
    assert run(t, level="high")["usage"]["reasoning_tokens"] == 2


def test_no_reasoning_tokens_when_the_provider_reports_none(mock):
    t, *_ = make(mock, "openai_compat_chat")
    assert "reasoning_tokens" not in run(t)["usage"]


def test_usage_reads_reasoning_from_dicts_and_objects():
    from types import SimpleNamespace as NS
    d = {"prompt_tokens": 1, "completion_tokens": 9, "completion_tokens_details": {"reasoning_tokens": 7}}
    assert ai._usage(d) == {"input_tokens": 1, "output_tokens": 9, "reasoning_tokens": 7}
    o = NS(prompt_tokens=None, completion_tokens=None, input_tokens=1, output_tokens=9,
           completion_tokens_details=None, output_tokens_details=NS(reasoning_tokens=4))
    assert ai._usage(o)["reasoning_tokens"] == 4
    for n in (None, 0):
        none = {"prompt_tokens": 1, "completion_tokens": 2, "completion_tokens_details": {"reasoning_tokens": n}}
        assert "reasoning_tokens" not in ai._usage(none)


# ---- streams are closed right after reading (2026-10-18)
def _open_http_streams():
    """Half-read httpx response generators that are still alive (suspended, not closed)."""
    import gc
    import types
    return [o for o in gc.get_objects() if isinstance(o, types.GeneratorType) and o.gi_frame is not None
            and "/httpx/" in o.gi_code.co_filename]


@pytest.mark.parametrize("name", ["anthropic", "foundry_claude", "openai_compat_chat", "openai_compat_resp"])
def test_a_stream_leaves_no_open_http_response(name, mock):
    """A stream must be closed in the calling thread: one left to the garbage collector can be closed
    while httpcore's connection pool holds its lock, and that close then waits for the lock forever (a
    later call hung, 2026-10-18). LiteLLM's CustomStreamWrapper has no sync close()."""
    import gc
    gc.collect()
    before = len(_open_http_streams())
    t, *_ = make(mock, name)
    for _ in range(3):
        run(t, stream=True, on_delta=lambda d: None)
    assert len(_open_http_streams()) == before


def t_dict(mock, name, **over):
    return make(mock, name, **over)[0]


def test_azure_chat_effort_with_arbitrary_deployment_name(mock):
    """Workaround 3: LiteLLM would refuse reasoning_effort for an unknown deployment name."""
    t, *_ = make(mock, "azure_chat", model="my-custom-deployment")
    run(t, level="low")
    assert mock.requests[-1]["body"]["reasoning_effort"] == "low"


# ---- images
@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("name", NAMES)
def test_image_input(name, stream, mock):
    t, *_ = make(mock, name)
    run(t, messages=user(image=True), stream=stream, on_delta=lambda d: None)
    body = mock.requests[-1]["body"]
    assert PNG.split(",")[1] in json.dumps(body)
    if t["format"] == "anthropic_messages":
        img = [b for b in body["messages"][0]["content"] if b["type"] == "image"][0]
        assert img["source"]["type"] == "base64" and img["source"]["media_type"] == "image/png"
    elif t["format"] == "chat_completions":
        assert any(b["type"] == "image_url" for b in body["messages"][0]["content"])
    else:
        assert any(b["type"] == "input_image" for b in body["input"][0]["content"])


def test_system_and_history_messages(mock):
    t, *_ = make(mock, "openai_compat_chat")
    msgs = [{"role": "system", "content": "be brief"}, {"role": "user", "content": "a"},
            {"role": "assistant", "content": "b"}, {"role": "user", "content": "c"}]
    run(t, messages=msgs, max_tokens=50)
    body = mock.requests[-1]["body"]
    assert [m["role"] for m in body["messages"]] == ["system", "user", "assistant", "user"]
    assert body["max_tokens"] == 50
    t, *_ = make(mock, "openai_compat_resp")
    run(t, messages=msgs, max_tokens=50)
    body = mock.requests[-1]["body"]
    assert [m["role"] for m in body["input"]] == ["system", "user", "assistant", "user"]
    assert body["max_output_tokens"] == 50


# ---- Responses streaming is real (workaround 2)
@pytest.mark.parametrize("name", ["foundry_v1_resp", "azure_resp", "openai_compat_resp"])
def test_responses_streaming_is_native_even_for_unknown_model(name, mock):
    t, *_ = make(mock, name, model="never-heard-of-it")
    run(t, stream=True, on_delta=lambda d: None)
    assert mock.requests[-1]["body"]["stream"] is True


# ---- errors
@pytest.mark.parametrize("scenario,kind", [("401", "auth"), ("403", "forbidden")])
@pytest.mark.parametrize("name", NAMES)
def test_auth_errors_map_to_kinds(name, scenario, kind, mock):
    mock.reset(scenario)
    t, *_ = make(mock, name)
    with pytest.raises(Exception) as ei:
        run(t)
    err = from_exception(ei.value, ["SECRET"])
    assert err.kind == kind and err.status == int(scenario)
    assert "access denied" in err.message
    assert err.data()["hint"]
    assert len(mock.requests) == 1  # not retried


@pytest.mark.parametrize("name", ["openai_compat_chat", "foundry_claude", "foundry_v1_resp"])
def test_429_maps_to_rate_limit(name, mock):
    mock.reset("429")
    t, *_ = make(mock, name)
    with pytest.raises(Exception) as ei:
        run(t)
    err = from_exception(ei.value)
    assert err.kind == "rate_limit" and err.status == 429


def test_429_is_retried_then_succeeds(mock, monkeypatch):
    monkeypatch.setattr(ai, "NUM_RETRIES", 3)
    mock.reset("429_then_ok")
    t, *_ = make(mock, "foundry_claude")
    assert run(t)["text"] == "Hello from mock"
    assert len(mock.requests) == 3


def test_unreachable_endpoint_is_network_error():
    t = target(endpoint="http://127.0.0.1:9/v1")
    with pytest.raises(Exception) as ei:
        run(t)
    assert from_exception(ei.value).kind == "network"


def test_api_key_is_scrubbed_from_error_messages():
    err = from_exception(RuntimeError("bad key sk-live-123456 sent"), ["sk-live-123456"])
    assert "sk-live-123456" not in err.message


def test_invalid_target_is_invalid_params():
    from cairn_sidecar.errors import InvalidParams
    with pytest.raises(InvalidParams):
        ai.Target({"provider_kind": "nope"})
    with pytest.raises(InvalidParams):
        ai.Target({"provider_kind": "azure_foundry", "endpoint": ""})


def test_foundry_claude_unknown_deployment_name_with_effort(mock):
    """LiteLLM doesn't know this name: the model is registered so that effort is still accepted."""
    t, *_ = make(mock, "foundry_claude", model="claude-my-deployment")
    run(t, level="medium")
    body = mock.requests[-1]["body"]
    assert body["model"] == "claude-my-deployment" and ("thinking" in body or "output_config" in body)
