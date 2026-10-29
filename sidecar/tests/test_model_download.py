"""`model.download` (FR-RAG-EM-3, D-82) against a local server, with and without range requests."""
import hashlib
import http.server
import json
import os
import threading

import pytest

from cairn_sidecar.errors import InvalidParams, SidecarError
from cairn_sidecar.model_download import model_download
from helpers import FakeReq

FILES = {
    "onnx/model_q4.onnx": b"graph" * 1000,
    "onnx/model_q4.onnx_data": bytes(range(256)) * 9000,  # several reads of 1 MiB
    "tokenizer.json": b'{"vocab": {}}',
}


class Server(http.server.ThreadingHTTPServer):
    ranges = True  # honour Range headers
    corrupt = None  # a path served with one byte changed
    requests = []


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):  # noqa: N802
        path = self.path.lstrip("/")
        body = FILES.get(path)
        self.server.requests.append((path, self.headers.get("Range")))
        if body is None:
            self.send_response(404)
            self.end_headers()
            return
        if path == self.server.corrupt:
            body = body[:-1] + bytes([body[-1] ^ 1])
        start = 0
        rng = self.headers.get("Range")
        if rng and self.server.ranges:
            start = int(rng.removeprefix("bytes=").rstrip("-"))
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{len(body) - 1}/{len(body)}")
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(len(body) - start))
        self.end_headers()
        self.wfile.write(body[start:])


@pytest.fixture
def server():
    s = Server(("127.0.0.1", 0), Handler)
    s.requests = []
    t = threading.Thread(target=s.serve_forever, daemon=True)
    t.start()
    yield s
    s.shutdown()


def params(server, dest):
    base = f"http://127.0.0.1:{server.server_address[1]}/"
    return {"dest_dir": str(dest), "files": [
        {"url": base + p, "path": p, "size": len(b), "sha256": hashlib.sha256(b).hexdigest()}
        for p, b in FILES.items()]}


def downloaded(dest):
    return all((dest / p).read_bytes() == b for p, b in FILES.items())


def test_downloads_every_file_and_reports_progress(server, tmp_path):
    req = FakeReq(params(server, tmp_path))
    total = sum(len(b) for b in FILES.values())
    assert model_download(req) == {"done": total, "total": total}
    assert downloaded(tmp_path)
    assert not list(tmp_path.rglob("*.part"))
    steps = [json.loads(d) for d in req.streamed]
    assert steps[-1] == {"done": total, "total": total}
    assert [s["done"] for s in steps] == sorted(s["done"] for s in steps)


@pytest.mark.parametrize("ranges", [True, False])
def test_an_interrupted_download_resumes(server, tmp_path, ranges):
    server.ranges = ranges
    p = params(server, tmp_path)
    # The first file finished earlier; the second stopped half way.
    (tmp_path / "onnx").mkdir()
    (tmp_path / "onnx/model_q4.onnx").write_bytes(FILES["onnx/model_q4.onnx"])
    data = FILES["onnx/model_q4.onnx_data"]
    (tmp_path / "onnx/model_q4.onnx_data.part").write_bytes(data[: len(data) // 2])
    model_download(FakeReq(p))
    assert downloaded(tmp_path)
    asked = dict(server.requests)
    assert "onnx/model_q4.onnx" not in asked, "a finished file isn't fetched again"
    assert asked["onnx/model_q4.onnx_data"] == f"bytes={len(data) // 2}-"


def test_a_wrong_file_is_deleted_and_reported(server, tmp_path):
    server.corrupt = "tokenizer.json"
    with pytest.raises(SidecarError) as e:
        model_download(FakeReq(params(server, tmp_path)))
    assert e.value.kind == "model_check_failed" and "tokenizer.json" in e.value.message
    assert not (tmp_path / "tokenizer.json").exists() and not (tmp_path / "tokenizer.json.part").exists()
    assert (tmp_path / "onnx/model_q4.onnx_data").exists(), "files that passed are kept"


def test_server_errors_and_cancel(server, tmp_path):
    p = params(server, tmp_path)
    p["files"][0]["url"] += "-missing"
    with pytest.raises(SidecarError) as e:
        model_download(FakeReq(p))
    assert e.value.kind == "network" and "404" in e.value.message

    req = FakeReq(params(server, tmp_path))
    req.cancelled.set()
    assert model_download(req) == {"cancelled": True}


@pytest.mark.parametrize("change", [
    {"path": "../escape"}, {"path": "/abs"}, {"path": "a\\b"}, {"size": 0}, {"sha256": "xyz"},
    {"url": "file:///etc/passwd"},
])
def test_bad_params_are_refused(server, tmp_path, change):
    p = params(server, tmp_path)
    p["files"][0].update(change)
    with pytest.raises(InvalidParams):
        model_download(FakeReq(p))
    assert not os.listdir(tmp_path)
