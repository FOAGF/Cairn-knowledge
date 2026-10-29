"""Local mock of OpenAI Chat Completions, OpenAI Responses and Anthropic Messages.
Records every request. Scenarios: 'ok' (default), '401', '403', '429_then_ok'."""
import json, threading
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


def chat_response(model):
    return {"id": "chatcmpl-1", "object": "chat.completion", "created": 1, "model": model,
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hello from mock"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}}


def chat_stream(model):
    def ch(delta, fin=None):
        return {"id": "chatcmpl-1", "object": "chat.completion.chunk", "created": 1, "model": model,
                "choices": [{"index": 0, "delta": delta, "finish_reason": fin}]}
    return _sse([(None, ch({"role": "assistant", "content": ""})), (None, ch({"content": "Hello "})),
                 (None, ch({"content": "from mock"})), (None, ch({}, "stop")), (None, "[DONE]")])


def responses_obj(model, status="completed", with_output=True):
    return {"id": "resp_1", "object": "response", "created_at": 1, "status": status, "model": model,
            "output": [{"type": "message", "id": "msg_1", "status": "completed", "role": "assistant",
                        "content": [{"type": "output_text", "text": "Hello from mock", "annotations": []}]}] if with_output else [],
            "usage": {"input_tokens": 5, "output_tokens": 3, "total_tokens": 8},
            "parallel_tool_calls": True, "tool_choice": "auto", "tools": []}


def responses_stream(model):
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
    done = responses_obj(model)
    ev.append(("response.output_item.done", {"type": "response.output_item.done", "sequence_number": n, "output_index": 0, "item": done["output"][0]})); n += 1
    ev.append(("response.completed", {"type": "response.completed", "sequence_number": n, "response": done}))
    return _sse(ev)


def anthropic_response(model):
    return {"id": "msg_1", "type": "message", "role": "assistant", "model": model,
            "content": [{"type": "text", "text": "Hello from mock"}], "stop_reason": "end_turn", "stop_sequence": None,
            "usage": {"input_tokens": 5, "output_tokens": 3}}


def anthropic_stream(model):
    return _sse([
        ("message_start", {"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant", "model": model, "content": [], "stop_reason": None, "usage": {"input_tokens": 5, "output_tokens": 1}}}),
        ("content_block_start", {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hello "}}),
        ("content_block_delta", {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "from mock"}}),
        ("content_block_stop", {"type": "content_block_stop", "index": 0}),
        ("message_delta", {"type": "message_delta", "delta": {"stop_reason": "end_turn", "stop_sequence": None}, "usage": {"output_tokens": 3}}),
        ("message_stop", {"type": "message_stop"})])


def make_handler(state):
    class H(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *a): pass

        def _send(self, code, body, ctype="application/json", extra=None):
            if not isinstance(body, bytes): body = json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", ctype); self.send_header("Content-Length", str(len(body)))
            for k, v in (extra or {}).items(): self.send_header(k, v)
            self.end_headers(); self.wfile.write(body)

        def do_POST(self):
            raw = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            try: body = json.loads(raw or b"{}")
            except Exception: body = {"_raw": raw.decode(errors="replace")}
            path = self.path.split("?")[0]
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
            model = body.get("model", "mock")
            stream = bool(body.get("stream"))
            if path.endswith("/chat/completions"):
                return self._send(200, chat_stream(model), "text/event-stream") if stream else self._send(200, chat_response(model))
            if path.endswith("/responses"):
                return self._send(200, responses_stream(model), "text/event-stream") if stream else self._send(200, responses_obj(model))
            if path.endswith("/messages"):
                return self._send(200, anthropic_stream(model), "text/event-stream") if stream else self._send(200, anthropic_response(model))
            self._send(404, {"error": {"message": f"no route {path}"}})
    return H


def start():
    state = State()
    srv = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(state))
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, state, f"http://127.0.0.1:{srv.server_address[1]}"
