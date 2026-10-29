#!/usr/bin/env python
"""Owner-run live check of the REAL sidecar against REAL endpoints (never run by the tests).

    sidecar/.venv/bin/python sidecar/tools/live_check.py poc/a-llm/config.toml [--only NAME] [--m5 | --m6]

Reads the config format of poc/a-llm/config.example.toml, converts each target into a
PROTOCOL.md `target`, starts `python -I -m cairn_sidecar --role ai` and runs `llm.test`.
Keys: the environment variable named by `key_env`, else a hidden prompt. Entra targets need
`az login` done beforehand. Nothing is written to disk.

`--m5` runs the M5 checks instead of `llm.test`, per target: a `path` image (vision targets), a streamed call that
must report usage, a call with `max_tokens: 5` whose `finish_reason` must be `length`, and `llm.model_info`.

`--m6` runs the web search checks (M6, FR-LLM-AS-12), per target: one plain and one streamed `llm.complete` with
`web_search: {"max_uses": 2}` (the latest stable Python release), showing the searches, the number of sources, the
first source URL and whether any server-tool text leaked into the answer, then the `web` row of `llm.test`. A
`web_search = false` line in a target of the config skips that target. A provider that ignores web search (or
refuses it) is an INFO line, not a failure.

On the `stream` row: "first after X s of Y s" is the proof of real streaming. If X is about
equal to Y, the stream is buffered (faked) and shown at the end.
"""
import argparse
import getpass
import json
import os
import subprocess
import sys
import tomllib

KIND = {"azure_classic": "azure_foundry", "foundry_v1": "azure_foundry", "foundry_anthropic": "azure_foundry",
        "anthropic": "anthropic", "openai_compat": "openai_compatible"}
DEFAULT_LEVELS = ["low", "medium", "high"]


def to_target(c, key):
    """One [[target]] of the M0 config -> the PROTOCOL.md target object."""
    kind = c["kind"]
    fmt = c.get("format", "chat")
    if kind in ("foundry_anthropic", "anthropic"):
        fmt = "anthropic_messages"
    else:
        fmt = "responses" if fmt == "responses" else "chat_completions"
    auth = "azure_cli" if c.get("auth", "key") == "entra" else ("api_key" if key else "none")
    levels = c.get("effort_levels", DEFAULT_LEVELS)
    budgets = c.get("thinking_budgets")
    return {
        "provider_kind": KIND[kind],
        "endpoint": c.get("api_base", ""),
        "api_version": c.get("api_version"),
        "auth": auth,
        "api_key": key if auth == "api_key" else None,
        "model": c["model"],
        "format": fmt,
        "vision": bool(c.get("image", True)),
        "web_search": bool(c.get("web_search", True)),
        "effort": {"enabled": bool(c.get("effort", True)), "levels": levels, "default_level": None,
                   "custom": {k: str(v) for k, v in budgets.items()} if budgets else None},
    }


def get_key(c):
    if c.get("auth", "key") == "entra":
        return None
    env = c.get("key_env")
    key = os.environ.get(env) if env else None
    return key or getpass.getpass(f"[{c['name']}] API key ({env or 'none'}; Enter for none): ") or None


SAMPLE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "poc", "h-create", "samples",
                      "handover-scan.jpg")


def start():
    env = {k: v for k, v in os.environ.items() if k not in ("AZURE_OPENAI_API_KEY", "AZURE_API_KEY", "AZURE_AI_API_KEY")}
    return subprocess.Popen([sys.executable, "-I", "-m", "cairn_sidecar", "--role", "ai"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)


def call(p, method, params, rid=1, notes=None):
    """Send one request and wait for its answer. `notes` (a list) collects the `stream` notifications."""
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}) + "\n")
    p.stdin.flush()
    for line in p.stdout:
        m = json.loads(line)
        if m.get("id") == rid:
            return m
        if notes is not None and m.get("method") == "stream":
            notes.append(m["params"]["delta"])


def run_test(target):
    p = start()
    try:
        return call(p, "llm.test", {"target": target})
    finally:
        p.stdin.close()
        p.wait(10)


def run_m5(target):
    """The M5 checks for one target: a list of (ok, name, detail)."""
    out = []
    p = start()
    n = [0]

    def ask(method, params):
        n[0] += 1
        return call(p, method, params, n[0])

    def record(name, fn):
        try:
            ok, detail = fn()
        except Exception as e:  # noqa: BLE001
            ok, detail = False, f"{type(e).__name__}: {e}"
        out.append((ok, name, detail))

    def complete(**kw):
        res = ask("llm.complete", {"target": target, **kw})
        if "error" in res:
            e = res["error"]
            raise RuntimeError(f"{e['data'].get('kind')}: {e['message']}")
        return res["result"]

    def image():
        r = complete(messages=[{"role": "user", "content": [
            {"type": "text", "text": "Transcribe the first line of handwriting on this page, nothing else."},
            {"type": "image", "path": os.path.abspath(SAMPLE)}]}], timeout_s=120)
        return bool(r["text"].strip()), r["text"].strip()[:60] or "empty reply"

    def usage():
        r = complete(messages=[{"role": "user", "content": "Count from 1 to 10."}], stream=True)
        u = r["usage"]
        ok = bool(u) and u["input_tokens"] > 0 and u["output_tokens"] > 0
        return ok, f"usage {u}, finish_reason {r['finish_reason']}"

    def length():
        r = complete(messages=[{"role": "user", "content": "Write a long story about a dragon."}], max_tokens=5)
        return r["finish_reason"] == "length", f"finish_reason {r['finish_reason']!r}, {len(r['text'])} characters"

    def info():
        res = ask("llm.model_info", {"target": target})
        if "error" in res:
            return False, str(res["error"])
        r = res["result"]
        return True, f"{r} (null is normal for an unknown deployment name)"

    try:
        if target["vision"]:
            record("image-path", image)
        else:
            out.append((None, "image-path", "skipped: the model is not marked as vision-capable"))
        record("stream-usage", usage)
        record("finish-length", length)
        record("model-info", info)
    finally:
        p.stdin.close()
        p.wait(10)
    return out


LEAKS = ("srvtoolu", "web_search", "encrypted_", "server_tool_use", "tool_use_id")
QUESTION = "Which version of Python is the latest stable release right now? Answer in one sentence."


def run_m6(target):
    """The M6 web search checks for one target: a list of (ok, name, detail); ok is True, False, None (skipped) or
    "info" (the provider can't or won't search)."""
    out = []
    if not target.get("web_search"):
        return [(None, "web-plain", "skipped: web_search is off for this target"),
                (None, "web-stream", "skipped: web_search is off for this target")]
    p = start()
    n = [0]

    def ask(method, params, notes=None):
        n[0] += 1
        return call(p, method, params, n[0], notes)

    def web(stream):
        def run():
            notes = []
            res = ask("llm.complete", {"target": target, "stream": stream, "timeout_s": 120, "web_search": {"max_uses": 2},
                                       "messages": [{"role": "user", "content": QUESTION}]}, notes)
            if "error" in res:
                e = res["error"]
                kind = e["data"].get("kind")
                if kind == "web_search_unsupported":
                    return "info", f"the provider refused web search: {e['message']}"
                return False, f"{kind}: {e['message']}"
            r = res["result"]
            w = r.get("web")
            if not isinstance(w, dict):
                return False, "the result has no `web` object"
            leaked = [x for x in LEAKS if x in r["text"] or any(x in d for d in notes)]
            if stream and "".join(notes) != r["text"]:
                return False, f"the streamed pieces ({len(notes)}) do not add up to the text"
            first = w["sources"][0]["url"] if w["sources"] else "-"
            detail = (f"searches {w['searches']}, sources {len(w['sources'])}, first {first}, "
                      f"server-tool text in the answer: {'YES ' + str(leaked) if leaked else 'no'}"
                      f"{f', {len(notes)} stream pieces' if stream else ''}; answer: {r['text'].strip()[:80]!r}")
            if leaked:
                return False, detail
            if w["searches"] == 0:
                return "info", "no search was made: the provider ignores web search (" + detail + ")"
            return True, detail
        return run

    def record(name, fn):
        try:
            ok, detail = fn()
        except Exception as e:  # noqa: BLE001
            ok, detail = False, f"{type(e).__name__}: {e}"
        out.append((ok, name, detail))

    try:
        record("web-plain", web(False))
        record("web-stream", web(True))
        res = ask("llm.test", {"target": target})
        if "error" in res:
            out.append((False, "llm.test web", str(res["error"])))
        else:
            for ch in res["result"]["checks"]:
                if ch["name"] == "web":
                    ok = ch["ok"]
                    if ok is False and ("no search was made" in ch["detail"] or "refused web search" in ch["detail"]):
                        ok = "info"
                    out.append((ok, "llm.test web", ch["detail"]))
    finally:
        p.stdin.close()
        p.wait(10)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("config")
    ap.add_argument("--only")
    ap.add_argument("--m5", action="store_true", help="run the M5 checks instead of llm.test")
    ap.add_argument("--m6", action="store_true", help="run the M6 web search checks instead of llm.test")
    a = ap.parse_args()
    with open(a.config, "rb") as f:
        cfg = tomllib.load(f)
    failed = False
    for c in cfg["target"]:
        if a.only and c["name"] != a.only:
            continue
        print(f"\n== {c['name']} ({c['kind']}, {c.get('format', 'chat')}, model {c['model']})")
        if a.m6:
            for ok, name, detail in run_m6(to_target(c, get_key(c))):
                failed |= ok is False
                print(f"  {({True: 'PASS', False: 'FAIL', None: 'skip', 'info': 'INFO'})[ok]}  {name:<16} {detail}")
            continue
        if a.m5:
            for ok, name, detail in run_m5(to_target(c, get_key(c))):
                failed |= ok is False
                print(f"  {({True: 'PASS', False: 'FAIL', None: 'skip'})[ok]}  {name:<16} {detail}")
            continue
        res = run_test(to_target(c, get_key(c)))
        if res is None or "error" in res:
            print("  sidecar error:", (res or {}).get("error"))
            failed = True
            continue
        for ch in res["result"]["checks"]:
            mark = {True: "PASS", False: "FAIL", None: "skip"}[ch["ok"]]
            failed |= ch["ok"] is False
            print(f"  {mark}  {ch['name']:<16} {ch['detail']}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
