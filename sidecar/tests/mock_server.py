"""Local mock of OpenAI Chat Completions, OpenAI Responses and Anthropic Messages.
Records every request. Scenarios: 'ok' (default), '401', '403', '429', '429_then_ok',
'slow' (chat streams send one chunk every 0.2 s for a long time, for cancel tests),
'length' (every API says the reply was cut at the token limit), 'too_long' (400, input too long, in each
API's wording), 'hang' (every request answers only after 1.5 s), 'hang_then_ok' (only the first one does).
Web search (M6): scenario 'web' answers a request that asks for web search (an Anthropic web_search tool,
a Responses web_search tool, or web_search_options) like a provider that searched, with sources, in each API's shape
(the Anthropic one follows the owner's recorded Foundry probe); 'web_refused' answers such a request with a 400 that
names the tool; 'web_slow' streams a Claude search that never finishes (for cancel). Any other scenario ignores
web search, like DeepSeek on Foundry.
GET .../models lists models (OpenAI style, with display_name for the Anthropic Models API).
Copied from poc/a-llm/mock_server.py and extended."""
import json, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class State:
    def __init__(self):
        self.requests = []
        self.scenario = "ok"
        self.fail_count = 0
        self.lock = threading.Lock()

    def reset(self, scenario="ok"):
        with self.lock:
            self.requests.clear(); self.scenario = scenario; self.fail_count = 0


def _sse(events):
    out = b""
    for name, data in events:
        if name:
            out += f"event: {name}\n".encode()
        out += b"data: " + (data if isinstance(data, str) else json.dumps(data)).encode() + b"\n\n"
    return out


def _chat_usage(reasoning=None):
    """D-65: a request that asked for effort gets reasoning tokens, as reasoning models report them."""
    u = {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
    if reasoning is not None:
        u["completion_tokens_details"] = {"reasoning_tokens": reasoning}
    return u


def chat_response(model, fin="stop", reasoning=None):
    return {"id": "chatcmpl-1", "object": "chat.completion", "created": 1, "model": model,
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hello from mock"}, "finish_reason": fin}],
            "usage": _chat_usage(reasoning)}


def chat_stream(model, fin="stop", usage=False, reasoning=None):
    def ch(delta, fin=None):
        return {"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": fin}]}
    return _sse([(None, ch({"role": "assistant", "content": ""})), (None, ch({"content": "Hello "})),
                 (None, ch({"content": "from mock"})), (None, ch({}, fin))] + ([(None, {"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": model,
                                     "choices": [], "usage": _chat_usage(reasoning)})] if usage else [])
                + [(None, "[DONE]")])


def responses_obj(model, status="completed", with_output=True, reasoning=None):
    extra = {"incomplete_details": {"reason": "max_output_tokens"}} if status == "incomplete" else {}
    return {**extra, "id": "resp_1", "object": "response", "created_at": 1, "status": status, "model": model,
            "output": [{"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
                        "content": [{"type": "output_text", "text": "Hello from mock", "annotations": []}]}] if with_output else [],
            "usage": {"input_tokens": 5, "output_tokens": 3, "total_tokens": 8,
                      **({"output_tokens_details": {"reasoning_tokens": reasoning}} if reasoning is not None else {})},
            "parallel_tool_calls": True, "tool_choice": "auto", "tools": []}


def responses_stream(model, status="completed"):
    base = responses_obj(model, "in_progress", False)
    item = {"type": "message", "id": "msg_1", "status": "in_progress", "role": "assistant", "content": []}
    part = {"type": "output_text", "text": "", "annotations": []}
    ev = [("response.created", {"type": "response.created", "sequence_number": 0, "response": base}),
          ("response.in_progress", {"type": "response.in_progress", "sequence_number": 1, "response": base}),
          ("response.output_item.added", {"type": "response.output_item.added", "sequence_number": 2, "output_index": 0, "item": item}),
          ("response.content_part.added", {"type": "response.content_part.added", "sequence_number": 3, "item_id": "msg_1", "output_index": 0, "content_index": 0, "part": part})]
    n = 4
    for d in ("Hello ", "from mock"):
        ev.append(("response.output_text.delta", {"type": "response.output_text.delta", "sequence_number": n, "item_id": "msg_1", "output_index": 0, "content_index": 0, "delta": d})); n += 1
    ev.append(("response.output_text.done", {"type": "response.output_text.done", "sequence_number": n, "item_id": "msg_1", "output_index": 0, "content_index": 0, "text": "Hello from mock"})); n += 1
    done = responses_obj(model, status)
    ev.append(("response.output_item.done", {"type": "response.output_item.done", "sequence_number": n, "output_index": 0, "item": done["output"][0]})); n += 1
    name = "response.completed" if status == "completed" else "response.incomplete"
    ev.append((name, {"type": name, "sequence_number": n, "response": done}))
    return _sse(ev)


def anthropic_response(model, stop="end_turn"):
    return {"id": "msg_1", "type": "message", "role": "assistant", "model": model,
            "content": [{"type": "text", "text": "Hello from mock"}], "stop_reason": stop, "stop_sequence": None,
            "usage": {"input_tokens": 5, "output_tokens": 3}}


def anthropic_stream(model, stop="end_turn"):
    return _sse([
        ("message_start", {"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "usage": {"input_tokens": 5, "output_tokens": 1}}}),
        ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello "}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "from mock"}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        ("message_delta", {"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": None}, "usage": {"output_tokens": 3}}),
        ("message_stop", {"type": "message_stop"})])


WEB_PAGES = [("https://www.python.org/downloads/", "Download Python | Python.org", "Latest: Python 3.15.0"),
             ("https://docs.python.org/3/whatsnew/3.15.html", "What's new in Python 3.15", "Python 3.15 is the latest")]


# ---- D-69: DeepSeek, whose web search Cairn runs: function tools in Chat Completions, and the Search / Fetch API.
# Scenarios: 'ok' searches once, then answers citing the first page; 'ds_more' asks for another search every round
# until tool_choice is "none"; 'ds_fetch' searches, reads the second page, then cites it; 'ds_uncited' answers
# without a link; 'ds_no_required' refuses tool_choice "required" with a 400; 'ds_search_500' / 'ds_search_401' /
# 'ds_402' make the Search API fail that way.
def ds_tools(body):
    return any(isinstance(t, dict) and (t.get("function") or {}).get("name") == "web_search" for t in body.get("tools") or [])


def ds_next(body, sc):
    """What the model does next: ("call", name, args) or ("text", text)."""
    tool_msgs = [m for m in body.get("messages") or [] if m.get("role") == "tool"]
    if body.get("tool_choice") == "none":
        return ("text", "Out of searches; the latest is [Download Python](https://www.python.org/downloads/).")
    if sc == "ds_more":
        return ("call", "web_search", {"query": f"python release {len(tool_msgs)}"})
    if not tool_msgs:
        return ("call", "web_search", {"query": "latest python"})
    if sc == "ds_fetch" and len(tool_msgs) == 1:
        return ("call", "web_fetch", {"url": WEB_PAGES[1][0]})
    if sc == "ds_fetch":
        return ("text", f"See [What's new]({WEB_PAGES[1][0]}).")
    if sc == "ds_uncited":
        return ("text", "Python 3.15 is the latest.")
    return ("text", f"Python 3.15.0 is out: [Download Python]({WEB_PAGES[0][0]}).")


def ds_chat(model, body, sc, stream):
    act = ds_next(body, sc)
    usage = {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
    if not stream:
        r = chat_response(model)
        msg = r["choices"][0]["message"]
        if act[0] == "call":
            msg["content"] = None
            msg["tool_calls"] = [{"id": "call_1", "type": "function",
                                  "function": {"name": act[1], "arguments": json.dumps(act[2])}}]
            msg["reasoning_content"] = "I should search."
            r["choices"][0]["finish_reason"] = "tool_calls"
        else:
            msg["content"] = act[1]
        return r

    def ch(delta, fin=None):
        return {"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": fin}]}
    if act[0] == "call":
        args = json.dumps(act[2])
        events = [(None, ch({"role": "assistant", "content": None, "reasoning_content": "I should "})),
                  (None, ch({"reasoning_content": "search."})),
                  (None, ch({"tool_calls": [{"index": 0, "id": "call_1", "type": "function",
                                             "function": {"name": act[1], "arguments": args[:5]}}]})),
                  (None, ch({"tool_calls": [{"index": 0, "function": {"arguments": args[5:]}}]})),
                  (None, ch({}, "tool_calls"))]
    else:
        half = len(act[1]) // 2
        events = [(None, ch({"role": "assistant", "content": act[1][:half]})), (None, ch({"content": act[1][half:]})),
                  (None, ch({}, "stop"))]
    if (body.get("stream_options") or {}).get("include_usage"):
        events.append((None, {"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": model,
                              "choices": [], "usage": usage}))
    return _sse(events + [(None, "[DONE]")])


def ds_search(body, sc):
    if sc == "ds_search_500":
        return 500, {"error": {"message": "search backend down", "type": "server_error"}}
    if sc == "ds_search_401":
        return 401, {"error": {"message": "Authentication Fails", "type": "authentication_error"}}
    if sc == "ds_402":
        return 402, {"error": {"message": "Insufficient Balance", "type": "insufficient_balance"}}
    return 200, {"results": [{"rerank_score": 2.4, "title": title, "url": url, "snippet": cited, "summary": cited + " (summary)",
                              "content": "", "site_name": "Python.org" if i == 0 else None, "site_icon": None,
                              "date": "2026-10-01T00:00:00Z" if i == 0 else None, "authority": 3}
                             for i, (url, title, cited) in enumerate(WEB_PAGES)]}


def ds_fetch(body):
    return 200, {"title": "What's new in Python 3.15", "content": "# What's new\n\n" + "x" * 20000}


def wants_web(body):
    """Did the request ask for web search, in any API's way?"""
    if body.get("web_search_options") is not None:
        return True
    return any(isinstance(t, dict) and str(t.get("type", "")).startswith("web_search") for t in body.get("tools") or [])


def web_error(path):
    if path.endswith("/messages"):
        msg = "tools.0: Input tag 'web_search_20250305' found using 'type' does not match any of the expected tags"
        return {"type": "error", "error": {"type": "invalid_request_error", "message": msg}}
    return {"error": {"message": "Web search is not enabled for this organization.", "type": "invalid_request_error",
                      "code": "web_search_not_enabled"}}


def anthropic_web_response(model, body):
    tool = next(t for t in body["tools"] if t.get("name") == "web_search")
    cites = [{"type": "web_search_result_location", "url": u, "title": ti, "cited_text": c, "encrypted_index": "x"}
             for u, ti, c in WEB_PAGES]
    return {"id": "msg_1", "type": "message", "role": "assistant", "model": model, "stop_reason": "end_turn",
            "stop_sequence": None,
            "content": [{"type": "text", "text": "I'll search. "},
                        {"type": "server_tool_use", "id": "srvtoolu_01A", "name": "web_search", "input": {"query": "latest python"}},
                        {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_01A",
                         "content": [{"type": "web_search_result", "url": u, "title": ti, "encrypted_content": "e", "page_age": None}
                                     for u, ti, _ in WEB_PAGES]},
                        {"type": "text", "text": "Python 3.15.0 is the latest. ", "citations": [cites[0]]},
                        {"type": "text", "text": "It has new features.", "citations": [cites[1], cites[0]]}],
            "usage": {"input_tokens": 5, "output_tokens": 3, "server_tool_use": {"web_search_requests": min(1, tool.get("max_uses", 1))}}}


def anthropic_web_stream(model, slow=False):
    cites = [{"type": "web_search_result_location", "url": u, "title": ti, "cited_text": c, "encrypted_index": "x"}
             for u, ti, c in WEB_PAGES]
    ev = [("message_start", {"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "usage": {"input_tokens": 5, "output_tokens": 1}}}),
          ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
          ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "I'll search. "}}),
          ("content_block_stop", {"type": "content_block_stop", "index": 0}),
          ("content_block_start", {"type": "content_block_start", "index": 1, "content_block": {"type": "server_tool_use", "id": "srvtoolu_01A", "name": "web_search", "input": {}}}),
          ("content_block_delta", {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"query\": \"lat"}})]
    if slow:
        return ev  # the caller keeps the stream open
    ev += [("content_block_delta", {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "est python\"}"}}),
           ("content_block_stop", {"type": "content_block_stop", "index": 1}),
           ("content_block_start", {"type": "content_block_start", "index": 2, "content_block": {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_01A",
                                    "content": [{"type": "web_search_result", "url": u, "title": ti, "encrypted_content": "e", "page_age": None} for u, ti, _ in WEB_PAGES]}}),
           ("content_block_stop", {"type": "content_block_stop", "index": 2}),
           ("content_block_start", {"type": "content_block_start", "index": 3, "content_block": {"type": "text", "text": ""}}),
           ("content_block_delta", {"type": "content_block_delta", "index": 3, "delta": {"type": "text_delta", "text": "Python 3.15.0 "}}),
           ("content_block_delta", {"type": "content_block_delta", "index": 3, "delta": {"type": "citations_delta", "citation": cites[0]}}),
           ("content_block_delta", {"type": "content_block_delta", "index": 3, "delta": {"type": "text_delta", "text": "is the latest."}}),
           ("content_block_delta", {"type": "content_block_delta", "index": 3, "delta": {"type": "citations_delta", "citation": cites[1]}}),
           ("content_block_delta", {"type": "content_block_delta", "index": 3, "delta": {"type": "citations_delta", "citation": cites[0]}}),
           ("content_block_stop", {"type": "content_block_stop", "index": 3}),
           ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None},
                              "usage": {"output_tokens": 3, "server_tool_use": {"web_search_requests": 1}}}),
           ("message_stop", {"type": "message_stop"})]
    return _sse(ev)


def chat_web_response(model):
    r = chat_response(model)
    r["choices"][0]["message"]["content"] = "Python 3.15.0 is the latest."
    r["choices"][0]["message"]["annotations"] = [
        {"type": "url_citation", "url_citation": {"url": u, "title": ti, "start_index": 0, "end_index": 5}} for u, ti, _ in WEB_PAGES + WEB_PAGES[:1]]
    return r


def responses_web(model, status="completed"):
    r = responses_obj(model, status)
    msg = r["output"][0]
    msg["content"][0]["text"] = "Python 3.15.0 is the latest."
    msg["content"][0]["annotations"] = [{"type": "url_citation", "url": u, "title": ti, "start_index": 0, "end_index": 5}
                                        for u, ti, _ in WEB_PAGES + WEB_PAGES[:1]]
    r["output"] = [{"type": "web_search_call", "id": "ws_1", "status": "completed", "action": {"type": "search", "query": "latest python"}}, msg]
    return r


def responses_web_stream(model):
    done = responses_web(model)
    base = responses_obj(model, "in_progress", False)
    part = {"type": "output_text", "text": "", "annotations": []}
    msg_in = {"type": "message", "id": "msg_1", "status": "in_progress", "role": "assistant", "content": []}
    ws = done["output"][0]
    ev = [("response.created", {"type": "response.created", "sequence_number": 0, "response": base}),
          ("response.in_progress", {"type": "response.in_progress", "sequence_number": 1, "response": base}),
          ("response.output_item.added", {"type": "response.output_item.added", "sequence_number": 2, "output_index": 0, "item": {**ws, "status": "in_progress"}}),
          ("response.output_item.done", {"type": "response.output_item.done", "sequence_number": 3, "output_index": 0, "item": ws}),
          ("response.output_item.added", {"type": "response.output_item.added", "sequence_number": 4, "output_index": 1, "item": msg_in}),
          ("response.content_part.added", {"type": "response.content_part.added", "sequence_number": 5, "item_id": "msg_1", "output_index": 1, "content_index": 0, "part": part}),
          ("response.output_text.delta", {"type": "response.output_text.delta", "sequence_number": 6, "item_id": "msg_1", "output_index": 1, "content_index": 0, "delta": "Python 3.15.0 "}),
          ("response.output_text.delta", {"type": "response.output_text.delta", "sequence_number": 7, "item_id": "msg_1", "output_index": 1, "content_index": 0, "delta": "is the latest."}),
          ("response.output_text.done", {"type": "response.output_text.done", "sequence_number": 8, "item_id": "msg_1", "output_index": 1, "content_index": 0, "text": "Python 3.15.0 is the latest."}),
          ("response.output_item.done", {"type": "response.output_item.done", "sequence_number": 9, "output_index": 1, "item": done["output"][1]}),
          ("response.completed", {"type": "response.completed", "sequence_number": 10, "response": done})]
    return _sse(ev)


def chat_web_stream(model):
    def ch(delta, fin=None):
        return {"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": fin}]}
    ann = [{"type": "url_citation", "url_citation": {"url": u, "title": ti, "start_index": 0, "end_index": 5}} for u, ti, _ in WEB_PAGES]
    return _sse([(None, ch({"role": "assistant", "content": ""})), (None, ch({"content": "Python 3.15.0 "})),
                 (None, ch({"content": "is the latest.", "annotations": ann})), (None, ch({}, "stop")), (None, "[DONE]")])


def make_handler(state):
    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a): pass

        def _send(self, code, body, ctype="application/json", extra=None):
            if not isinstance(body, bytes): body = json.dumps(body).encode()
            if getattr(self, "_red", False): body = body.replace(b"Hello from mock", b"It is red")
            try:
                self.send_response(code)
                self.send_header("Content-Type", ctype); self.send_header("Content-Length", str(len(body)))
                for k, v in (extra or {}).items(): self.send_header(k, v)
                self.end_headers(); self.wfile.write(body)
            except OSError:
                pass  # the client gave up (timeout tests)

        def _slow_stream(self, model):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream"); self.send_header("Connection", "close")
            self.end_headers(); self.close_connection = True
            try:
                for i in range(100):
                    chunk = {"id": "c", "object": "chat.completion.chunk", "created": 1, "model": model,
                             "choices": [{"index": 0, "delta": {"content": f"w{i} "}, "finish_reason": None}]}
                    self.wfile.write(b"data: " + json.dumps(chunk).encode() + b"\n\n"); self.wfile.flush()
                    time.sleep(0.2)
                self.wfile.write(b"data: [DONE]\n\n"); self.wfile.flush()
            except OSError:
                pass

        def _slow_web_stream(self, model):
            """A Claude stream stuck in a server-side search: text, then the search call, then endless query pieces."""
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream"); self.send_header("Connection", "close")
            self.end_headers(); self.close_connection = True
            try:
                self.wfile.write(_sse(anthropic_web_stream(model, slow=True))); self.wfile.flush()
                for i in range(100):
                    piece = {"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": " "}}
                    self.wfile.write(b"event: content_block_delta\ndata: " + json.dumps(piece).encode() + b"\n\n"); self.wfile.flush()
                    time.sleep(0.2)
            except OSError:
                pass

        def do_GET(self):
            path = self.path.split("?")[0]; self._red = False
            with state.lock:
                state.requests.append({"path": path, "query": self.path.partition("?")[2], "method": "GET",
                                       "headers": {k.lower(): v for k, v in self.headers.items()}, "body": {}})
            if state.scenario in ("401", "403"):
                return self._send(int(state.scenario), {"error": {"message": f"mock {state.scenario}: access denied"}})
            if path.endswith("/models"):
                return self._send(200, {"data": [{"id": "model-a", "display_name": "Model A"}, {"id": "model-b"}]})
            self._send(404, {"error": {"message": f"no route {path}"}})

        def do_POST(self):
            raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            try: body = json.loads(raw or b"{}")
            except Exception: body = {"_raw": raw.decode(errors="replace")}
            path = self.path.split("?")[0]
            self._red = b"image/png" in raw   # image in the request: the mock "sees" red
            with state.lock:
                state.requests.append({"path": path, "query": self.path.partition("?")[2],
                                       "headers": {k.lower(): v for k, v in self.headers.items()}, "body": body})
                sc = state.scenario
                if sc == "429_then_ok" and state.fail_count < 2:
                    state.fail_count += 1; sc = "429"
            if sc in ("401", "403"):
                return self._send(int(sc), {"error": {"message": f"mock {sc}: access denied", "type": "auth_error", "code": sc}})
            if sc == "429":
                return self._send(429, {"error": {"message": "mock rate limited", "type": "rate_limit", "code": "429"}}, extra={"retry-after": "0"})
            if sc == "hang_then_ok":
                with state.lock:
                    hang = state.fail_count < 1
                    state.fail_count += 1
                sc = "hang" if hang else "ok"
            if sc == "hang":
                time.sleep(1.5)
            if path.endswith("/v1/tools/search"):
                return self._send(*ds_search(body, sc))
            if path.endswith("/v1/tools/fetch"):
                return self._send(*ds_fetch(body))
            model = body.get("model", "mock")
            stream = bool(body.get("stream"))
            if sc == "too_long":
                msg = ("prompt is too long: 300000 tokens > 200000 maximum" if path.endswith("/messages")
                       else "This model's maximum context length is 128000 tokens. However, your messages resulted in 300000 tokens.")
                return self._send(400, {"error": {"message": msg, "type": "invalid_request_error"}})
            length = sc == "length"
            if wants_web(body):
                if sc == "web_refused":
                    return self._send(400, web_error(path))
                if sc == "web_slow" and stream and path.endswith("/messages"):
                    return self._slow_web_stream(model)
                if sc == "web":
                    if path.endswith("/messages"):
                        return self._send(200, anthropic_web_stream(model), "text/event-stream") if stream else self._send(200, anthropic_web_response(model, body))
                    if path.endswith("/responses"):
                        return self._send(200, responses_web_stream(model), "text/event-stream") if stream else self._send(200, responses_web(model))
                    if path.endswith("/chat/completions"):
                        return self._send(200, chat_web_stream(model), "text/event-stream") if stream else self._send(200, chat_web_response(model))
            if path.endswith("/chat/completions") and ds_tools(body):
                if sc == "ds_no_required" and body.get("tool_choice") == "required":
                    return self._send(400, {"error": {"message": "tool_choice 'required' is not supported", "type": "invalid_request_error"}})
                return self._send(200, ds_chat(model, body, sc, stream), "text/event-stream") if stream else self._send(200, ds_chat(model, body, sc, False))
            if sc == "slow" and stream and path.endswith("/chat/completions"):
                return self._slow_stream(model)
            if path.endswith("/chat/completions"):
                fin = "length" if length else "stop"
                thought = 2 if body.get("reasoning_effort") else None
                return self._send(200, chat_stream(model, fin, bool((body.get("stream_options") or {}).get("include_usage")), thought), "text/event-stream") if stream else self._send(200, chat_response(model, fin, thought))
            if path.endswith("/responses"):
                st = "incomplete" if length else "completed"
                thought = 2 if body.get("reasoning") else None
                return self._send(200, responses_stream(model, st), "text/event-stream") if stream else self._send(200, responses_obj(model, st, reasoning=thought))
            if path.endswith("/messages"):
                stop = "max_tokens" if length else "end_turn"
                return self._send(200, anthropic_stream(model, stop), "text/event-stream") if stream else self._send(200, anthropic_response(model, stop))
            self._send(404, {"error": {"message": f"no route {path}"}})
    return H


def start():
    state = State()
    srv = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(state))
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, state, f"http://127.0.0.1:{srv.server_address[1]}"
