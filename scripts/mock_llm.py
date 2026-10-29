"""A mock LLM (OpenAI Chat Completions, streaming too) for checking Cairn's M5 and M6 flows in the real app without
real models (scripts/probes/m5-flows.js, m6-flows.js):

    python3 -I scripts/mock_llm.py 5298 [log file]

It picks a canned reply by the first words of the prompt template (crates/cairn-core/prompts/), so it must follow
the templates when their first lines change. Each request's template is appended to the log file.

M6 web search: a request with `web_search_options` gets `url_citation` annotations (as OpenAI's search models
answer), unless the question contains "ignore", which plays a provider that ignores web search (DeepSeek on Foundry,
2026-10-15).

D-69 DeepSeek: a request with the `web_search` function tool (Cairn's own search loop) first calls the tool, then answers
with a Markdown link to a result; `/v1/tools/search` and `/v1/tools/fetch` answer like DeepSeek's Search and Fetch API."""
import json, re, sys, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = sys.argv[2] if len(sys.argv) > 2 else "/dev/null"

TRUTH = """Shift handover: night shift, 14 Oct
Pattern 2214 at P3W: 86 holes loaded, 4 wet holes dewatered first.
MMU-07: auger hose replaced at 02:30, tested OK.
Product used: 18.4 t emulsion, 1.2 t AN.
Misfires: none. Kestrel updated before 6 am.
备注：P3W 北侧台阶有积水，装药前先抽水。
Checked by R. Ortiz, all good"""

PUMP = """# EP-40 pump calibration and maintenance

## Safety
Isolate the pump at the MMU control panel and bleed the line pressure before any service work.

## Calibration
Calibrate the flow meter every 250 operating hours, and always after replacing the stator.

1. Flush the line with water until no emulsion comes out.
2. Run water at 120 kg/min for 2 minutes into the calibration tank.
3. Weigh the tank. The weighed amount must be within 2% of the meter reading.
4. If it is not, enter the new K-factor in the MMU controller under Settings > Flow. The factory K-factor is 0.982.

> **Warning:** Never calibrate with emulsion in the line.

## Maintenance
| Part | Interval | Note |
|---|---|---|
| Auger slew bearing | weekly | 6 strokes of EP2 grease |
| Pump stator | 1,500 hours | replace; then calibrate |

Torque the pump mounting bolts to 210 Nm after every stator change. Record every service in Kestrel."""


def text_of(content):
    if isinstance(content, str):
        return content
    return "\n".join(b.get("text", "") for b in content if b.get("type") == "text")


def source_of(user):
    m = re.search(r"<source>\n(.*?)\n</source>", user, re.S)
    return m.group(1) if m else user


def reply_for(body):
    msgs = body["messages"]
    first = text_of(msgs[0]["content"])
    user = text_of(msgs[1]["content"]) if len(msgs) > 1 else first
    last = text_of(msgs[-1]["content"])
    if first.startswith("Transcribe"):
        return "ocr", TRUTH
    if first.startswith("You check a new page"):
        page = re.search(r"<page>\n(.*?)\n</page>", user, re.S).group(1)
        groups = re.findall(r"^- ([^:\n]+)", user.split("Page:")[0], re.M)
        group = next((g for g in groups if "maint" in g.lower()), groups[0] if groups else None)
        two = "Travel" in page and "calibration" in page.lower()
        terms = [{"term": t, "meaning": "", "confidence": "unknown"} for t in ("Kestrel", "P3W") if t in page]
        terms.append({"term": "boom clamp", "meaning": "", "confidence": "unknown"})  # the app filter must drop it
        return "check", json.dumps({"topics": ["Calibration", "Travel budget"] if two else ["Pump service"],
                                    "one_topic": not two,
                                    "group": {"name": group, "existing": True} if group else {"name": "Equipment", "existing": False},
                                    "tags": ["mmu", "pump"], "terms": terms})
    if first.startswith("That was not a valid JSON"):
        return "check-retry", "{}"
    if first.startswith("You turn source text"):
        src = source_of(user)
        if "EP-40" in src:
            return "draft-file", PUMP
        title = re.search(r"^#\s+(.+)$", src, re.M)
        return "draft-file", f"# {title.group(1) if title else 'Shift handover'}\n\n" + re.sub(r"^#\s+.+\n+", "", src, flags=re.M)
    if first.startswith("You help the user refine"):
        if last.rstrip().endswith("?"):
            return "revise-answer", "The factory K-factor is 0.982; enter it under Settings > Flow."
        draft = text_of(msgs[2]["content"])
        m = re.search(r'title to "?([^".]+)"?', last)
        if m:
            return "revise-page", re.sub(r"^# .*$", f"# {m.group(1).strip()}", draft, count=1, flags=re.M)
        return "revise-page", draft + "\n\n## Note\nRevised as asked."
    if first.startswith("You turn a task"):
        return "task-draft", "# MMU-07 auger hose wear\n\n## Problem\nThe hose split at the elbow (part HX-2210).\n\n## Fix\nMoved the boom clamp 40 mm and added sleeve SL-75. Tested at 12 bar."
    if first.startswith("You rewrite a draft page"):
        return "keep-one-topic", "# Flow meter calibration\n\nThe EP-40 flow meters on MMU-03 and MMU-07 are due for calibration."
    if first.startswith("You merge new content"):
        existing = re.search(r"<existing>\n(.*?)\n</existing>", user, re.S).group(1)
        return "suggest-merge", "# New title the app must replace\n\n" + re.sub(r"^# .*\n+", "", existing) + "\n\n> **Conflict:** existing vs new."
    if first.startswith("You sort ungrouped pages"):
        ids = re.findall(r"^- \[(p\d+)\]", user, re.M)
        return "suggest-groups", json.dumps({"pages": [{"id": i, "group": "Field notes", "existing": False} for i in ids],
                                             "new_groups": [{"name": "Field notes", "description": "Notes from site visits."}]})
    if first.startswith("You draft one part"):
        return "draft-part", "## Part\n" + source_of(user)[:400]
    if first.startswith("You combine the drafts"):
        return "draft-combine", "# Combined\n\n" + user[:600]
    m6 = m6_reply(body, first, user, last)
    if m6:
        return m6
    return "unknown", "OK"


# ------------------------------------------------------------------ M6: the assistant

STOP = {"how", "do", "i", "the", "a", "an", "what", "is", "are", "about", "please", "tell", "me", "to", "of", "in", "on",
        "for", "does", "say", "use", "set", "up", "can", "you", "my", "we", "our", "and", "with", "when", "why", "which"}


def tag(user, name):
    m = re.search(rf"<{name}>\n?(.*?)\n?</{name}>", user, re.S)
    return m.group(1).strip() if m else ""


def plan_reply(user):
    msg = tag(user, "message")
    low = msg.lower()
    attached = re.search(r"^Attached file: (.+)$", user, re.M)
    attached = attached and attached.group(1).strip() != "none"
    outside = [n for n in re.findall(r"^- (.+?) \([A-Z0-9]+\): not in scope$", user, re.M) if n.lower() in low]
    words = [w for w in re.findall(r"[\w\-一-鿿]+", msg) if w.lower() not in STOP]
    plan = {"kind": "question", "search": " ".join(words[:6]), "filters": {"categories": [], "types": [], "statuses": [],
            "priorities": [], "due": None, "updated": None}, "refs": [], "outside_scope": outside, "intro": ""}
    if attached:
        plan["kind"] = "file_draft" if not msg or "page" in low else "file_question"
    elif re.match(r"(create|add|new|make)\b", low):
        plan["kind"] = "create"
    elif re.match(r"(mark|set|change|update|move|delete|remove|comment)\b", low):
        plan["kind"] = "change"
        plan["filters"]["types"] = ["task"] if "task" in low or "mark" in low else []
    elif re.search(r"\b(list|which|blocked tasks|open tasks|due)\b", low):
        plan["kind"] = "find"
        plan["filters"]["types"] = ["task"]
        plan["filters"]["statuses"] = ["blocked"] if "blocked" in low else ["todo", "in_progress", "blocked"]
        plan["search"] = " ".join(w for w in words if w.lower() not in {"blocked", "tasks", "list", "open", "due"})
        plan["intro"] = "Here is what I found:"
    elif "boiling point" in low or "general knowledge" in low:
        plan["kind"], plan["search"] = "general", ""
    if "second one" in low:
        plan["refs"], plan["search"] = ["c2"], ""
    return json.dumps(plan)


def answer_reply(user):
    n = len(re.findall(r"^\[\d+\] ", tag(user, "sources"), re.M))
    if n == 0:
        return ("Your notes don't cover this.\n\nGeneral knowledge, not from your notes: water boils at 100 °C at sea "
                "level.")
    second = " It is also mentioned in another note [2]." if n > 1 else ""
    return f"According to your notes, this is how it works [1].{second} A number that is not a source is removed [9]."


def action_reply(user, last):
    labels = re.findall(r"^([tp]\d+): ", tag(user, "candidates"), re.M)
    category = re.search(r"^- (.+?) \([A-Z0-9]+\): in scope$", user, re.M)
    category = category.group(1) if category else "General"
    req = re.search(r"^Request: (.*)$", user, re.M)
    low = (req.group(1) if req else last).lower()
    tasks = [l for l in labels if l.startswith("t")]
    pages = [l for l in labels if l.startswith("p")]
    if "delete" in low:
        return json.dumps({"changes": [], "reply": "I can't delete anything from the chat; open the task to delete it."})
    if "page" in low and re.match(r"(create|make|add|new)", low):
        return json.dumps({"changes": [{"op": "create_page", "category": category, "title": "Notes from the chat",
                                        "content": "What the conversation found."}], "reply": "Drafting the page."})
    if "page" in low and pages:
        return json.dumps({"changes": [{"op": "update_page", "target": pages[0],
                                        "change": "Add a line saying it was checked by the mock."}],
                           "reply": "Changing the page."})
    if re.match(r"(create|add|new|make)", low):
        return json.dumps({"changes": [{"op": "create_task", "category": category, "title": "Order a spare HX-2210 hose",
                                        "type": "general", "priority": "high", "due": None, "labels": ["mmu"],
                                        "description": "Log it in SiteTrak too.", "links": [],
                                        "terms": [{"term": "SiteTrak", "meaning": "", "confidence": "unknown"},
                                                  {"term": "spare hose", "meaning": "", "confidence": "unknown"}]}],
                           "reply": "Creating the task."})
    if "comment" in low and tasks:
        return json.dumps({"changes": [{"op": "comment", "target": tasks[0], "text": "Checked by the mock."}],
                           "reply": "Adding the comment."})
    if tasks:
        status = "done" if "done" in low else "in_progress"
        return json.dumps({"changes": [{"op": "update_task", "target": t, "status": status} for t in tasks[:3]],
                           "reply": f"Setting {min(3, len(tasks))} task(s) to {status}."})
    return json.dumps({"changes": [], "reply": "Which task do you mean?"})


def m6_reply(body, first, user, last):
    if first.startswith("You are the first step of the assistant"):
        return "ask-plan", plan_reply(user), None
    if first.startswith("You are the assistant of a personal work knowledge base") and "You answer the user's question" in first:
        return "ask-answer", answer_reply(last), None
    if first.startswith("You are the assistant of a personal work knowledge base") and "asks for a change" in first:
        return "ask-action", action_reply(last, last), None
    if first.startswith("You write one new page"):
        return "ask-page", "# Notes from the chat\n\nWhat the conversation found, written by the mock.\n\n- One fact.\n- Another fact.", None
    if first.startswith("You change one existing page"):
        page = tag(last, "page")
        return "edit-page", page + "\n\nChecked by the mock.", None
    if first.startswith("You look for possible glossary terms"):
        return "doc-terms", json.dumps({"terms": [{"term": "Tanager", "meaning": "", "confidence": "unknown"},
                                                  {"term": "post-detonation report", "meaning": "", "confidence": "low"}]}), None
    if first.startswith("You answer the user's message."):
        if "web_search_options" in body and "ignore" not in last.lower():
            text = "The latest stable release is Python 3.15.0, released on 9 October 2026."
            ann = [{"type": "url_citation", "url_citation": {"url": "https://docs.python.org/3/whatsnew/3.15.html",
                                                             "title": "What's new in Python 3.15", "start_index": 0,
                                                             "end_index": len(text)}}]
            return "ask-web", text, ann
        return "ask-web-ignored", "As of my knowledge, the latest is Python 3.14 ([python.org](https://www.python.org/)).", None
    return None


DS_PAGES = [("https://www.python.org/downloads/", "Download Python | Python.org", "Python 3.15.0 is the latest release (9 October 2026)."),
            ("https://docs.python.org/3/whatsnew/3.15.html", "What's new in Python 3.15", "Changes in Python 3.15.")]


def ds_search_results(body):
    return {"results": [{"rerank_score": 2.0, "title": t, "url": u, "snippet": c, "summary": c, "content": "",
                         "site_name": "Python.org", "site_icon": None, "date": "2026-10-09T00:00:00Z", "authority": 3}
                        for u, t, c in DS_PAGES]}


def ds_wants_tools(body):
    return any((t.get("function") or {}).get("name") == "web_search" for t in body.get("tools") or [] if isinstance(t, dict))


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, obj):
        out = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def _ds_tool_call(self, body, model):
        """D-69: the first round of Cairn's DeepSeek loop: call web_search."""
        call = {"id": "call_1", "type": "function", "function": {"name": "web_search", "arguments": json.dumps({"query": "latest python release"})}}
        usage = {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110}
        with open(LOG, "a") as f:
            f.write(f"{time.strftime('%H:%M:%S')} ds-tool-call tool_choice={body.get('tool_choice')}\n")
        if not body.get("stream"):
            return self._json({"id": "c", "object": "chat.completion", "created": 1, "model": model,
                               "choices": [{"index": 0, "message": {"role": "assistant", "content": None, "tool_calls": [call]},
                                            "finish_reason": "tool_calls"}], "usage": usage})
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for d, fin in (({"role": "assistant", "tool_calls": [{"index": 0, **call}]}, None), ({}, "tool_calls")):
            chunk = {"id": "c", "object": "chat.completion.chunk", "created": 1, "model": model,
                     "choices": [{"index": 0, "delta": d, "finish_reason": fin}]}
            self.wfile.write(b"data: " + json.dumps(chunk).encode() + b"\n\n")
        u = {"id": "c", "object": "chat.completion.chunk", "created": 1, "model": model, "choices": [], "usage": usage}
        self.wfile.write(b"data: " + json.dumps(u).encode() + b"\n\ndata: [DONE]\n\n")

    def do_GET(self):
        if self.path.split("?")[0].endswith("/models"):
            return self._json({"data": [{"id": "deepseek-flash"}, {"id": "deepseek-v4-pro"}]})
        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        path = self.path.split("?")[0]
        if path.endswith("/v1/tools/search"):
            with open(LOG, "a") as f:
                f.write(f"{time.strftime('%H:%M:%S')} ds-search query={body.get('query')!r}\n")
            return self._json(ds_search_results(body))
        if path.endswith("/v1/tools/fetch"):
            return self._json({"title": "Download Python", "content": "# Download Python\n\nPython 3.15.0."})
        if ds_wants_tools(body) and not any(m.get("role") == "tool" for m in body.get("messages") or []) \
                and body.get("tool_choice") != "none":
            return self._ds_tool_call(body, body.get("model", "mock"))
        if ds_wants_tools(body):
            text = (f"The latest stable release is Python 3.15.0, released on 9 October 2026 "
                    f"([Download Python]({DS_PAGES[0][0]})).")
            kind, rest = "ask-web-deepseek", []
        else:
            kind, text, *rest = reply_for(body)
        annotations = rest[0] if rest else None
        with open(LOG, "a") as f:
            # The effort that reached the server, in whichever form the request format uses (FR-LLM-EF-2).
            effort = body.get("reasoning_effort") or body.get("reasoning") or body.get("thinking") or "-"
            f.write(f"{time.strftime('%H:%M:%S')} {kind} stream={bool(body.get('stream'))} effort={effort}\n")
        model = body.get("model", "mock")
        # D-65: like a reasoning model, a request with an effort level reports reasoning tokens
        # (more for a higher level), so the reply's "thought for N tokens" can be checked.
        thought = {"low": 120, "medium": 480, "high": 1500}.get(str(body.get("reasoning_effort") or ""), 0)
        usage = {"prompt_tokens": 100, "completion_tokens": 50 + thought, "total_tokens": 150 + thought,
                 **({"completion_tokens_details": {"reasoning_tokens": thought}} if thought else {})}
        if body.get("stream"):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            words = re.split(r"(\s+)", text)
            for i in range(0, len(words), 6):
                d = {"id": "c", "object": "chat.completion.chunk", "created": 1, "model": model,
                     "choices": [{"index": 0, "delta": {"content": "".join(words[i:i + 6])}, "finish_reason": None}]}
                self.wfile.write(b"data: " + json.dumps(d).encode() + b"\n\n")
                self.wfile.flush()
                time.sleep(0.03)
            delta = {"annotations": annotations} if annotations else {}
            d = {"id": "c", "object": "chat.completion.chunk", "created": 1, "model": model,
                 "choices": [{"index": 0, "delta": delta, "finish_reason": "stop"}]}
            self.wfile.write(b"data: " + json.dumps(d).encode() + b"\n\n")
            u = {"id": "c", "object": "chat.completion.chunk", "created": 1, "model": model, "choices": [],
                 "usage": usage}
            self.wfile.write(b"data: " + json.dumps(u).encode() + b"\n\ndata: [DONE]\n\n")
            return
        out = json.dumps({"id": "c", "object": "chat.completion", "created": 1, "model": model,
                          "choices": [{"index": 0, "message": {"role": "assistant", "content": text,
                                                               **({"annotations": annotations} if annotations else {})},
                                       "finish_reason": "stop"}],
                          "usage": usage}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
