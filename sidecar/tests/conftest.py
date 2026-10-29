import os
import sys

import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

# Tests must never reach the internet, and never see real keys or proxies.
for k in list(os.environ):
    if k.startswith(("AZURE_", "OPENAI_", "ANTHROPIC_")) or k.lower() in ("http_proxy", "https_proxy", "all_proxy"):
        del os.environ[k]
os.environ["NO_PROXY"] = "127.0.0.1,localhost"
os.environ["no_proxy"] = "127.0.0.1,localhost"

import mock_server  # noqa: E402


def clean_env():
    """Environment for sidecar subprocesses: like the app's, but without the LiteLLM settings
    (the sidecar must set those defaults itself)."""
    env = {k: v for k, v in os.environ.items()
           if k not in ("LITELLM_LOCAL_MODEL_COST_MAP", "LITELLM_LOG", "PYTHONPATH")}
    env["PYTHONUNBUFFERED"] = "1"
    return env


@pytest.fixture(scope="session")
def _srv():
    srv, state, url = mock_server.start()
    yield state, url
    srv.shutdown()


@pytest.fixture
def mock(_srv):
    state, url = _srv
    state.reset()
    state.url = url
    return state


@pytest.fixture
def new_mock():
    """A separate mock server, for tests that need two at once."""
    servers = []

    def make():
        srv, state, url = mock_server.start()
        state.url = url
        servers.append(srv)
        return state

    yield make
    for s in servers:
        s.shutdown()


@pytest.fixture(autouse=True)
def fast_retries(monkeypatch):
    from cairn_sidecar import ai
    monkeypatch.setattr(ai, "NUM_RETRIES", 0)
    monkeypatch.setattr(ai, "BACKOFF", 0.0)
