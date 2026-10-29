"""M0-a: what does LiteLLM actually send? All traffic goes to a localhost mock."""
import base64, json, pytest, litellm

PNG = "data:image/png;base64," + base64.b64encode(b"\x89PNG\r\n\x1a\nfake").decode()
EFFORTS = ["low", "medium", "high"]
TOKEN_CALLS = []


def fake_provider():  # stands in for get_bearer_token_provider(AzureCliCredential(), scope)
    TOKEN_CALLS.append(1)
    return f"fake-entra-token-{len(TOKEN_CALLS)}"


# name -> (kind, model, api_base suffix, auth kwargs, expected path, expected auth header, effort kind)
# kind: chat | responses.  effort kinds: chat -> reasoning_effort, resp -> reasoning.effort, claude -> see test
CASES = {
    "azure_classic_chat_key":    ("chat", "azure/dep1", "", "azure_key", "/openai/deployments/dep1/chat/completions", "api-key", "chat"),
    "azure_classic_chat_entra":  ("chat", "azure/dep1", "", "azure_entra", "/openai/deployments/dep1/chat/completions", "bearer", "chat"),
    "azure_classic_resp_key":    ("responses", "azure/responses/dep1", "", "azure_key", "/openai/responses", "api-key", "resp"),
    "azure_classic_resp_entra":  ("responses", "azure/responses/dep1", "", "azure_entra", "/openai/responses", "bearer", "resp"),
    "foundry_v1_chat_key":       ("chat", "openai/dep1", "/openai/v1", "key", "/openai/v1/chat/completions", "bearer", "chat"),
    "foundry_v1_chat_entra":     ("chat", "openai/dep1", "/openai/v1", "entra_as_key", "/openai/v1/chat/completions", "bearer", "chat"),
    "foundry_v1_resp_key":       ("responses", "openai/dep1", "/openai/v1", "key", "/openai/v1/responses", "bearer", "resp"),
    "foundry_v1_resp_entra":     ("responses", "openai/dep1", "/openai/v1", "entra_as_key", "/openai/v1/responses", "bearer", "resp"),
    "openai_compat_chat":        ("chat", "openai/llama3", "/v1", "key", "/v1/chat/completions", "bearer", "chat"),
    "openai_compat_resp":        ("responses", "openai/llama3", "/v1", "key", "/v1/responses", "bearer", "resp"),
    "claude_messages":           ("chat", "anthropic/claude-sonnet-4-5", "", "key", "/v1/messages", "x-api-key", "claude"),
}
EFFORT_KEYS = ("reasoning_effort", "reasoning", "thinking", "output_config")


def auth_kwargs(mode):
    if mode == "azure_key":    return {"api_key": "SECRET", "api_version": "2025-04-01-preview"}
    if mode == "azure_entra":  return {"azure_ad_token_provider": fake_provider, "api_version": "2025-04-01-preview"}
    if mode == "entra_as_key": return {"api_key": fake_provider()}   # token fetched per request by our code
    return {"api_key": "SECRET"}


def register_native_streaming(model):
    """Without this LiteLLM silently fakes Responses-API streaming for models missing from its cost map."""
    prov, _, name = model.partition("/")
    name = name.removeprefix("responses/")
    litellm.register_model({f"{prov}/{name}": {"litellm_provider": prov, "supports_native_streaming": True, "supports_reasoning": True, "supports_vision": True}})


def run(case, mock, effort=None, stream=False, image=False, **extra):
    kind, model, suffix, auth, *_ = CASES[case]
    kw = dict(model=model, api_base=mock.url + suffix, **auth_kwargs(auth), **extra)
    text = "Transcribe the text only."
    if kind == "chat":
        content = [{"type": "text", "text": text}] + ([{"type": "image_url", "image_url": {"url": PNG}}] if image else [])
        kw["messages"] = [{"role": "user", "content": content}]
        if effort: kw["reasoning_effort"] = effort
        # Azure deployment names are arbitrary: LiteLLM's per-model capability table does not know them.
        if effort and model.startswith("azure/"): kw["allowed_openai_params"] = ["reasoning_effort"]
        r = litellm.completion(stream=stream, **kw)
        if not stream: return r.choices[0].message.content
        return "".join((c.choices[0].delta.content or "") for c in r)
    content = [{"type": "input_text", "text": text}] + ([{"type": "input_image", "image_url": PNG}] if image else [])
    register_native_streaming(model)
    kw["input"] = [{"role": "user", "content": content}]
    if effort: kw["reasoning"] = {"effort": effort}
    r = litellm.responses(stream=stream, **kw)
    if not stream: return r.output[0].content[0].text
    out = ""
    for ev in r:
        if getattr(ev, "type", "") == "response.output_text.delta": out += ev.delta
    return out


def check_auth(req, expected):
    h = req["headers"]
    if expected == "api-key":
        assert h["api-key"] == "SECRET" and "authorization" not in h
    elif expected == "bearer":
        assert h["authorization"].startswith("Bearer ") and "api-key" not in h
    elif expected == "x-api-key":
        assert h["x-api-key"] == "SECRET" and "authorization" not in h


@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("case", CASES)
def test_basic_and_auth_and_effort_unset(case, stream, mock):
    _, _, _, auth, path, hdr, _ = CASES[case]
    assert run(case, mock, stream=stream) == "Hello from mock"
    req = mock.requests[-1]
    assert req["path"] == path
    check_auth(req, hdr)
    if auth == "azure_entra" or auth == "entra_as_key":
        assert req["headers"]["authorization"].startswith("Bearer fake-entra-token-")
    for k in EFFORT_KEYS:
        assert k not in req["body"], f"{k} must not be sent when effort is unset"
    assert bool(req["body"].get("stream")) == stream
    if path.startswith("/openai/") and "/v1" not in path:
        assert req["query"].startswith("api-version=")


@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("effort", EFFORTS)
@pytest.mark.parametrize("case", CASES)
def test_effort_set(case, effort, stream, mock):
    kind = CASES[case][6]
    assert run(case, mock, effort=effort, stream=stream) == "Hello from mock"
    body = mock.requests[-1]["body"]
    if kind == "chat":
        assert body["reasoning_effort"] == effort and "reasoning" not in body
    elif kind == "resp":
        assert body["reasoning"] == {"effort": effort} and "reasoning_effort" not in body
    else:  # claude-sonnet-4-5: thinking budget
        budget = {"low": 1024, "medium": 2048, "high": 4096}[effort]
        assert body["thinking"] == {"type": "enabled", "budget_tokens": budget}
        assert body["max_tokens"] > budget


@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("case", CASES)
def test_image_input(case, stream, mock):
    kind = CASES[case][0]
    assert run(case, mock, image=True, stream=stream) == "Hello from mock"
    body = mock.requests[-1]["body"]
    s = json.dumps(body)
    assert PNG.split(",")[1] in s  # image bytes present
    if case == "claude_messages":
        blocks = body["messages"][0]["content"]
        img = [b for b in blocks if b["type"] == "image"][0]
        assert img["source"]["type"] == "base64" and img["source"]["media_type"] == "image/png"
    elif kind == "chat":
        assert any(b["type"] == "image_url" for b in body["messages"][0]["content"])
    else:
        assert any(b["type"] == "input_image" for b in body["input"][0]["content"])


@pytest.mark.parametrize("model,expect", [
    ("claude-opus-4-1", ("enabled", None)), ("claude-haiku-4-5", ("enabled", None)),
    ("claude-sonnet-4-6", ("adaptive", "high")), ("claude-opus-4-7", ("adaptive", "high")),
])
def test_claude_effort_by_model_generation(model, expect, mock):
    litellm.completion(model="anthropic/" + model, api_base=mock.url, api_key="SECRET", reasoning_effort="high",
                       messages=[{"role": "user", "content": "hi"}])
    body = mock.requests[-1]["body"]
    assert body["thinking"]["type"] == expect[0]
    assert body.get("output_config", {}).get("effort") == expect[1]


def test_azure_arbitrary_deployment_name_rejects_effort_without_allowed_params(mock):
    """RISK: capability lookup is by model-name; custom deployment names are refused unless overridden."""
    with pytest.raises(litellm.UnsupportedParamsError):
        litellm.completion(model="azure/my-custom-deployment", api_base=mock.url, api_key="SECRET", api_version="2025-04-01-preview",
                           reasoning_effort="low", messages=[{"role": "user", "content": "hi"}])


def test_azure_v1_endpoint_with_azure_prefix_is_broken(mock):
    """azure/ prefix + /openai/v1 base doubles the path -> use openai/ prefix for the v1 style."""
    litellm.completion(model="azure/dep1", api_base=mock.url + "/openai/v1/", api_key="SECRET", api_version="preview",
                       messages=[{"role": "user", "content": "hi"}])
    assert mock.requests[-1]["path"] != "/openai/v1/chat/completions"


def test_azure_ai_prefix_responses_not_routed_to_responses_api(mock):
    litellm.responses(model="azure_ai/dep1", api_base=mock.url + "/openai/v1", api_key="SECRET", input="hi")
    assert mock.requests[-1]["path"].endswith("/chat/completions")  # silently falls back to chat -> avoid


def test_entra_token_provider_called_per_request_so_refresh_works(mock):
    TOKEN_CALLS.clear()
    for _ in range(3):
        litellm.completion(model="azure/dep1", api_base=mock.url, azure_ad_token_provider=fake_provider,
                           api_version="2025-04-01-preview", messages=[{"role": "user", "content": "hi"}])
    toks = [r["headers"]["authorization"] for r in mock.requests]
    assert toks == [f"Bearer fake-entra-token-{i}" for i in (1, 2, 3)]


def test_callable_api_key_not_supported_on_openai_prefix(mock):
    with pytest.raises(Exception):
        litellm.completion(model="openai/dep1", api_base=mock.url + "/openai/v1", api_key=fake_provider,
                           messages=[{"role": "user", "content": "hi"}])


# ---- NFR-NET-2 ----
ERR_CALLS = {
    "azure_chat": lambda u: litellm.completion(model="azure/d", api_base=u, api_key="K", api_version="x", messages=[{"role": "user", "content": "hi"}]),
    "openai_chat": lambda u: litellm.completion(model="openai/d", api_base=u + "/v1", api_key="K", messages=[{"role": "user", "content": "hi"}]),
    "openai_resp": lambda u: litellm.responses(model="openai/d", api_base=u + "/v1", api_key="K", input="hi"),
    "azure_resp": lambda u: litellm.responses(model="azure/responses/d", api_base=u, api_key="K", api_version="x", input="hi"),
    "claude": lambda u: litellm.completion(model="anthropic/claude-sonnet-4-5", api_base=u, api_key="K", messages=[{"role": "user", "content": "hi"}]),
}


@pytest.mark.parametrize("code", [401, 403])
@pytest.mark.parametrize("name", ERR_CALLS)
def test_auth_errors_are_clear_and_not_retried(name, code, mock):
    mock.reset(str(code))
    with pytest.raises(Exception) as ei:
        ERR_CALLS[name](mock.url)
    assert getattr(ei.value, "status_code", None) == code
    assert "access denied" in str(ei.value)
    assert len(mock.requests) == 1


@pytest.mark.parametrize("name", ERR_CALLS)
def test_429_retried_with_num_retries(name, mock):
    mock.reset("429_then_ok")
    kind = ERR_CALLS[name]
    # re-run the same call shape with num_retries=3
    import inspect
    u = mock.url
    kw = {"num_retries": 3}
    fn = {"azure_chat": lambda: litellm.completion(model="azure/d", api_base=u, api_key="K", api_version="x", messages=[{"role": "user", "content": "hi"}], **kw),
          "openai_chat": lambda: litellm.completion(model="openai/d", api_base=u + "/v1", api_key="K", messages=[{"role": "user", "content": "hi"}], **kw),
          "openai_resp": lambda: litellm.responses(model="openai/d", api_base=u + "/v1", api_key="K", input="hi", **kw),
          "azure_resp": lambda: litellm.responses(model="azure/responses/d", api_base=u, api_key="K", api_version="x", input="hi", **kw),
          "claude": lambda: litellm.completion(model="anthropic/claude-sonnet-4-5", api_base=u, api_key="K", messages=[{"role": "user", "content": "hi"}], **kw)}[name]
    fn()
    assert len(mock.requests) == 3  # 429, 429, 200


def test_429_exhausted_raises_rate_limit_error(mock):
    mock.reset("429_then_ok")
    with pytest.raises(litellm.RateLimitError):
        litellm.completion(model="anthropic/claude-sonnet-4-5", api_base=mock.url, api_key="K", num_retries=1,
                           messages=[{"role": "user", "content": "hi"}])


def test_responses_stream_is_silently_faked_for_unregistered_model(mock):
    """RISK: unknown model name -> request sent WITHOUT stream=true, events replayed locally (no real token streaming)."""
    r = litellm.responses(model="openai/never-heard-of-it", api_base=mock.url + "/v1", api_key="K", input="hi", stream=True)
    list(r)
    assert type(r).__name__ == "MockResponsesAPIStreamingIterator"
    assert "stream" not in mock.requests[-1]["body"]


def test_chat_stream_is_native_for_unknown_model(mock):
    list(litellm.completion(model="openai/never-heard-of-it", api_base=mock.url + "/v1", api_key="K", stream=True,
                            messages=[{"role": "user", "content": "hi"}]))
    assert mock.requests[-1]["body"]["stream"] is True
