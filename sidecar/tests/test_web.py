"""M6: web search (FR-LLM-AS-12). Request shapes per route, the evidence read back, refusals, cancel, llm.test."""
import json

import pytest

from cairn_sidecar import ai
from cairn_sidecar.errors import InvalidParams, from_exception
from cairn_sidecar.web import Evidence, parse_web_search
from helpers import Client, FakeReq, target, user
from test_routes import NAMES as ALL_NAMES, cases, make, run

# DeepSeek's web search is Cairn's own (test_deepseek.py), not a provider option.
NAMES = [n for n in ALL_NAMES if not n.startswith("deepseek")]

CLAUDE = ["foundry_claude", "foundry_claude_other_name", "anthropic"]
RESPONSES = ["foundry_v1_resp", "azure_resp", "openai_compat_resp"]
CHAT = [n for n in NAMES if n not in CLAUDE and n not in RESPONSES]
URLS = ["https://www.python.org/downloads/", "https://docs.python.org/3/whatsnew/3.15.html"]
SEARCH = {"max_uses": 2}


def web_run(mock, name, scenario="web", stream=False, **kw):
    mock.reset(scenario)
    t, _, _ = make(mock, name)
    deltas = []
    r = run(t, stream=stream, on_delta=deltas.append, web_search=SEARCH, **kw)
    return r, deltas


# ---------------------------------------------------------------- request shapes
@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("name", NAMES)
def test_request_shape_per_format(name, stream, mock):
    web_run(mock, name, scenario="ok", stream=stream)
    body = mock.requests[-1]["body"]
    if name in CLAUDE:
        # LiteLLM passes the hosted-tool dict through unchanged, max_uses included
        assert body["tools"] == [{"type": "web_search_20250305", "name": "web_search", "max_uses": 2}]
        assert "web_search_options" not in body
    elif name in RESPONSES:
        assert body["tools"] == [{"type": "web_search"}]
    else:
        assert body["web_search_options"] == {"search_context_size": "medium"}
        assert "tools" not in body


@pytest.mark.parametrize("name", NAMES)
def test_no_web_search_sends_nothing_and_no_web_result(name, mock):
    t, _, _ = make(mock, name)
    r = run(t)
    body = mock.requests[-1]["body"]
    assert "tools" not in body and "web_search_options" not in body
    assert "web" not in r


# ---------------------------------------------------------------- evidence
@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("name", NAMES)
def test_evidence_from_each_format(name, stream, mock):
    r, deltas = web_run(mock, name, stream=stream)
    assert r["web"]["searches"] == 1
    assert [s["url"] for s in r["web"]["sources"]] == URLS  # de-duplicated, first-cited order
    first = r["web"]["sources"][0]
    assert first["title"] == "Download Python | Python.org"
    assert first["cited_text"] == ("Latest: Python 3.15.0" if name in CLAUDE else "")
    assert r["finish_reason"] == "stop"
    assert "Python 3.15.0" in r["text"]
    if stream:
        assert "".join(deltas) == r["text"]
    for leak in ("srvtoolu", "web_search", "query", "encrypted"):
        assert leak not in r["text"] and not any(leak in d for d in deltas)


def test_claude_streamed_text_keeps_both_sides_of_the_search(mock):
    r, deltas = web_run(mock, "foundry_claude", stream=True)
    assert r["text"] == "I'll search. Python 3.15.0 is the latest."
    assert deltas[0] == "I'll search. "
    assert r["usage"] is not None and r["usage"]["output_tokens"] == 3


def test_claude_plain_text_has_no_server_blocks(mock):
    r, _ = web_run(mock, "anthropic")
    assert r["text"] == "I'll search. Python 3.15.0 is the latest. It has new features."


@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
@pytest.mark.parametrize("name", NAMES)
def test_provider_that_ignores_web_search_gives_zero_searches(name, stream, mock):
    r, _ = web_run(mock, name, scenario="ok", stream=stream)
    assert r["web"] == {"searches": 0, "sources": []}
    assert r["text"] == "Hello from mock"


# ---- the recorded shapes, without a server
def test_evidence_reads_recorded_foundry_shapes():
    ev = Evidence()
    # non-streamed message (poc/i-assistant/out/foundry-claude-entra/web-plain.json): citations are lists of lists
    ev.read_message({"tool_calls": [{"id": "srvtoolu_01R", "type": "function", "function": {"name": "web_search"}}],
                     "provider_specific_fields": {
                         "citations": [[{"type": "web_search_result_location", "url": "https://a.org/1", "title": "A",
                                         "cited_text": "text a", "encrypted_index": "E"}],
                                       [{"type": "web_search_result_location", "url": "https://b.org/2", "title": "B",
                                         "cited_text": "text b"}],
                                       [{"type": "web_search_result_location", "url": "https://a.org/1", "title": "A",
                                         "cited_text": "again"}]],
                         "web_search_results": [{"type": "web_search_tool_result", "content": [
                             {"type": "web_search_result", "url": "https://never-cited.org", "title": "N"}]}]}})
    ev.add_usage({"server_tool_use": {"web_search_requests": 1, "tool_search_requests": None}})
    out = ev.result(True)
    assert out["searches"] == 1
    assert out["sources"] == [{"url": "https://a.org/1", "title": "A", "cited_text": "text a"},
                              {"url": "https://b.org/2", "title": "B", "cited_text": "text b"}]  # not the uncited result


def test_evidence_streamed_chunks_and_search_count_fallback():
    ev = Evidence()
    ev.read_delta({"provider_specific_fields": {"web_search_calls": {"srvtoolu_1": {"status": "in_progress"}}},
                   "tool_calls": [{"id": "srvtoolu_1", "function": {"name": "web_search", "arguments": ""}}]})
    ev.read_delta({"tool_calls": [{"id": None, "function": {"arguments": "{\"query"}}]})
    ev.read_delta({"provider_specific_fields": {"citation": {"type": "web_search_result_location", "url": "https://a.org",
                                                              "title": "A", "cited_text": "t"}}})
    ev.read_delta({"content": "text"})
    # no usage from the provider: the calls seen are counted
    assert ev.result(True) == {"searches": 1, "sources": [{"url": "https://a.org", "title": "A", "cited_text": "t"}]}
    ev.add_usage({"server_tool_use": {"web_search_requests": 2}})
    assert ev.result(True)["searches"] == 2


def test_evidence_usage_mirror_and_client_tool_calls_do_not_count():
    ev = Evidence()
    ev.read_delta({"tool_calls": [{"id": "call_1", "function": {"name": "get_weather"}}]})
    assert ev.result(True) == {"searches": 0, "sources": []}
    ev.add_usage({"prompt_tokens_details": {"web_search_requests": 3}})
    assert ev.result(True)["searches"] == 3


def test_evidence_chat_url_citation_counts_as_one_search():
    ev = Evidence()
    ev.read_message({"annotations": [{"type": "url_citation", "url_citation": {"url": "https://a.org", "title": "A"}}]})
    assert ev.result(False)["searches"] == 1 and ev.result(False)["sources"][0]["cited_text"] == ""
    assert Evidence().result(False) == {"searches": 0, "sources": []}


def test_evidence_responses_output_items():
    ev = Evidence()
    ev.read_responses({"output": [{"type": "web_search_call", "id": "ws_1"},
                                  {"type": "web_search_call", "id": "ws_2"},
                                  {"type": "message", "content": [{"type": "output_text", "text": "x", "annotations": [
                                      {"type": "url_citation", "url": "https://a.org", "title": "A"},
                                      {"type": "file_citation", "file_id": "f"}]}]}]})
    ev.read_responses({"output": [{"type": "web_search_call", "id": "ws_1"}]})  # seen again (stream item event)
    assert ev.result() == {"searches": 2, "sources": [{"url": "https://a.org", "title": "A", "cited_text": ""}]}


# ---------------------------------------------------------------- the param
@pytest.mark.parametrize("bad", ["yes", 3, {"max_uses": 0}, {"max_uses": "5"}, {"max_uses": True}, {"max_uses": 1.5}])
def test_web_search_param_is_checked(bad):
    with pytest.raises(InvalidParams):
        parse_web_search(bad)


def test_web_search_param_values():
    assert parse_web_search(None) is None and parse_web_search(False) is None
    assert parse_web_search({"max_uses": 3}) == {"max_uses": 3}
    assert parse_web_search({}) == {"max_uses": 5}


def test_llm_complete_through_the_method(mock):
    t = make(mock, "foundry_claude")[0]
    mock.reset("web")
    r = FakeReq({"target": t, "messages": user(), "stream": True, "web_search": {"max_uses": 4}})
    res = ai.llm_complete(r)
    assert mock.requests[-1]["body"]["tools"][0]["max_uses"] == 4
    assert res["web"]["searches"] == 1 and len(res["web"]["sources"]) == 2
    assert "".join(r.streamed) == res["text"]
    res = ai.llm_complete(FakeReq({"target": t, "messages": user()}))
    assert "web" not in res


# ---------------------------------------------------------------- refusals
@pytest.mark.parametrize("name", NAMES)
def test_refusal_maps_to_web_search_unsupported_and_is_not_retried(name, mock, monkeypatch):
    monkeypatch.setattr(ai, "NUM_RETRIES", 3)
    with pytest.raises(Exception) as ei:
        web_run(mock, name, scenario="web_refused")
    err = from_exception(ei.value, ("SECRET",))
    assert err.kind == "web_search_unsupported" and err.status == 400
    assert "web" in err.message.lower() and "SECRET" not in err.message
    assert err.data()["hint"]
    assert len(mock.requests) == 1


def test_refusal_message_has_the_providers_reason(mock):
    with pytest.raises(Exception) as ei:
        web_run(mock, "anthropic", scenario="web_refused")
    msg = from_exception(ei.value).message
    assert "web_search_20250305" in msg and "litellm" not in msg.lower()
    with pytest.raises(Exception) as ei:
        web_run(mock, "openai_compat_chat", scenario="web_refused")
    assert "not enabled" in from_exception(ei.value).message


def _status(code, text, name="ProviderError"):
    e = type(name, (Exception,), {})(text)
    e.status_code = code
    return e


@pytest.mark.parametrize("text", [
    "tools.0: Input tag 'web_search_20250305' found using 'type' does not match any of the expected tags",
    "Web search is not enabled for this organization",
    "openai does not support parameters: ['web_search_options'], for model gpt-4o",
    "Invalid value: 'web_search_preview'. Supported values are: 'function'",
    "web-search tool not supported",
])
def test_refusals_that_name_web_search(text):
    assert from_exception(_status(400, text)).kind == "web_search_unsupported"
    assert from_exception(_status(422, text, "UnprocessableEntityError")).kind == "web_search_unsupported"


@pytest.mark.parametrize("code,text,kind", [
    (400, "Unsupported value: reasoning_effort does not support 'minimal'", "bad_request"),
    (400, "tools.0.custom.input_schema: Field required", "bad_request"),
    (400, "Invalid 'messages': the search index could not be read", "bad_request"),
    (400, "prompt is too long: 300000 tokens > 200000 maximum", "too_long"),
    (401, "web search key rejected", "auth"),
    (403, "web search is not allowed for this key", "forbidden"),
    (404, "web_search deployment not found", "not_found"),
    (429, "web search rate limited", "rate_limit"),
    (500, "web search backend failed", "internal"),
])
def test_other_failures_keep_their_kind(code, text, kind):
    assert from_exception(_status(code, text)).kind == kind


def test_normal_400_stays_bad_request_with_web_search_on(mock):
    mock.reset("too_long")
    t, _, _ = make(mock, "openai_compat_chat")
    with pytest.raises(Exception) as ei:
        run(t, web_search=SEARCH)
    assert from_exception(ei.value).kind == "too_long"


# ---------------------------------------------------------------- cancel mid-search
def test_cancel_during_a_search_answers_with_the_text_so_far(new_mock):
    slow = new_mock()
    slow.reset("web_slow")
    c = Client("ai")
    try:
        t = target(provider_kind="anthropic", endpoint=slow.url, format="anthropic_messages", model="claude-sonnet-4-5")
        rid = c.send("llm.complete", {"target": t, "messages": user(), "stream": True, "web_search": SEARCH})
        first = c.next()
        assert first["method"] == "stream" and first["params"]["delta"] == "I'll search. "
        c.send("cancel", {"id": rid}, notify=True)
        _, res = c.wait_for(rid, timeout=5)
        assert res["result"]["cancelled"] is True and res["result"]["finish_reason"] == "cancelled"
        assert res["result"]["text"] == "I'll search. "
        assert res["result"]["web"]["sources"] == []
        assert c.call("ping")["result"]["pong"]
    finally:
        c.close()


# ---------------------------------------------------------------- llm.test
def web_check(res):
    return next(c for c in res["checks"] if c["name"] == "web")


@pytest.mark.parametrize("name", ["foundry_claude", "openai_compat_resp", "openai_compat_chat"])
def test_llm_test_web_check_passes(name, mock):
    mock.reset("web")
    t = make(mock, name, web_search=True)[0]
    check = web_check(ai.llm_test(FakeReq({"target": t})))
    assert check["ok"] is True and "1 search" in check["detail"] and "2 sources" in check["detail"]
    body = mock.requests[-1]["body"]
    assert "today's date" in json.dumps(body)
    if name == "foundry_claude":
        assert body["tools"][0]["max_uses"] == 1


def test_llm_test_web_check_no_search(mock):
    t = make(mock, "openai_compat_chat", web_search=True)[0]
    check = web_check(ai.llm_test(FakeReq({"target": t})))
    assert check["ok"] is False and check["detail"].startswith("no search was made")


def test_llm_test_web_check_refused(mock):
    mock.reset("web_refused")
    t = make(mock, "foundry_claude", web_search=True)[0]
    res = ai.llm_test(FakeReq({"target": t}))
    check = web_check(res)
    assert check["ok"] is False and "refused web search" in check["detail"] and "web_search_20250305" in check["detail"]
    assert [c["ok"] for c in res["checks"][:2]] == [True, True]  # the other checks ran as before


def test_llm_test_web_check_skipped_unless_ticked(mock):
    t = make(mock, "openai_compat_chat")[0]
    check = web_check(ai.llm_test(FakeReq({"target": t})))
    assert check["ok"] is None and "off" in check["detail"]
    assert all("tools" not in r["body"] and "web_search_options" not in r["body"] for r in mock.requests)


def test_llm_test_web_check_skipped_when_basic_fails():
    t = target(endpoint="http://127.0.0.1:9/v1", web_search=True)
    res = ai.llm_test(FakeReq({"target": t}))
    assert web_check(res) == {"name": "web", "ok": None, "detail": "skipped: basic call failed"}
