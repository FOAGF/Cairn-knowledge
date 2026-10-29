"""DeepSeek web search (D-69, FR-LLM-AS-12).

DeepSeek's Chat Completions API has no built-in web search; DeepSeek offers a Search API and a Fetch API
instead (api-docs.deepseek.com/guides/search: `POST /v1/tools/search`, `POST /v1/tools/fetch`, the same
API key). So Cairn runs the search: the model gets two function tools, `web_search` and `web_fetch`, each
call it makes is run against those APIs, and the result goes back as the tool message, until the model
answers. The first round must search (the user asked for the web); a server that refuses a required tool
choice gets `auto` instead. At most `max_uses` calls (searches and page reads) per message; a call over the
limit is answered with LIMIT_REACHED, and the next round offers no tool.

Evidence: every search or page read that returned something counts as a search. The sources are the
pages given to the model whose URL the answer cites; when it cites none, the first pages it was given.

A rejected key (401) or an empty balance (402) stops the call with that error; any other failure of a
search or page read goes back to the model as a tool result, so it can answer with what it has.

The tool texts were written by a Fable sub-agent (the owner's rule for prompt text).
"""
import json

from .errors import SidecarError, from_exception, from_status

DEFAULT_BASE = "https://api.deepseek.com"
SEARCH_PATH = "/v1/tools/search"
FETCH_PATH = "/v1/tools/fetch"
RESULTS_PER_SEARCH = 8
QUERY_MAX = 128  # the Search API's limit for `query` and `ori_question`
FETCH_CHARS = 12000  # a page read is cut to this many characters
UNCITED_SOURCES = 5  # sources shown when the answer cites no page
HTTP_TIMEOUT = 60

# ---------------------------------------------------------------- the texts (Fable)
SEARCH_TOOL_DESCRIPTION = "Search the web. Each result has a title, URL, date, site and a short summary written for you to read. Use it for anything current and anything you are not sure of. When the results do not answer the question, search again with other words."

SEARCH_QUERY_DESCRIPTION = "The search words, 1 to 128 characters, in the language the pages are likely written in. Keep the names, codes and numbers of the question. You may use site:example.com, -site:example.com, lang:en, after:YYYY-MM-DD, before:YYYY-MM-DD and \"quoted phrases\" for exact wording."

FETCH_TOOL_DESCRIPTION = "Read one web page in full, as Markdown. Use it only when a result's summary is not enough to answer; usually it is. A page read counts toward this message's search limit."

FETCH_URL_DESCRIPTION = "The page's URL, exactly as a search result gave it."

RESULTS_HEADER = "Results for \"{query}\" ({count}). Cite every page you use inline, where you use it, as a Markdown link to its URL exactly as written here: [title](url). Never write a URL that is not in the results. Search again with other words, or read a page with web_fetch, when this is not enough; otherwise answer now."

RESULT_ITEM = "{n}. [{title}]({url})\nSite: {site}\nDate: {date}\n{summary}"

NO_RESULTS = "No results for \"{query}\". Try other words, a broader query or another language; or answer with what you have and say the search found nothing."

SEARCH_FAILED = "The search could not run: {reason}. Answer from what you already have. When nothing came from the web, say plainly that you could not search and that the answer may be out of date."

FETCH_HEADER = "Page \"{title}\" at {url}. Cite it as a Markdown link to this URL when you use it. Content:"

FETCH_FAILED = "The page {url} could not be read: {reason}. Use its summary, or another result, instead."

LIMIT_REACHED = "Not run: this message's limit of {limit} searches and page reads is used up. Answer now with what you have, citing the pages you used, and say so when something could not be checked."

TOOLS = [
    {"type": "function", "function": {
        "name": "web_search", "description": SEARCH_TOOL_DESCRIPTION,
        "parameters": {"type": "object", "properties": {"query": {"type": "string", "description": SEARCH_QUERY_DESCRIPTION}},
                       "required": ["query"]}}},
    {"type": "function", "function": {
        "name": "web_fetch", "description": FETCH_TOOL_DESCRIPTION,
        "parameters": {"type": "object", "properties": {"url": {"type": "string", "description": FETCH_URL_DESCRIPTION}},
                       "required": ["url"]}}},
]


# ---------------------------------------------------------------- the Search and Fetch API
def base_url(t):
    """The API's base address: the provider's endpoint (a trailing /v1 dropped, the tool paths have it) or DeepSeek's."""
    base = (t.endpoint or DEFAULT_BASE).rstrip("/")
    return base[:-3] if base.endswith("/v1") else base


def _post(t, path, body, timeout):
    import httpx
    try:
        resp = httpx.post(base_url(t) + path, json=body, timeout=timeout,
                          headers={"Authorization": f"Bearer {t.api_key or ''}", "Content-Type": "application/json"})
    except Exception as e:  # noqa: BLE001
        raise from_exception(e, t.secrets())
    if resp.status_code >= 400:
        try:
            detail = resp.json()["error"]["message"]
        except Exception:  # noqa: BLE001
            detail = resp.text
        if resp.status_code == 402:
            raise SidecarError("internal", f"DeepSeek's balance is used up (402): top it up to search the web. {detail}"[:300],
                               status=402)
        raise from_status(resp.status_code, detail)
    try:
        return resp.json()
    except ValueError:
        raise SidecarError("internal", "DeepSeek's answer was not valid JSON.")


def _stops(e):
    """A failure that ends the whole call: a rejected key, or no balance left."""
    return isinstance(e, SidecarError) and (e.kind == "auth" or getattr(e, "status", None) == 402)


def _reason(e):
    return " ".join(str(getattr(e, "message", None) or e).split())[:200] or "unknown error"


def _link_url(url):
    """A URL that stays one Markdown link target."""
    return url.replace(" ", "%20").replace("(", "%28").replace(")", "%29")


def _one_line(s):
    return " ".join(str(s or "").split())


def format_results(query, results):
    items = []
    for n, r in enumerate(results, start=1):
        url = r["url"]
        text = RESULT_ITEM.format(n=n, title=_one_line(r.get("title")).replace("[", "(").replace("]", ")") or url,
                                  url=_link_url(url), site=_one_line(r.get("site_name")), date=_one_line(r.get("date")),
                                  summary=(r.get("summary") or r.get("snippet") or "").strip())
        # A line whose value is empty (no site, no date) is dropped.
        items.append("\n".join(line for line in text.split("\n") if line.strip() not in ("Site:", "Date:")))
    return RESULTS_HEADER.format(query=query, count=len(results)) + "\n\n" + "\n\n".join(items)


# ---------------------------------------------------------------- one round with the model
def _round(lm, kw, stream, on_delta, cancelled, sent):
    """One Chat Completions call. Returns (text, tool calls, reasoning, usage, finish, model, stopped)."""
    from .ai import _close_stream, _finish, _usage

    r = lm.completion(**kw)
    if not stream:
        msg = r.choices[0].message
        calls = [{"id": c.id, "name": c.function.name, "arguments": c.function.arguments or ""}
                 for c in (getattr(msg, "tool_calls", None) or [])]
        return (msg.content or "", calls, getattr(msg, "reasoning_content", None), _usage(getattr(r, "usage", None)),
                _finish(r.choices[0].finish_reason), getattr(r, "model", None), False)
    text, reasoning, calls = [], [], {}
    usage = finish = model = None
    stopped = False
    try:
        for ch in r:
            if cancelled is not None and cancelled.is_set():
                stopped = True
                break
            if ch.choices:
                d = ch.choices[0].delta
                if d.content:
                    text.append(d.content)
                    sent.append(d.content)
                    if on_delta:
                        on_delta(d.content)
                if getattr(d, "reasoning_content", None):
                    reasoning.append(d.reasoning_content)
                for tc in getattr(d, "tool_calls", None) or []:
                    slot = calls.setdefault(tc.index if tc.index is not None else len(calls),
                                            {"id": None, "name": "", "arguments": ""})
                    slot["id"] = tc.id or slot["id"]
                    if tc.function is not None:
                        slot["name"] += tc.function.name or ""
                        slot["arguments"] += tc.function.arguments or ""
                finish = _finish(ch.choices[0].finish_reason) or finish
            usage = _usage(getattr(ch, "usage", None)) or usage
            model = getattr(ch, "model", None) or model
    finally:
        _close_stream(r)
    ordered = [calls[k] for k in sorted(calls)]
    for i, c in enumerate(ordered):
        c["id"] = c["id"] or f"call_{i}"
    return "".join(text), ordered, "".join(reasoning) or None, usage, finish, model, stopped


def _add_usage(total, u):
    if not u:
        return total
    out = dict(total or {"input_tokens": 0, "output_tokens": 0})
    out["input_tokens"] += u.get("input_tokens", 0)
    out["output_tokens"] += u.get("output_tokens", 0)
    if u.get("reasoning_tokens"):
        out["reasoning_tokens"] = out.get("reasoning_tokens", 0) + u["reasoning_tokens"]
    return out


def _question(messages):
    """The user's last message, for the Search API's `ori_question`."""
    for m in reversed(messages):
        if m.get("role") == "user":
            c = m.get("content")
            text = c if isinstance(c, str) else " ".join(b.get("text", "") for b in c or [] if b.get("type") == "text")
            return _one_line(text)[:QUERY_MAX]
    return ""


# ---------------------------------------------------------------- the loop
def run(t, messages, level, stream, max_tokens, on_delta, cancelled, timeout, web_search, ev):
    """`run_completion` for a DeepSeek target with web search on. Same result shape, with "web"."""
    from .ai import _with_retries, build_call, litellm

    lm = litellm()
    limit = web_search["max_uses"]
    question = _question(messages)
    _, kw = build_call(t, messages, level, stream, max_tokens, timeout, None)
    convo = list(kw["messages"])
    ev.reset()
    given = {}  # url -> {"url", "title"}: every page given to the model, in order
    cited_forms = {}  # url -> the forms it may take in the answer (as given, and as a link target)
    used = 0
    texts, usage, finish, model = [], None, None, None
    must_search, required_ok = True, True
    sent = []  # streamed text already shown (nothing is retried after that)

    def give(url, title):
        if isinstance(url, str) and url and url not in given:
            given[url] = {"url": url, "title": title if isinstance(title, str) else "", "cited_text": ""}
            cited_forms[url] = {url, _link_url(url)}

    def search(args):
        query = _one_line(args.get("query"))[:QUERY_MAX]
        if not query:
            return SEARCH_FAILED.format(reason="the query was empty")
        body = {"query": query, "count": RESULTS_PER_SEARCH}
        if question:
            body["ori_question"] = question
        try:
            data = _post(t, SEARCH_PATH, body, HTTP_TIMEOUT)
        except Exception as e:  # noqa: BLE001
            if _stops(e):
                raise
            return SEARCH_FAILED.format(reason=_reason(e))
        results = [r for r in data.get("results") or [] if isinstance(r, dict) and isinstance(r.get("url"), str)]
        ev.add_call()
        if not results:
            return NO_RESULTS.format(query=query)
        for r in results:
            give(r["url"], r.get("title"))
        return format_results(query, results)

    def fetch(args):
        url = _one_line(args.get("url"))
        if not url:
            return FETCH_FAILED.format(url="(none)", reason="no URL was given")
        try:
            data = _post(t, FETCH_PATH, {"url": url}, HTTP_TIMEOUT)
        except Exception as e:  # noqa: BLE001
            if _stops(e):
                raise
            return FETCH_FAILED.format(url=url, reason=_reason(e))
        content = data.get("content") or ""
        if len(content) > FETCH_CHARS:
            content = content[:FETCH_CHARS] + "\n…"
        title = _one_line(data.get("title")) or url
        ev.add_call()
        give(url, title)
        return FETCH_HEADER.format(title=title, url=_link_url(url)) + "\n\n" + content

    for _round_no in range(limit + 2):
        if cancelled is not None and cancelled.is_set():
            finish = "cancelled"
            break
        round_kw = dict(kw, messages=list(convo), tools=TOOLS)
        if used >= limit:
            round_kw["tool_choice"] = "none"
        elif must_search and required_ok:
            round_kw["tool_choice"] = "required"
        else:
            round_kw["tool_choice"] = "auto"

        before = len(sent)

        def attempt():
            del sent[before:]  # a retried round starts again
            return _round(lm, round_kw, stream, on_delta, cancelled, sent)

        try:
            text, calls, reasoning, u, fin, mdl, stopped = _with_retries(attempt, lambda: len(sent) > before, cancelled)
        except Exception as e:  # noqa: BLE001
            # A server that refuses a required tool choice: let the model choose (once).
            if round_kw.get("tool_choice") == "required" and from_exception(e).kind == "bad_request":
                required_ok = False
                continue
            raise
        usage = _add_usage(usage, u)
        model = mdl or model
        finish = fin or finish
        if text:
            texts.append(text)
        if stopped:
            finish = "cancelled"
            break
        if not calls:
            break
        must_search = False
        convo.append({"role": "assistant", "content": text or None,
                      "tool_calls": [{"id": c["id"], "type": "function",
                                      "function": {"name": c["name"], "arguments": c["arguments"] or "{}"}} for c in calls],
                      **({"reasoning_content": reasoning} if reasoning else {})})
        for c in calls:
            # A single model response may contain several tools. Stop before each new HTTP call.
            if cancelled is not None and cancelled.is_set():
                finish = "cancelled"
                break
            if used >= limit:
                result = LIMIT_REACHED.format(limit=limit)
            else:
                used += 1
                try:
                    args = json.loads(c["arguments"] or "{}")
                    args = args if isinstance(args, dict) else {}
                except ValueError:
                    args = {}
                if c["name"] == "web_fetch":
                    result = fetch(args)
                elif c["name"] == "web_search":
                    result = search(args)
                else:
                    used -= 1
                    result = f"There is no tool called {c['name']}. Use web_search or web_fetch."
            convo.append({"role": "tool", "tool_call_id": c["id"], "content": result})

    answer = "".join(texts)
    cited = [u for u in given if any(f in answer for f in cited_forms[u])]
    for u in cited or list(given)[:UNCITED_SOURCES]:
        ev.sources[u] = given[u]
    return {"text": answer, "model": model or t.model, "usage": usage, "finish_reason": finish,
            "web": ev.result(False)}
