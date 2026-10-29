"""Windows-readiness of the stdio link (M8): the app may send "\\r\\n" lines, and the sidecar must
answer with plain "\\n" on every OS. Both can be checked on Linux."""
import json
import os
import subprocess
import sys

from conftest import clean_env
from helpers import ROOT


def test_crlf_requests_are_read_and_answers_end_in_lf_only():
    # this checkout's package, as helpers.py launches it
    launch = f"import sys; sys.path.insert(0, {ROOT!r}); from cairn_sidecar.__main__ import main; sys.exit(main())"
    proc = subprocess.run(
        [sys.executable, "-I", "-c", launch, "--role", "ai"],
        input=b'{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}\r\n',
        capture_output=True, env=clean_env(), timeout=60,
    )
    assert proc.returncode == 0
    assert proc.stdout.endswith(b"}\n") and b"\r" not in proc.stdout
    assert json.loads(proc.stdout)["result"]["pong"] is True


def test_lower_priority_never_fails_without_os_nice(monkeypatch):
    from cairn_sidecar import __main__ as entry
    monkeypatch.delattr(os, "nice", raising=False)
    entry.lower_priority()  # no os.nice and not win32: does nothing, raises nothing
