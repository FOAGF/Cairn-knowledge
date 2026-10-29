"""Role `embed`, with the real EmbeddingGemma files when present (see scripts/setup-sidecar-dev.sh)."""
import base64
import math
import os
import struct
import time

import pytest

from conftest import HERE
from helpers import Client

MODEL_DIR = os.path.join(os.path.dirname(HERE), "models", "embeddinggemma-300m")
HAVE_MODEL = all(os.path.isfile(os.path.join(MODEL_DIR, f)) for f in
                 ("onnx/model_q4.onnx", "onnx/model_q4.onnx_data", "tokenizer.json"))
needs_model = pytest.mark.skipif(not HAVE_MODEL, reason="embedding model files not found (run scripts/setup-sidecar-dev.sh)")

P = {"model_dir": MODEL_DIR, "dims": 256, "threads": 2}


def vec(b64):
    raw = base64.b64decode(b64)
    return list(struct.unpack(f"<{len(raw) // 4}f", raw))


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


@pytest.fixture(scope="module")
def embed():
    c = Client("embed")
    yield c
    c.close()


def test_model_missing_error_names_the_path(tmp_path):
    c = Client("embed")
    try:
        res = c.call("embed.query", {"model_dir": str(tmp_path), "dims": 256, "threads": 1, "text": "x"})
        e = res["error"]
        assert e["data"]["kind"] == "model_missing"
        assert str(tmp_path) in e["message"]
        assert c.call("ping")["result"]["pong"]
    finally:
        c.close()


def test_bad_params_are_invalid_params(tmp_path):
    c = Client("embed")
    try:
        assert c.call("embed.query", {"dims": 256, "text": "x"})["error"]["code"] == -32602
        assert c.call("embed.query", {"model_dir": str(tmp_path), "dims": 99999, "text": "x"})["error"]["code"] == -32602
    finally:
        c.close()


@needs_model
def test_runs_at_low_priority(embed):
    pid = embed.call("ping")["result"]["pid"]
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel32.OpenProcess.restype = wintypes.HANDLE
        kernel32.GetPriorityClass.argtypes = [wintypes.HANDLE]
        kernel32.GetPriorityClass.restype = wintypes.DWORD
        kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
        handle = kernel32.OpenProcess(0x1000, False, pid)  # PROCESS_QUERY_LIMITED_INFORMATION
        assert handle, ctypes.get_last_error()
        try:
            assert kernel32.GetPriorityClass(handle) == 0x4000  # BELOW_NORMAL_PRIORITY_CLASS
        finally:
            kernel32.CloseHandle(handle)
    else:
        with open(f"/proc/{pid}/stat") as f:
            nice = int(f.read().rsplit(")", 1)[1].split()[16])
        assert nice >= 10


@needs_model
def test_info_loads_the_model(embed):
    res = embed.call("embed.info", P, timeout=120)["result"]
    assert res["loaded"] is True and res["dims"] == 256 and res["model"] == "embeddinggemma-300m-q4"
    assert res["load_ms"] > 0


@needs_model
def test_vectors_are_256_dim_normalised_and_rank_correctly(embed):
    docs = [
        {"title": "Venus", "text": "Venus, often called Earth's twin, is the hottest planet with a thick, toxic atmosphere."},
        {"title": "Mars", "text": "Mars, known for its reddish appearance, is often referred to as the Red Planet."},
        {"title": None, "text": "Jupiter, the largest planet in our solar system, has a prominent red spot."},
        {"title": "Cooking", "text": "A good risotto needs warm stock, patience and constant stirring."},
    ]
    r = embed.call("embed.documents", {**P, "items": docs}, timeout=120)["result"]
    vs = [vec(v) for v in r["vectors"]]
    assert len(vs) == 4 and all(len(v) == 256 for v in vs)
    assert all(abs(math.sqrt(dot(v, v)) - 1) < 1e-4 for v in vs)
    assert len(base64.b64decode(r["vectors"][0])) == 256 * 4
    q = vec(embed.call("embed.query", {**P, "text": "Which planet is known as the Red Planet?"})["result"]["vector"])
    assert len(q) == 256 and abs(math.sqrt(dot(q, q)) - 1) < 1e-4
    scores = [dot(q, v) for v in vs]
    assert scores.index(max(scores)) == 1, scores  # Mars
    assert scores[3] == min(scores)  # the risotto


@needs_model
def test_dims_truncation_and_order(embed):
    items = [{"title": "a", "text": "short"}, {"title": "b", "text": "a much longer text " * 30}, {"title": "c", "text": "mid length text"}]
    r64 = embed.call("embed.documents", {**P, "dims": 64, "items": items}, timeout=60)["result"]["vectors"]
    r256 = embed.call("embed.documents", {**P, "items": items}, timeout=60)["result"]["vectors"]
    assert [len(vec(v)) for v in r64] == [64, 64, 64]
    # same order as the input: the 64-dim vector is the re-normalised head of the 256-dim one
    for a, b in zip(r64, r256):
        head = vec(b)[:64]
        n = math.sqrt(dot(head, head))
        assert dot(vec(a), [x / n for x in head]) > 0.999


@needs_model
def test_query_goes_before_a_queued_document_batch(embed):
    embed.call("embed.info", P, timeout=120)
    long_text = "The quick brown fox jumps over the lazy dog. " * 30
    batch = [{"title": f"t{i}", "text": long_text} for i in range(8)]
    a = embed.send("embed.documents", {**P, "items": batch})      # starts at once
    time.sleep(0.3)                                                # ... and is running when the others arrive
    b = embed.send("embed.documents", {**P, "items": batch})      # waits in the queue
    q = embed.send("embed.query", {**P, "text": "fox"})            # queued last, must run before b
    order = []
    while len(order) < 3:
        m = embed.next(timeout=120)
        if "id" in m:
            assert "result" in m, m
            order.append(m["id"])
    assert order == [a, q, b], order
