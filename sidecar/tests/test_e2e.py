"""End to end over a real subprocess: `python -I -m cairn_sidecar --role ai`."""
import os
import subprocess
import sys
import time

import pytest

from conftest import clean_env
from helpers import Client, target, user


@pytest.fixture
def sidecar():
    c = Client("ai")
    yield c
    c.close()


def params(mock, **over):
    return {"target": target(endpoint=mock.url + "/v1"), "messages": user(), **over}


def test_ping_is_fast_and_lazy(sidecar):
    t0 = time.time()
    res = sidecar.call("ping")
    took = time.time() - t0
    assert res["result"]["pong"] is True and res["result"]["role"] == "ai"
    assert res["result"]["python"].startswith("3.") and res["result"]["version"]
    assert took < 2.0, f"ping took {took:.2f} s"
    print(f"\n[info] first ping {took * 1000:.0f} ms after start")


def test_nothing_heavy_is_imported_at_start():
    """-X importtime lists every import on stderr: none of the heavy libraries may appear after ping."""
    p = subprocess.Popen([sys.executable, "-I", "-X", "importtime", "-m", "cairn_sidecar", "--role", "ai"],
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                         env=clean_env())
    out, err = p.communicate('{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}\n', timeout=20)
    assert '"pong":true' in out and p.returncode == 0
    imported = {line.rsplit("|", 1)[-1].strip().split(".")[0] for line in err.splitlines() if "|" in line}
    for heavy in ("litellm", "onnxruntime", "numpy", "tokenizers", "azure", "openai", "httpx", "pydantic",
                  "pypdfium2", "docx", "PIL", "lxml"):
        assert heavy not in imported, f"{heavy} was imported at start"
    assert "json" in imported


def test_embed_role_does_not_import_litellm_or_onnx_at_start():
    c = Client("embed")
    try:
        assert c.call("ping")["result"]["role"] == "embed"
    finally:
        c.close()


def test_streamed_complete_notifications_come_before_result(sidecar, mock):
    rid = sidecar.send("llm.complete", params(mock, stream=True))
    notes, res = sidecar.wait_for(rid)
    assert [n["params"]["delta"] for n in notes] == ["Hello ", "from mock"]
    assert all(n["method"] == "stream" and n["params"]["id"] == rid and "id" not in n for n in notes)
    assert res["result"]["text"] == "Hello from mock"


def test_complete_without_stream(sidecar, mock):
    res = sidecar.call("llm.complete", params(mock))
    assert res["result"] == {"text": "Hello from mock", "model": "dep1", "usage": {"input_tokens": 5, "output_tokens": 3},
                             "finish_reason": "stop"}


def test_error_has_kind_hint_and_no_key(sidecar, mock):
    mock.reset("401")
    res = sidecar.call("llm.complete", params(mock))
    e = res["error"]
    assert e["code"] == -32000 and e["data"]["kind"] == "auth" and e["data"]["status"] == 401 and e["data"]["hint"]
    assert "SECRET" not in str(res) and not any("SECRET" in line for line in sidecar.stderr)


def test_llm_test_over_the_wire(sidecar, mock):
    res = sidecar.call("llm.test", {"target": target(endpoint=mock.url + "/v1")})
    assert [c["ok"] for c in res["result"]["checks"]] == [True] * 6 + [None]


def test_stdout_stays_clean_when_a_library_prints(sidecar, mock):
    # LiteLLM prints "Provider List" for unknown models; that must not reach the protocol stream.
    res = sidecar.call("llm.complete", params(mock) | {"target": target(endpoint=mock.url + "/v1", model="unknown-model-xyz")})
    assert "result" in res
    assert sidecar.msgs.empty()


def test_protocol_errors_and_survival(sidecar):
    sidecar.send_raw("this is not json")
    assert sidecar.next()["error"]["code"] == -32700
    assert sidecar.call("no.such.method")["error"]["code"] == -32601
    sidecar.send_raw('{"jsonrpc":"2.0","id":99,"method":"llm.complete","params":[]}')
    assert sidecar.next()["error"]["code"] == -32602
    res = sidecar.call("llm.complete", {"target": {"provider_kind": "bogus"}, "messages": user()})
    assert res["error"]["code"] == -32602
    assert sidecar.call("ping")["result"]["pong"]


def test_concurrent_requests(sidecar, new_mock):
    slow, fast = new_mock(), new_mock()
    slow.reset("slow")
    a = sidecar.send("llm.complete", params(slow, stream=True))
    b = sidecar.send("llm.complete", params(fast))
    c = sidecar.send("llm.test", {"target": target(endpoint=fast.url + "/v1")})
    done = {}
    t0 = time.time()
    while len(done) < 2 and time.time() - t0 < 20:
        m = sidecar.next()
        if "id" in m:
            done[m["id"]] = m
    assert set(done) == {b, c}, "the slow stream must not block the others"
    assert done[b]["result"]["text"] == "Hello from mock"
    assert a not in done
    sidecar.send("cancel", {"id": a}, notify=True)
    sidecar.wait_for(a)


def test_cancel_stops_a_stream_promptly(sidecar, new_mock):
    slow = new_mock()
    slow.reset("slow")
    rid = sidecar.send("llm.complete", params(slow, stream=True))
    first = sidecar.next()
    assert first["method"] == "stream" and first["params"]["id"] == rid
    t0 = time.time()
    sidecar.send("cancel", {"id": rid}, notify=True)
    notes, res = sidecar.wait_for(rid, timeout=5)
    assert time.time() - t0 < 1.5
    assert res["result"]["cancelled"] is True and res["result"]["finish_reason"] == "cancelled" and res["result"]["text"].startswith("w0 ")
    time.sleep(0.7)  # the slow server keeps sending: nothing more may arrive
    assert sidecar.msgs.empty()
    assert sidecar.call("ping")["result"]["pong"]


def test_cancel_of_unknown_id_is_ignored(sidecar):
    sidecar.send("cancel", {"id": 12345}, notify=True)
    assert sidecar.call("ping")["result"]["pong"]


def test_shutdown_answers_then_exits_0(sidecar):
    res = sidecar.call("shutdown")
    assert res["result"] == {}
    assert sidecar.proc.wait(5) == 0


def test_exit_0_when_stdin_closes(sidecar):
    sidecar.call("ping")
    sidecar.proc.stdin.close()
    assert sidecar.proc.wait(5) == 0
