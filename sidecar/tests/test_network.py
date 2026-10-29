"""NFR-SEC-2: the sidecar contacts only the configured endpoint (LiteLLM must not fetch anything else)."""
import json
import os
import subprocess
import sys

from conftest import HERE, clean_env


def test_only_the_mock_host_is_contacted(mock):
    env = clean_env()  # without LITELLM_LOCAL_MODEL_COST_MAP: the sidecar's own default must be enough
    out = subprocess.run([sys.executable, "-I", os.path.join(HERE, "net_probe.py"), mock.url],
                         capture_output=True, text=True, env=env, timeout=120)
    assert out.returncode == 0, out.stderr[-2000:]
    calls = json.loads(out.stdout.strip().splitlines()[-1])
    assert calls, "the probe saw no network use at all: it is broken"
    hosts = {h for _, h in calls}
    assert hosts <= {"127.0.0.1"}, f"unexpected network targets: {hosts}"
