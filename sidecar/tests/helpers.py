import json
import os
import queue
import subprocess
import sys
import threading
import time
import types

from conftest import clean_env

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PNG = "data:image/png;base64,iVBORw0KGgo="


class FakeReq:
    """Stands in for rpc.Request when a method is called directly."""

    def __init__(self, params):
        self.params = params
        self.cancelled = threading.Event()
        self.streamed = []
        self.cancel_result = None

    def stream(self, delta):
        self.streamed.append(delta)


def target(**over):
    t = {"provider_kind": "openai_compatible", "endpoint": "", "api_version": None, "auth": "api_key",
         "api_key": "SECRET", "model": "dep1", "format": "chat_completions", "vision": True,
         "effort": {"enabled": True, "levels": ["low", "medium", "high"], "default_level": None, "custom": None}}
    effort = over.pop("effort", None)
    t.update(over)
    if effort is not None:
        t["effort"] = {**t["effort"], **effort}
    return t


def user(text="hi", image=False):
    if not image:
        return [{"role": "user", "content": text}]
    return [{"role": "user", "content": [{"type": "text", "text": text}, {"type": "image", "data_url": PNG}]}]


class Client:
    """A real sidecar subprocess, spoken to over stdio."""

    def __init__(self, role, extra_env=None):
        env = clean_env()
        env.update(extra_env or {})
        self.t0 = time.time()
        # like `python -I -m cairn_sidecar`, but always this checkout's package (an editable install may point elsewhere)
        launch = f"import sys; sys.path.insert(0, {ROOT!r}); from cairn_sidecar.__main__ import main; sys.exit(main())"
        self.proc = subprocess.Popen([sys.executable, "-I", "-c", launch, "--role", role],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                     text=True, bufsize=1, env=env)
        self.msgs = queue.Queue()
        self.stderr = []
        self.seq = 0
        self.arrivals = []  # every message, in order, with a time
        threading.Thread(target=self._read, daemon=True).start()
        threading.Thread(target=lambda: self.stderr.extend(self.proc.stderr), daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            m = json.loads(line)
            self.arrivals.append((time.time(), m))
            self.msgs.put(m)

    def send(self, method, params=None, notify=False):
        msg = {"jsonrpc": "2.0", "method": method, "params": params or {}}
        if not notify:
            self.seq += 1
            msg["id"] = self.seq
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        return msg.get("id")

    def send_raw(self, line):
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def next(self, timeout=20):
        return self.msgs.get(timeout=timeout)

    def wait_for(self, rid, timeout=20):
        """Everything up to and including the response to `rid`: (notifications, response)."""
        notes = []
        end = time.time() + timeout
        while True:
            m = self.next(max(0.01, end - time.time()))
            if m.get("id") == rid:
                return notes, m
            notes.append(m)

    def call(self, method, params=None, timeout=20):
        return self.wait_for(self.send(method, params), timeout)[1]

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.kill()  # our own child
            self.proc.wait()
