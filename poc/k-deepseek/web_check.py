#!/usr/bin/env python
"""Owner-run: DeepSeek as a provider and its web search (D-69) on the real DeepSeek API, through the sidecar's own
code (sidecar/cairn_sidecar/ai.py and deepseek.py), with the app's ask-web prompt.

    DEEPSEEK_API_KEY=sk-... sidecar/.venv/bin/python poc/k-deepseek/web_check.py [--model NAME ...] [--responses]

Without DEEPSEEK_API_KEY it asks for the key (hidden). Default models: deepseek-flash and deepseek-v4-pro.
Per model:
  plain   one call without web search ("Reply with exactly: pong").
  search  the Search API directly (POST /v1/tools/search, 3 results): the first title and URL.
  web     ask-web with web search on, streamed, through run_completion (Cairn's tool loop): the rounds and their
          tool_choice, searches, sources, whether the answer links the result pages, and the answer.
  limit   web search with max_uses 1 on a question that needs two lookups: at most one search or page read.
  resp    (with --responses, information only) the Responses API with DeepSeek's built-in `web_search` tool: does
          the reply carry web_search_call items and url_citation annotations? D-69 runs its own search because
          DeepSeek documents neither; if both come back, the built-in tool could be used later.
Cost: about 8 calls per model and up to 4 searches or page reads (USD 0.75 per 1,000 from 1 November 2026).
Prints everything; writes nothing.
"""
import argparse, datetime as dt, getpass, importlib.util, os, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, "sidecar"))
sys.path.insert(0, os.path.join(ROOT, "poc", "h-create"))
from cairn_sidecar import ai, deepseek  # noqa: E402
import prompts as P  # noqa: E402  (poc/h-create/prompts.py: renders crates/cairn-core/prompts/*.md)


def web_search_on_text():
    """The ask-web `search` text for web search on, as poc/i-assistant/llm_check.py keeps it (word for word as the app)."""
    spec = importlib.util.spec_from_file_location("i_llm_check", os.path.join(ROOT, "poc", "i-assistant", "llm_check.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.WEB_SEARCH_ON


ENDPOINT = ""  # DeepSeek's own address; --endpoint points the script at a mock


def target(model, key, fmt="chat_completions"):
    return ai.Target({"provider_kind": "deepseek", "endpoint": ENDPOINT, "auth": "api_key", "api_key": key, "model": model,
                      "format": fmt, "vision": False, "effort": {"enabled": False}})


def show(name, ok, detail):
    mark = {True: "PASS", False: "FAIL", None: "INFO"}[ok]
    print(f"  {mark} {name}: {detail}")


def counting_rounds():
    """Wraps litellm.completion to record each round's tool_choice."""
    lm = ai.litellm()
    rounds = []
    orig = lm.completion

    def wrapped(**kw):
        rounds.append(kw.get("tool_choice", "-"))
        return orig(**kw)
    lm.completion = wrapped
    return rounds, lambda: setattr(lm, "completion", orig)


def ask_web(question, search_text):
    today = dt.date.today()
    return P.render("ask-web", today=f"{today.isoformat()} ({today.strftime('%A')})", search=search_text, message=question)


def check_model(model, key, search_text, responses):
    print(f"\n== {model}")
    t = target(model, key)
    try:
        r = ai.run_completion(t, [{"role": "user", "content": "Reply with exactly: pong"}])
        show("plain", "pong" in r["text"].lower(), repr(r["text"][:60]))
    except Exception as e:  # noqa: BLE001
        show("plain", False, str(e)[:300])
        return

    try:
        data = deepseek._post(t, deepseek.SEARCH_PATH, {"query": "latest Python release", "count": 3}, 60)
        res = data.get("results") or []
        show("search", bool(res), f"{len(res)} results; first: {res[0].get('title')!r} {res[0].get('url')}" if res else "no results")
    except Exception as e:  # noqa: BLE001
        show("search", False, str(e)[:300])

    question = "Which version of Python is the latest stable release right now, and when was it released? Answer in two sentences."
    rounds, restore = counting_rounds()
    t0 = time.time()
    try:
        deltas = []
        r = ai.run_completion(t, ask_web(question, search_text), stream=True, on_delta=deltas.append,
                              web_search={"max_uses": 5})
        web = r["web"]
        linked = [s["url"] for s in web["sources"] if s["url"] in r["text"]]
        show("web", web["searches"] > 0,
             f"{web['searches']} searches/reads in {time.time() - t0:.0f} s; rounds {rounds}; "
             f"{len(web['sources'])} sources, {len(linked)} linked in the answer; streamed {len(deltas)} pieces")
        for s in web["sources"]:
            print(f"      source: {s['title'][:70]!r} {s['url']}")
        print("      answer: " + " ".join(r["text"].split())[:600])
        if r.get("usage"):
            print(f"      usage: {r['usage']}")
    except Exception as e:  # noqa: BLE001
        show("web", False, f"rounds {rounds}: {str(e)[:300]}")
    finally:
        restore()

    rounds, restore = counting_rounds()
    try:
        q = "Compare the latest stable releases of Python and of Rust: version numbers and release dates."
        r = ai.run_completion(t, ask_web(q, search_text), web_search={"max_uses": 1})
        show("limit", r["web"]["searches"] <= 1, f"{r['web']['searches']} searches/reads; rounds {rounds}")
    except Exception as e:  # noqa: BLE001
        show("limit", False, str(e)[:300])
    finally:
        restore()

    if responses:
        try:
            rt = target(model, key, fmt="responses")
            _, kw = ai.build_call(rt, ask_web(question, search_text), max_tokens=800)
            kw["tools"] = [{"type": "web_search"}]
            r = ai.litellm().responses(**kw)
            kinds = [getattr(o, "type", None) for o in (r.output or [])]
            notes = [getattr(a, "type", None) for o in (r.output or []) if getattr(o, "type", "") == "message"
                     for c in (o.content or []) for a in (getattr(c, "annotations", None) or [])]
            show("resp", None, f"output items {kinds}; annotations {notes}")
        except Exception as e:  # noqa: BLE001
            show("resp", None, f"refused: {str(e)[:300]}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--model", action="append", help="model name (repeatable); default deepseek-flash and deepseek-v4-pro")
    ap.add_argument("--responses", action="store_true", help="also try the Responses API's built-in web_search (information)")
    ap.add_argument("--endpoint", default="", help=argparse.SUPPRESS)  # for a dry run against sidecar/tests/mock_server.py
    a = ap.parse_args()
    global ENDPOINT
    ENDPOINT = a.endpoint
    key = os.environ.get("DEEPSEEK_API_KEY") or getpass.getpass("DeepSeek API key: ")
    search_text = web_search_on_text()
    for model in a.model or ["deepseek-flash", "deepseek-v4-pro"]:
        check_model(model, key, search_text, a.responses)


if __name__ == "__main__":
    main()
