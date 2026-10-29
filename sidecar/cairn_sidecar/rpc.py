"""Newline-delimited JSON-RPC 2.0 over stdio (see PROTOCOL.md).

One reader thread parses stdin and dispatches. Handlers either run in their own
thread (role `ai`) or on a single worker thread fed by a priority queue (role
`embed`). All writes to stdout go through one lock.
"""
import itertools
import json
import os
import queue
import sys
import threading

from . import __version__, errors
from .errors import InvalidParams, SidecarError


def log(*a, level="INFO"):
    """One stderr line `[sidecar] LEVEL text`; the app maps LEVEL to its own log (NFR-SEC-3)."""
    print("[sidecar]", level, *a, file=sys.stderr, flush=True)


class Request:
    """One request in flight. Handlers get this as `req`."""

    def __init__(self, server, rid, method, params):
        self.server, self.id, self.method, self.params = server, rid, method, params
        self.cancelled = threading.Event()
        self._lock = threading.Lock()
        self._answered = False
        self.cancel_result = lambda: {"cancelled": True}

    def stream(self, delta):
        """Send a `stream` notification (dropped once the request is cancelled)."""
        if delta and not self.cancelled.is_set():
            self.server.send({"jsonrpc": "2.0", "method": "stream", "params": {"id": self.id, "delta": delta}})

    def _answer(self, msg):
        with self._lock:  # first answer wins (cancel may answer before the handler returns)
            if self._answered:
                return
            self._answered = True
        self.server.send(msg)

    def result(self, value):
        self._answer({"jsonrpc": "2.0", "id": self.id, "result": value})

    def error(self, err):
        self._answer({"jsonrpc": "2.0", "id": self.id,
                      "error": {"code": err.code, "message": err.message, "data": err.data()}})

    def cancel(self):
        self.cancelled.set()
        self.result(self.cancel_result())


class Server:
    """`methods` maps a name to `(function(req) -> result, priority)`.

    priority None: run in a new thread. An int: run on the queue worker, lowest first.
    """

    def __init__(self, role, methods, secrets_of=lambda params: ()):
        self.role, self.secrets_of = role, secrets_of
        self.methods = methods
        self._out = threading.Lock()
        # Libraries sometimes print() to stdout (LiteLLM does). Keep the real stdout for
        # protocol messages only and point `sys.stdout` at stderr.
        self._stdout, sys.stdout = sys.stdout, sys.stderr
        self._inflight = {}
        self._inflight_lock = threading.Lock()
        self._queue = queue.PriorityQueue()
        self._seq = itertools.count()
        self._worker = None

    # ---- output
    def send(self, obj):
        line = json.dumps(obj, separators=(",", ":"), ensure_ascii=True) + "\n"
        with self._out:
            try:
                self._stdout.write(line)
                self._stdout.flush()
            except (BrokenPipeError, ValueError):
                os._exit(0)  # the app is gone

    # ---- running a request
    def _run(self, req, fn):
        if req.cancelled.is_set():
            return
        try:
            req.result(fn(req))
        except BaseException as e:  # noqa: BLE001 - everything must become an answer
            err = errors.from_exception(e, self.secrets_of(req.params))
            if err.kind == "internal" and err.code == errors.APP_ERROR:
                log(f"{req.method}: {type(e).__name__}", level="ERROR")  # class name only, never content
            req.error(err)
        finally:
            with self._inflight_lock:
                self._inflight.pop(req.id, None)

    def _queue_worker(self):
        while True:
            _, _, req, fn = self._queue.get()
            self._run(req, fn)

    def _dispatch(self, msg):
        method, params, rid = msg.get("method"), msg.get("params", {}), msg.get("id")
        is_request = "id" in msg
        if method == "cancel":
            with self._inflight_lock:
                req = self._inflight.get(params.get("id") if isinstance(params, dict) else None)
            if req:
                req.cancel()
            return
        if not isinstance(method, str):
            if is_request:
                self.send({"jsonrpc": "2.0", "id": rid, "error": {"code": errors.INVALID_REQUEST, "message": "invalid request"}})
            return
        if params is None:
            params = {}
        entry = self.methods.get(method)
        if entry is None:
            if is_request:
                self.send({"jsonrpc": "2.0", "id": rid, "error": {"code": errors.METHOD_NOT_FOUND, "message": f"unknown method: {method}"}})
            return
        if not is_request:
            return
        if not isinstance(params, dict):
            self.send({"jsonrpc": "2.0", "id": rid, "error": {"code": errors.INVALID_PARAMS, "message": "params must be an object"}})
            return
        fn, priority = entry
        req = Request(self, rid, method, params)
        if priority == "inline":  # ping, shutdown: answered by the reader thread
            self._run(req, fn)
            return
        with self._inflight_lock:
            self._inflight[rid] = req
        if priority is None:
            threading.Thread(target=self._run, args=(req, fn), daemon=True, name=f"req-{rid}").start()
        else:
            if self._worker is None:
                self._worker = threading.Thread(target=self._queue_worker, daemon=True, name="worker")
                self._worker.start()
            self._queue.put((priority, next(self._seq), req, fn))

    def serve(self):
        """Read stdin until it closes; then exit 0 (the app closes stdin to stop us)."""
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except ValueError:
                self.send({"jsonrpc": "2.0", "id": None, "error": {"code": errors.PARSE_ERROR, "message": "parse error"}})
                continue
            if not isinstance(msg, dict):
                self.send({"jsonrpc": "2.0", "id": None, "error": {"code": errors.INVALID_REQUEST, "message": "invalid request"}})
                continue
            try:
                self._dispatch(msg)
            except Exception as e:  # noqa: BLE001 - the reader must survive anything
                log(f"dispatch: {type(e).__name__}", level="ERROR")
        self._stdout.flush()
        os._exit(0)


def common_methods(role):
    def ping(req):
        return {"pong": True, "role": role, "pid": os.getpid(),
                "python": sys.version.split()[0], "version": __version__}

    def shutdown(req):
        # Answer first (the Server writes the result after we return), then exit shortly after.
        threading.Timer(0.05, lambda: os._exit(0)).start()
        return {}

    return {"ping": (ping, "inline"), "shutdown": (shutdown, "inline")}


__all__ = ["Server", "Request", "common_methods", "log", "InvalidParams", "SidecarError"]
