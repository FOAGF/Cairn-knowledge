"""D-69: DeepSeek as its own provider kind, and its web search run by Cairn through DeepSeek's Search and Fetch API
(function tools in Chat Completions; FR-LLM-AS-12)."""
import json

import pytest

from cairn_sidecar import ai, deepseek
from cairn_sidecar.errors import SidecarError
from cairn_sidecar.web import Evidence
from helpers import FakeReq, target, user
from test_routes import make, run

URLS = ["https://www.python.org/downloads/", "https://docs.python.org/3/whatsnew/3.15.html"]


def ds_run(mock, scenario="ok", stream=False, max_uses=3, **kw):
    mock.reset(scenario)
    t, _, _ = make(mock, "deepseek_chat")
    deltas = []
    r = run(t, stream=stream, on_delta=deltas.append, web_search={"max_uses": max_uses},
            messages=user("What is the latest Python release?"), **kw)
    return r, deltas


def chats(mock):
    return [q for q in mock.requests if q["path"] == "/chat/completions"]


def searches(mock):
    return [q for q in mock.requests if q["path"] == "/v1/tools/search"]


# ---------------------------------------------------------------- the provider kind
def test_default_address_is_deepseeks():
    _, kw = ai.build_call(ai.Target(target(provider_kind="deepseek", endpoint="")), user())
    assert kw["api_base"] == "https://api.deepseek.com" and kw["model"] == "openai/dep1"
    assert deepseek.base_url(ai.Target(target(provider_kind="deepseek", endpoint="https://api.deepseek.com/v1/"))) == "https://api.deepseek.com"


def test_models_are_listed_from_slash_models(mock):
    t, _, _ = make(mock, "deepseek_chat")
    out = ai.llm_list_models(FakeReq({"target": t}))
    assert [m["id"] for m in out["models"]] == ["model-a", "model-b"]
    req = mock.requests[-1]
    assert req["path"] == "/models" and req["headers"]["authorization"] == "Bearer SECRET"


# ---------------------------------------------------------------- the search loop
@pytest.mark.parametrize("stream", [False, True], ids=["nostream", "stream"])
def test_search_then_answer_with_the_cited_page(mock, stream):
    r, deltas = ds_run(mock, stream=stream)
    assert r["text"] == f"Python 3.15.0 is out: [Download Python]({URLS[0]})."
    assert r["web"] == {"searches": 1, "sources": [{"url": URLS[0], "title": "Download Python | Python.org", "cited_text": ""}]}
    assert r["finish_reason"] == "stop"
    assert r["usage"]["input_tokens"] == 10  # two rounds, added up
    assert ("".join(deltas) == r["text"]) if stream else deltas == []

    first, second = chats(mock)
    assert [t["function"]["name"] for t in first["body"]["tools"]] == ["web_search", "web_fetch"]
    assert first["body"]["tool_choice"] == "required", "the user asked for the web: the first round must search"
    assert second["body"]["tool_choice"] == "auto"
    (s,) = searches(mock)
    assert s["body"] == {"query": "latest python", "count": deepseek.RESULTS_PER_SEARCH,
                         "ori_question": "What is the latest Python release?"}
    assert s["headers"]["authorization"] == "Bearer SECRET"
    # The round with the tool call goes back as it was, then the results as the tool message.
    msgs = second["body"]["messages"]
    assistant, tool = msgs[-2], msgs[-1]
    assert assistant["tool_calls"][0]["function"] == {"name": "web_search", "arguments": json.dumps({"query": "latest python"})}
    assert assistant["reasoning_content"] == "I should search."
    assert tool["role"] == "tool" and tool["tool_call_id"] == assistant["tool_calls"][0]["id"]
    assert tool["content"].startswith('Results for "latest python" (2).')
    assert f"1. [Download Python | Python.org]({URLS[0]})\nSite: Python.org\nDate: 2026-10-01T00:00:00Z\n" in tool["content"]
    assert f"2. [What's new in Python 3.15]({URLS[1]})\nPython 3.15 is the latest (summary)" in tool["content"], "empty site and date lines go"


def test_the_limit_ends_the_searching(mock):
    r, _ = ds_run(mock, "ds_more", max_uses=2)
    assert len(searches(mock)) == 2
    assert r["web"]["searches"] == 2
    choices = [q["body"]["tool_choice"] for q in chats(mock)]
    assert choices == ["required", "auto", "none"]
    assert r["text"].startswith("Out of searches")


def test_a_page_read_counts_and_is_cited(mock):
    r, _ = ds_run(mock, "ds_fetch")
    fetches = [q for q in mock.requests if q["path"] == "/v1/tools/fetch"]
    assert [f["body"] for f in fetches] == [{"url": URLS[1]}]
    tool = chats(mock)[-1]["body"]["messages"][-1]["content"]
    assert tool.startswith(f'Page "What\'s new in Python 3.15" at {URLS[1]}.') and tool.endswith("x\n…")
    assert len(tool) < deepseek.FETCH_CHARS + 300
    assert r["web"]["searches"] == 2 and [s["url"] for s in r["web"]["sources"]] == [URLS[1]]


def test_an_answer_without_links_shows_the_pages_it_was_given(mock):
    r, _ = ds_run(mock, "ds_uncited")
    assert [s["url"] for s in r["web"]["sources"]] == URLS


def test_a_failed_search_goes_back_to_the_model(mock):
    r, _ = ds_run(mock, "ds_search_500")
    tool = chats(mock)[-1]["body"]["messages"][-1]["content"]
    assert tool.startswith("The search could not run:") and "search backend down" in tool
    assert r["web"] == {"searches": 0, "sources": []}, "no search ran, so it is not a web answer"


@pytest.mark.parametrize("scenario,status", [("ds_search_401", 401), ("ds_402", 402)])
def test_a_rejected_key_or_empty_balance_stops_the_call(mock, scenario, status):
    with pytest.raises(SidecarError) as e:
        ds_run(mock, scenario)
    assert e.value.status == status
    if status == 402:
        assert "balance" in e.value.message


def test_a_refused_required_tool_choice_falls_back_to_auto(mock):
    r, _ = ds_run(mock, "ds_no_required")
    choices = [q["body"]["tool_choice"] for q in chats(mock)]
    assert choices == ["required", "auto", "auto"]
    assert r["web"]["searches"] == 1


def test_without_web_search_no_tools_are_sent(mock):
    mock.reset()
    t, _, _ = make(mock, "deepseek_chat")
    r = run(t)
    body = chats(mock)[-1]["body"]
    assert "tools" not in body and "web" not in r and r["text"] == "Hello from mock"


def test_cancel_between_rounds(mock):
    import threading

    class StopAfterSearch(threading.Event):
        """Reads as set once the first search has been made."""
        def is_set(self):
            return bool(searches(mock))

    mock.reset()
    t, _, _ = make(mock, "deepseek_chat")
    r = ai.run_completion(ai.Target(t), user("q"), web_search={"max_uses": 3}, cancelled=StopAfterSearch(), evidence=Evidence())
    assert r["finish_reason"] == "cancelled"
    assert len(chats(mock)) == 1, "no second round after the cancel"


@pytest.mark.parametrize("second_tool", ["web_search", "web_fetch"])
def test_cancel_stops_remaining_tools_in_the_same_round(mock, monkeypatch, second_tool):
    import threading

    class StopAfterSearch(threading.Event):
        def is_set(self):
            return bool(searches(mock))

    def model_round(*args):
        calls = [
            {"id": "first", "name": "web_search", "arguments": json.dumps({"query": "first"})},
            {"id": "second", "name": second_tool,
             "arguments": json.dumps({"query": "second", "url": URLS[0]})},
        ]
        return "", calls, None, None, "tool_calls", "test-model", False

    # Control only the provider response; the real loop and HTTP tools run against the local server.
    monkeypatch.setattr(deepseek, "_round", model_round)
    mock.reset()
    t, _, _ = make(mock, "deepseek_chat")
    result = run(t, web_search={"max_uses": 3}, cancelled=StopAfterSearch(), evidence=Evidence())
    requests = [q for q in mock.requests if q["path"] in (deepseek.SEARCH_PATH, deepseek.FETCH_PATH)]
    assert len(requests) == 1, "Stop must prevent dispatching another tool in the same model response"
    assert result["finish_reason"] == "cancelled"
