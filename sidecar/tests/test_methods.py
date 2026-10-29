"""llm.test, llm.list_models, azure.check_signin, llm.complete (called directly)."""
import base64
import json
import sys
import time
import types

import pytest

from cairn_sidecar import ai
from cairn_sidecar.errors import SidecarError, from_exception
from helpers import FakeReq, target, user


def names(res):
    return [(c["name"], c["ok"]) for c in res["checks"]]


def test_llm_test_all_checks_pass(mock):
    t = target(endpoint=mock.url + "/v1")
    res = ai.llm_test(FakeReq({"target": t}))
    assert names(res) == [("basic", True), ("stream", True), ("effort:low", True), ("effort:medium", True),
                          ("effort:high", True), ("image", True), ("web", None)]
    assert "replied in" in res["checks"][0]["detail"]
    assert "2 text chunks" in res["checks"][1]["detail"]
    assert res["checks"][5]["detail"] == "It is red"
    # basic sends no effort; each effort check sends its level
    bodies = [r["body"] for r in mock.requests]
    assert "reasoning_effort" not in bodies[0] and "reasoning_effort" not in bodies[1]
    assert [b.get("reasoning_effort") for b in bodies[2:5]] == ["low", "medium", "high"]


def test_llm_test_skip_rules(mock):
    t = target(endpoint=mock.url + "/v1", vision=False, effort={"enabled": False})
    res = ai.llm_test(FakeReq({"target": t}))
    assert names(res) == [("basic", True), ("stream", True), ("effort:low", None), ("effort:medium", None),
                          ("effort:high", None), ("image", None), ("web", None)]
    assert "effort is off" in res["checks"][2]["detail"]
    assert "vision" in res["checks"][5]["detail"]


def test_llm_test_custom_levels_responses_and_claude(mock):
    t = target(provider_kind="openai_compatible", endpoint=mock.url + "/v1", format="responses",
               effort={"levels": ["minimal", "high"]})
    res = ai.llm_test(FakeReq({"target": t}))
    assert [c["name"] for c in res["checks"]][2:4] == ["effort:minimal", "effort:high"]
    assert all(c["ok"] for c in res["checks"] if c["name"] != "web")
    t = target(provider_kind="azure_foundry", endpoint=mock.url + "/anthropic", format="anthropic_messages",
               model="my-sonnet", effort={"custom": {"low": "1500"}})
    res = ai.llm_test(FakeReq({"target": t}))
    assert all(c["ok"] for c in res["checks"] if c["name"] != "web"), res


@pytest.mark.parametrize("scenario,detail", [("401", "rejected"), ("403", "not allowed")])
def test_llm_test_401_skips_the_rest_403_does_not_stop(scenario, detail, mock):
    mock.reset(scenario)
    t = target(endpoint=mock.url + "/v1")
    res = ai.llm_test(FakeReq({"target": t}))
    assert res["checks"][0]["ok"] is False and detail in res["checks"][0]["detail"]
    if scenario == "401":
        assert all(c["ok"] is None and c["detail"] == "skipped: basic call failed" for c in res["checks"][1:])
        assert len(mock.requests) == 1
    else:  # forbidden does not make the other checks pointless
        assert all(c["ok"] is False for c in res["checks"] if c["name"] != "web")
    assert "SECRET" not in json.dumps(res)


def test_llm_test_effort_rejected_is_reported_but_continues(mock, monkeypatch):
    real = ai.run_completion

    def fake(t, messages, level=None, **kw):
        if level == "medium":
            e = Exception("model refuses reasoning_effort")
            e.status_code = 400
            raise e
        return real(t, messages, level=level, **kw)
    monkeypatch.setattr(ai, "run_completion", fake)
    res = ai.llm_test(FakeReq({"target": target(endpoint=mock.url + "/v1")}))
    assert names(res)[2:5] == [("effort:low", True), ("effort:medium", False), ("effort:high", True)]
    assert "refused" in res["checks"][3]["detail"] and "[" in res["checks"][3]["detail"]  # message + hint
    assert res["checks"][5]["ok"] is True


def test_llm_test_network_failure_skips_rest():
    res = ai.llm_test(FakeReq({"target": target(endpoint="http://127.0.0.1:9/v1")}))
    assert res["checks"][0]["ok"] is False
    assert all(c["ok"] is None for c in res["checks"][1:])


def test_llm_test_single_chunk_stream_is_flagged(mock, monkeypatch):
    real = ai.run_completion

    def one_chunk(t, messages, level=None, stream=False, on_delta=None, **kw):
        if stream:
            on_delta("everything at once")
            return {"text": "x", "model": "m", "usage": None}
        return real(t, messages, level=level, stream=stream, on_delta=on_delta, **kw)
    monkeypatch.setattr(ai, "run_completion", one_chunk)
    res = ai.llm_test(FakeReq({"target": target(endpoint=mock.url + "/v1")}))
    assert res["checks"][1]["ok"] is False and "probably not real" in res["checks"][1]["detail"]


# ---- llm.complete
def test_llm_complete_stream_notifications_and_result(mock):
    r = FakeReq({"target": target(endpoint=mock.url + "/v1"), "messages": user(), "stream": True, "effort": "high"})
    res = ai.llm_complete(r)
    assert r.streamed == ["Hello ", "from mock"]
    assert res["text"] == "Hello from mock" and res["model"] == "dep1"
    assert mock.requests[-1]["body"]["reasoning_effort"] == "high"


def test_llm_complete_no_stream_sends_no_notifications(mock):
    r = FakeReq({"target": target(endpoint=mock.url + "/v1"), "messages": user()})
    assert ai.llm_complete(r)["text"] == "Hello from mock" and r.streamed == []


def test_llm_complete_effort_ignored_when_disabled(mock):
    r = FakeReq({"target": target(endpoint=mock.url + "/v1", effort={"enabled": False}),
                 "messages": user(), "effort": "high"})
    ai.llm_complete(r)
    assert "reasoning_effort" not in mock.requests[-1]["body"]


# ---- list_models
def test_list_models_openai_compatible(mock):
    for endpoint in (mock.url + "/v1", mock.url + "/v1/", mock.url):
        res = ai.llm_list_models(FakeReq({"target": target(endpoint=endpoint, model="")}))
        assert [m["id"] for m in res["models"]] == ["model-a", "model-b"]
        assert mock.requests[-1]["path"] == "/v1/models"
        assert mock.requests[-1]["headers"]["authorization"] == "Bearer SECRET"


def test_list_models_local_without_key_sends_no_auth(mock):
    ai.llm_list_models(FakeReq({"target": target(provider_kind="local", endpoint=mock.url + "/v1", auth="none", api_key=None)}))
    assert "authorization" not in mock.requests[-1]["headers"]


def test_list_models_anthropic(mock):
    t = target(provider_kind="anthropic", endpoint=mock.url, format="anthropic_messages", model="")
    res = ai.llm_list_models(FakeReq({"target": t}))
    assert res["models"][0] == {"id": "model-a", "label": "Model A"}
    assert res["models"][1]["label"] is None
    req = mock.requests[-1]
    assert req["path"] == "/v1/models" and req["headers"]["x-api-key"] == "SECRET"


def test_list_models_azure_is_unsupported():
    with pytest.raises(SidecarError) as ei:
        ai.llm_list_models(FakeReq({"target": target(provider_kind="azure_foundry", endpoint="https://x.example/openai/v1")}))
    assert ei.value.kind == "unsupported" and ei.value.data()["kind"] == "unsupported"


def test_list_models_http_errors_map_to_kinds(mock):
    mock.reset("401")
    with pytest.raises(SidecarError) as ei:
        ai.llm_list_models(FakeReq({"target": target(endpoint=mock.url + "/v1")}))
    assert ei.value.kind == "auth" and ei.value.status == 401


# ---- azure.check_signin (azure-identity is replaced by a fake: no real sign-in is touched)
def jwt(claims):
    b = lambda d: base64.urlsafe_b64encode(json.dumps(d).encode()).decode().rstrip("=")
    return f"{b({'alg': 'none'})}.{b(claims)}.sig"


def fake_identity(monkeypatch, cred):
    mod = types.ModuleType("azure.identity")
    mod.AzureCliCredential = cred
    monkeypatch.setitem(sys.modules, "azure.identity", mod)


def test_check_signin_ok(monkeypatch):
    class Cred:
        def get_token(self, scope):
            assert scope == "https://cognitiveservices.azure.com/.default"
            return types.SimpleNamespace(token=jwt({"upn": "me@example.com"}), expires_on=time.time() + 3600)
    fake_identity(monkeypatch, Cred)
    res = ai.azure_check_signin(FakeReq({}))
    assert res["signed_in"] is True and res["account"] == "me@example.com"
    assert res["detail"].startswith("token valid until ")


def test_check_signin_failure_is_a_result_not_an_error(monkeypatch):
    class Cred:
        def get_token(self, scope):
            raise RuntimeError("Please run 'az login' to set up an account")
    fake_identity(monkeypatch, Cred)
    res = ai.azure_check_signin(FakeReq({}))
    assert res["signed_in"] is False and "az login" in res["detail"]


def test_signin_exception_maps_to_signin_kind():
    class CredentialUnavailableError(Exception):
        pass
    err = from_exception(CredentialUnavailableError("nothing"))
    assert err.kind == "signin" and "az login" in err.data()["hint"]
