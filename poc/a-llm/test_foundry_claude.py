"""Claude models hosted on Azure AI Foundry (Anthropic Messages format), API key and Entra ID. Localhost mock only."""
import pytest, litellm
from test_providers import PNG, EFFORT_KEYS, fake_provider, TOKEN_CALLS

DEP = "claude-sonnet-4-5"   # LiteLLM routes azure_ai/* to the Anthropic handler only if the name contains "claude"


def call(mock, auth, stream=False, effort=None, image=False, model=f"azure_ai/{DEP}"):
    kw = dict(model=model, api_base=mock.url + "/anthropic", num_retries=0)
    if auth == "entra": kw["azure_ad_token_provider"] = fake_provider
    else: kw["api_key"] = "SECRET"
    content = [{"type": "text", "text": "hi"}] + ([{"type": "image_url", "image_url": {"url": PNG}}] if image else [])
    kw["messages"] = [{"role": "user", "content": content}]
    if effort: kw["reasoning_effort"] = effort
    r = litellm.completion(stream=stream, **kw)
    if stream: return "".join((c.choices[0].delta.content or "") for c in r)
    return r.choices[0].message.content


@pytest.mark.parametrize("auth", ["key", "entra"])
@pytest.mark.parametrize("stream", [False, True])
def test_path_auth_and_no_effort(auth, stream, mock):
    assert call(mock, auth, stream=stream)
    req = mock.requests[-1]; h = req["headers"]
    assert req["path"] == "/anthropic/v1/messages"
    assert "anthropic-version" in h
    if auth == "entra":
        assert h["authorization"].startswith("Bearer fake-entra-token-") and "api-key" not in h and "x-api-key" not in h
    else:
        assert h["api-key"] == "SECRET" and "authorization" not in h
    assert not any(k in req["body"] for k in EFFORT_KEYS)


def test_entra_token_fetched_per_request(mock):
    before = len(TOKEN_CALLS)
    call(mock, "entra"); call(mock, "entra")
    assert len(TOKEN_CALLS) - before >= 2


@pytest.mark.parametrize("effort", ["low", "medium", "high"])
def test_effort_maps_to_claude_thinking(effort, mock):
    call(mock, "entra", effort=effort)
    body = mock.requests[-1]["body"]
    assert "thinking" in body or "output_config" in body, body.keys()
    assert "reasoning_effort" not in body


def test_image_input(mock):
    call(mock, "entra", image=True)
    content = mock.requests[-1]["body"]["messages"][0]["content"]
    assert any(c.get("type") == "image" for c in content)


def test_deployment_name_without_claude_is_not_routed_to_messages(mock):
    """Risk: a custom deployment name without 'claude' goes to the OpenAI-style chat route instead."""
    try: call(mock, "entra", model="azure_ai/my-sonnet")
    except Exception: pass
    assert mock.requests and mock.requests[-1]["path"] != "/anthropic/v1/messages"
