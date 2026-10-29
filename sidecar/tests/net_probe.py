"""Run in a fresh process by test_network.py: records every DNS lookup and connect while
the sidecar's ai code makes calls against the mock (argv[1] = mock url). Prints JSON."""
import json
import socket
import sys

calls = []
_gai, _connect, _connect_ex = socket.getaddrinfo, socket.socket.connect, socket.socket.connect_ex


def host_of(addr):
    return str(addr[0]) if isinstance(addr, tuple) else str(addr)


def gai(host, *a, **k):
    calls.append(["getaddrinfo", str(host)])
    return _gai(host, *a, **k)


def connect(self, addr):
    calls.append(["connect", host_of(addr)])
    return _connect(self, addr)


def connect_ex(self, addr):
    calls.append(["connect_ex", host_of(addr)])
    return _connect_ex(self, addr)


socket.getaddrinfo = gai
socket.socket.connect = connect
socket.socket.connect_ex = connect_ex

from cairn_sidecar import ai  # noqa: E402

url = sys.argv[1]
msgs = [{"role": "user", "content": "hi"}]


def t(**kw):
    base = {"provider_kind": "openai_compatible", "auth": "api_key", "api_key": "K", "model": "m",
            "format": "chat_completions", "vision": False, "effort": {"enabled": True, "levels": ["low"]}}
    return ai.Target({**base, **kw})


for target in (
    t(endpoint=url + "/v1"),
    t(endpoint=url + "/v1", format="responses"),
    t(provider_kind="azure_foundry", endpoint=url, api_version="2025-04-01-preview"),
    t(provider_kind="azure_foundry", endpoint=url + "/anthropic", format="anthropic_messages", model="claude-x"),
    t(provider_kind="anthropic", endpoint=url, format="anthropic_messages", model="claude-sonnet-4-5"),
):
    ai.run_completion(target, msgs, level="low", stream=True, on_delta=lambda d: None)
    ai.run_completion(target, msgs)
print(json.dumps(calls))
