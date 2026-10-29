"""A tiny stand-in for the real sidecar (sidecar/PROTOCOL.md), for the Rust tests. Plain python3, no deps.

Usage: python3 fake_sidecar.py --role ai|embed

Methods: ping, shutdown, echo, env, stream, slow, cancelled, crash, embed.query, embed.documents,
embed.info, and (M5, for the chat jobs in src-tauri/src/commands/files.rs):
- file.extract: the file at `path` holds a JSON spec {"parts": [{"page": 1, "text": "..."} |
  {"page": 2, "image": true}]} (a .md/.txt file is read as text); image parts get a JPEG-named file
  in `out_dir`. The text "BADFILE" in the file fails with the kind bad_file.
- llm.complete: canned replies by what the messages look like (images = OCR; an assistant message =
  a revision; ", part i of n" = a part draft; "drafted in n parts" = the combine call; else a
  file draft),
  `stream` notifications when asked, `finish_reason`. Markers in the messages: "SLOWSTREAM" (many
  slow chunks), "CUTOFF" (finish_reason length), "ANSWERONLY" in a revision request (a plain answer).
  A call whose messages hold more than 10000 characters fails with the kind too_long.
- llm.model_info: max_input_tokens 3000 for the model "ctx-small", else null.
- calls: what llm.complete and file.extract were called with, in order.
Requests run in their own threads; the process exits when stdin closes.
"""
import base64
import hashlib
import json
import math
import os
import struct
import sys
import re
import threading
import time

out_lock = threading.Lock()
cancelled = set()
call_log = []
TOO_LONG_CHARS = 10000
role = sys.argv[sys.argv.index("--role") + 1] if "--role" in sys.argv else "?"


def send(obj):
    with out_lock:
        sys.stdout.write(json.dumps(obj) + "\n")
        sys.stdout.flush()


def vector(text, dims):
    """Deterministic, normalised, from the SHA-256 of the text."""
    raw = []
    block = 0
    while len(raw) < dims:
        digest = hashlib.sha256(f"{block}:{text}".encode()).digest()
        raw.extend((b - 127.5) / 127.5 for b in digest)
        block += 1
    raw = raw[:dims]
    norm = math.sqrt(sum(x * x for x in raw)) or 1.0
    return base64.b64encode(struct.pack(f"<{dims}f", *[x / norm for x in raw])).decode()


def text_of(content):
    if isinstance(content, str):
        return content
    return "\n".join(b.get("text", "") for b in content if b.get("type") == "text")


def error(rid, kind, message):
    send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32000, "message": message, "data": {"kind": kind}}})


def extract(rid, p):
    with open(p["path"], encoding="utf-8") as f:
        raw = f.read()
    call_log.append({"method": "file.extract", "path": os.path.basename(p["path"])})
    if "BADFILE" in raw:
        return error(rid, "bad_file", "The file is damaged.")
    ext = p["path"].rsplit(".", 1)[-1].lower()
    if ext in ("md", "txt"):
        return {"kind": "text", "pages": 1, "parts": [{"page": 1, "text": raw}]}
    spec = json.loads(raw)
    os.makedirs(p["out_dir"], exist_ok=True)
    parts = []
    for i, part in enumerate(spec["parts"], start=1):
        if part.get("image"):
            path = os.path.join(p["out_dir"], f"p{i:04d}.jpg")
            with open(path, "wb") as f:
                f.write(b"\xff\xd8fake")
            parts.append({"page": i, "image_path": path, "width": 10, "height": 10})
        else:
            parts.append({"page": i, "text": part["text"]})
    return {"kind": "pdf" if ext == "pdf" else ext, "pages": len(parts), "parts": parts}


# ---- M6 actions block: ask-action, ask-page, edit-page (src-tauri/src/commands/actions/tests.rs) ----
# The request line ("Request: ..." / "Change to make: ...") picks the reply by keyword:
#   ask-action: UNREADABLE (never JSON), RETRYJSON (garbage until the check-retry message),
#   CREATE (a task with a link and a glossary term), THREE (three updates), COMMENT, AMBIGUOUS
#   (no changes, a question), NEWPAGE, PAGEEDIT, MIXED (a create and a page edit), DELETE,
#   NOOP (a change that changes nothing), FROMSOURCE (a task whose description is the last line of the
#   <sources> block, D-83).
#   Tasks are t1, t2, t3 and the page p1, as the tests list the candidates.
#   edit-page: "CANNOT" in the change gives a plain answer, "SAME" gives the page unchanged,
#   else the page with a line added. ask-page: a page titled by the "Title" line, else "Generated page".
def actions_reply(messages, texts):
    system, user = texts[0], texts[-1]
    if "You propose the changes; the app shows them" in system:
        asked = next((t for t in reversed(texts) if "Request: " in t), user)
        request = (re.search(r"Request: (.*)", asked) or [None, ""])[1]
        user = asked
        if "UNREADABLE" in request:
            return "I would rather talk about it.", "ask-action"
        if "RETRYJSON" in request and "That was not a valid JSON object" not in texts[-1]:
            return "Here you go: {changes: oops", "ask-action"
        category = (re.search(r"^- (.*) \([A-Z0-9]+\): in scope", user, re.M) or [None, "General"])[1]
        changes, reply = [], "Here is what I would change."
        if "CREATE" in request:
            changes.append({"op": "create_task", "category": category, "title": "Order a new auger hose",
                            "type": "bug", "priority": "high", "due": "2026-10-30", "labels": ["hose"],
                            "description": "The MMU hose split. Order two metres.",
                            "links": [{"to": "t1", "kind": "relates"}, {"to": "p1", "kind": "page"}],
                            "terms": [{"term": "MMU", "meaning": "Mobile mixing unit", "confidence": "high"}]})
        if "FROMSOURCE" in request:
            block = (re.search(r"<sources>\n(.*?)\n?</sources>", user, re.S) or [None, ""])[1]
            last = [l for l in block.splitlines() if l.strip()][-1:] or ["(no sources)"]
            changes.append({"op": "create_task", "category": category, "title": "From a comment", "type": "general",
                            "priority": "medium", "due": None, "labels": [], "description": last[0],
                            "links": [{"to": "t1", "kind": "relates"}], "terms": []})
        if "THREE" in request:
            changes += [{"op": "update_task", "target": "t1", "status": "done"},
                        {"op": "update_task", "target": "t2", "priority": "high"},
                        {"op": "update_task", "target": "t3", "due": "2026-11-02", "labels": ["site"]}]
            reply = "Updating three tasks."
        if "NOOP" in request:  # t1 is already to do: the change is dropped
            changes.append({"op": "update_task", "target": "t1", "status": "todo"})
            reply = "Setting it to to do."
        if "COMMENT" in request:
            changes.append({"op": "comment", "target": "t1", "text": "Replaced the hose on site."})
            reply = "Adding a comment."
        if "AMBIGUOUS" in request:
            reply = "Which task do you mean?"
        if "DELETE" in request:
            reply = "The chat cannot delete a task; open it and delete it there."
        if "NEWPAGE" in request:
            changes.append({"op": "create_page", "category": category, "title": "Hose guide",
                            "content": "How to replace the auger hose."})
            reply = "Writing a page."
        if "PAGEEDIT" in request or "MIXED" in request:
            change = "CANNOT say" if "CANNOT" in request else ("SAME" if "SAME" in request else "Add a step: check the seals.")
            changes.append({"op": "update_page", "target": "p1", "change": change})
            reply = "Changing the page."
        return json.dumps({"changes": changes, "reply": reply}), "ask-action"
    if system.startswith("You write one new page"):
        title = (re.search(r"Title, when one was chosen: (.*)", user) or [None, ""])[1].strip()
        return "# " + (title or "Generated page") + "\n\nFrom the sources. [1]", "ask-page"
    if system.startswith("You change one existing page"):
        page = re.search(r"<page>\n(.*?)\n</page>", user, re.S).group(1)
        change = (re.search(r"Change to make: (.*)", user) or [None, ""])[1]
        if "CANNOT" in change:
            return "The page does not say what is missing.", "edit-page"
        if "SAME" in change:
            return page, "edit-page"
        return page + "\n\n- " + change, "edit-page"
    return None


# ---- M6 answers agent: the assistant's calls (src-tauri/src/commands/assistant.rs) ----------------
# Told apart by the system message. Markers in the user's message steer them:
# - ask-plan: kind question by default, the search words are the message without markers;
#   FINDME (find; the words after it are the search), GENERALQ, CREATEIT, CHANGEIT, FILEDRAFT,
#   STATUS_TODO (a status filter), OUTSIDE:Name, REF1 (refs ["c1"]); BADPLAN (never JSON),
#   PLANRETRY (not JSON until it is asked again).
# - ask-answer: cites [1] and the invalid [99] when there are sources, says "Your notes don't
#   cover this." without; TWOCITE cites [2, 1]; NOCITE cites nothing. It reports the number of
#   sources and of earlier turns.
# - doc-terms: one term in the document ("Zorp") and one that is not; BADTERMS (never JSON).
# - ask-web: the `web` evidence by marker: WEBSEARCHED (a search with two sources), WEBREFUSE
#   (refused with web_search_unsupported while web_search is on), otherwise no search was made.
# The call log entries of these calls hold `last` (the last message), `n_messages`, `effort` and
# `web_search`.
ASSISTANT_KINDS = ("ask-plan", "ask-answer", "doc-terms", "ask-web")


def assistant_reply(messages):
    texts = [text_of(m["content"]) for m in messages]
    system = texts[0] if messages[0]["role"] == "system" else ""
    everything = "\n".join(texts)
    if "first step of the assistant" in system:
        m = (re.search(r"<message>\n(.*?)\n</message>", everything, re.S) or [None, ""])[1]
        retried = any(x["role"] == "assistant" for x in messages)
        if "BADPLAN" in m or ("PLANRETRY" in m and not retried):
            return "I would rather chat.", "ask-plan"
        plan = {"kind": "question", "search": "", "filters": {}, "refs": [], "outside_scope": [], "intro": ""}
        words = m
        for marker, kind in (("FINDME", "find"), ("GENERALQ", "general"), ("CREATEIT", "create"),
                             ("CHANGEIT", "change"), ("FILEDRAFT", "file_draft")):
            if marker in m:
                plan["kind"] = kind
                words = m.split(marker, 1)[1] if marker == "FINDME" else m.replace(marker, "")
        if plan["kind"] == "find":
            plan["intro"] = "Here is what I found:"
        if "STATUS_TODO" in m:
            plan["filters"]["statuses"] = ["todo"]
            words = words.replace("STATUS_TODO", "")
        if "REF1" in m:
            plan["refs"] = ["c1"]
            words = words.replace("REF1", "")
        out = re.search(r"OUTSIDE:(\w[\w ]*)", words)
        if out:
            plan["outside_scope"] = [out.group(1).strip()]
            words = words.replace(out.group(0), "")
        plan["search"] = " ".join(re.sub(r"[^\w\- ]", " ", words).split()) if plan["kind"] != "general" else ""
        return json.dumps(plan), "ask-plan"
    if "answer the user's question from the numbered sources" in system:
        question = texts[-1]
        sources = len(re.findall(r"^\[\d+\] ", question, re.M))
        earlier = len(messages) - 2
        if "NOCITE" in question:
            body = "Nothing to cite."
        elif sources == 0:
            body = "Your notes don't cover this."
            if "GENERALQ" in question:
                body = "General knowledge, not from your notes: the sky is blue."
        elif "TWOCITE" in question and sources >= 2:
            body = "Both matter [2, 1]."
        else:
            body = "It is in the notes [1] and not in [99]."
        return f"{body} Sources: {sources}. Earlier turns: {earlier}.", "ask-answer"
    if "look for possible glossary terms in a document" in system:
        if "BADTERMS" in everything:
            return "no terms here", "doc-terms"
        terms = [{"term": "Zorp", "meaning": "a made-up unit", "confidence": "high"},
                 {"term": "Absent", "meaning": "not in the text", "confidence": "high"}]
        return json.dumps({"terms": terms}), "doc-terms"
    if system.startswith("You answer the user's message."):
        return "Web answer.", "ask-web"
    if len(messages) >= 2 and texts[0].startswith("That was not a valid JSON object"):
        return "{}", "check-retry"
    return None


def assistant_web(p, everything):
    """The `web` evidence of an ask-web call (None when web search was not asked for) and the
    refusal message, if the call is refused."""
    if not p.get("web_search"):
        return None, None
    if "WEBREFUSE" in everything:
        return None, "The request names web_search_options, which this model does not accept."
    if "WEBSEARCHED" in everything:
        sources = [{"url": "https://example.com/a", "title": "Page A", "cited_text": "the quoted text"},
                   {"url": "https://example.com/b", "title": "Page B", "cited_text": ""}]
        return {"searches": 1, "sources": sources}, None
    return {"searches": 0, "sources": []}, None


def reply_for(messages):
    """The canned reply for a call, and the kind of call it was."""
    assistant = assistant_reply(messages)
    if assistant:
        return assistant
    texts = [text_of(m["content"]) for m in messages]
    if messages and messages[0]["role"] == "system":
        found = actions_reply(messages, texts)
        if found is not None:
            return found
    last = messages[-1]
    if isinstance(last["content"], list) and any(b.get("type") == "image" for b in last["content"]):
        paths = [b["path"] for b in last["content"] if b.get("type") == "image"]
        for path in paths:
            if not os.path.exists(path):
                return "missing image " + path, "ocr-missing"
        return "\n\n[page break]\n\n".join("Scanned text of " + os.path.basename(x) for x in paths), "ocr"
    if any(m["role"] == "assistant" for m in messages):
        request = texts[-1]
        if "ANSWERONLY" in request:
            return "Sure, the draft covers " + str(len(messages)) + " messages so far.", "revise"
        return "# Revised: " + request + "\n\nRevised body.", "revise"
    user = texts[-1]
    name = (re.search(r"Source file: ([^,\n]*)", user) or [None, "file"])[1].strip()
    if re.search(r"drafted in \d+ parts", user):
        return "# Combined " + name + "\n\n" + str(user.count("<part>")) + " parts combined.", "combine"
    m = re.search(r", part (\d+) of (\d+)", user)
    if m and "<source>" in user:
        return "# Part " + m.group(1) + "\n\n- facts of part " + m.group(1), "part"
    instruction = (re.search(r"Instruction: (.*)", user) or [None, ""])[1].strip()
    return "# Draft of " + name + "\n\nInstruction: " + instruction + "\nSource chars: " + str(len(user)), "draft"


def complete(rid, p):
    messages = p["messages"]
    total = sum(len(text_of(m["content"])) for m in messages)
    reply, kind = reply_for(messages)
    entry = {"method": "llm.complete", "kind": kind, "chars": total, "stream": bool(p.get("stream")),
             "model": p["target"].get("model")}
    everything = "\n".join(text_of(m["content"]) for m in messages)
    web = None
    if kind in ASSISTANT_KINDS:
        entry.update({"last": text_of(messages[-1]["content"]), "n_messages": len(messages),
                      "effort": p.get("effort"), "web_search": p.get("web_search")})
    call_log.append(entry)
    if total > TOO_LONG_CHARS:
        return error(rid, "too_long", "The input is too long for this model.")
    if kind == "ask-web":
        web, refusal = assistant_web(p, everything)
        if refusal:
            return error(rid, "web_search_unsupported", refusal)
    finish = "length" if "CUTOFF" in everything else "stop"
    if p.get("stream"):
        slow = "SLOWSTREAM" in everything
        chunks = [reply[i:i + 8] for i in range(0, len(reply), 8)] if not slow else [reply[:4]] + ["."] * 400
        sent = ""
        for chunk in chunks:
            if rid in cancelled:
                return {"text": sent, "model": "fake", "usage": None, "finish_reason": "cancelled"}
            send({"jsonrpc": "2.0", "method": "stream", "params": {"id": rid, "delta": chunk}})
            sent += chunk
            time.sleep(0.05 if slow else 0)
        reply = sent if slow else reply
    usage = {"input_tokens": 1, "output_tokens": 1}
    # D-65: a call that sends an effort level reports some reasoning. The level is decided as the real
    # worker does (resolve_level in cairn_sidecar/ai.py): only when effort is on for the model.
    eff = (p.get("target") or {}).get("effort") or {}
    if eff.get("enabled") and (p.get("effort") or eff.get("default_level")):
        usage["reasoning_tokens"] = 10
    result = {"text": reply, "model": "fake", "usage": usage, "finish_reason": finish}
    if web is not None:
        result["web"] = web
    return result


def handle(req):
    rid, method, p = req.get("id"), req.get("method"), req.get("params") or {}
    try:
        if method == "ping":
            result = {"pong": True, "role": role, "pid": os.getpid(), "python": "fake", "version": "0.0"}
        elif method == "shutdown":
            send({"jsonrpc": "2.0", "id": rid, "result": {}})
            os._exit(0)
        elif method == "echo":
            result = {"text": p.get("text"), "pid": os.getpid()}
        elif method == "env":
            keys = ["AZURE_OPENAI_API_KEY", "AZURE_API_KEY", "AZURE_AI_API_KEY"]
            result = {
                "azure_present": [k for k in keys if k in os.environ],
                "cost_map": os.environ.get("LITELLM_LOCAL_MODEL_COST_MAP"),
                "log": os.environ.get("LITELLM_LOG"),
                "path_set": "PATH" in os.environ,
            }
        elif method == "stream":
            n = 0
            for part in p.get("parts", []):
                if rid in cancelled:
                    break
                send({"jsonrpc": "2.0", "method": "stream", "params": {"id": rid, "delta": part}})
                n += 1
                time.sleep(p.get("delay_ms", 0) / 1000)
            result = {"text": "".join(p.get("parts", [])), "sent": n, "stopped": rid in cancelled}
        elif method == "slow":
            end = time.time() + p.get("ms", 100) / 1000
            while time.time() < end and rid not in cancelled:
                time.sleep(0.005)
            result = {"cancelled": rid in cancelled}
        elif method == "cancelled":
            result = {"ids": sorted(cancelled)}
        elif method == "file.extract":
            result = extract(rid, p)
            if result is None:
                return
        elif method == "llm.complete":
            result = complete(rid, p)
            if result is None:
                return
        elif method == "llm.model_info":
            result = {"max_input_tokens": 3000 if p["target"].get("model") == "ctx-small" else None,
                      "max_output_tokens": None}
        elif method == "calls":
            result = {"calls": call_log}
        elif method == "crash":
            os._exit(3)
        elif method == "embed.query":
            result = {"vector": vector("q:" + p["text"], p["dims"])}
        elif method == "embed.documents":
            result = {"vectors": [vector(f"d:{i['title']}|{i['text']}", p["dims"]) for i in p["items"]]}
        elif method == "embed.info":
            result = {"loaded": True, "model": "fake", "dims": p["dims"], "load_ms": 1}
        else:
            send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": f"no method {method}"}})
            return
        send({"jsonrpc": "2.0", "id": rid, "result": result})
    except KeyError as e:
        send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32602, "message": f"missing {e}",
                                                       "data": {"kind": "bad_request", "hint": "check params"}}})


for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    if msg.get("method") == "cancel":
        cancelled.add(msg["params"]["id"])
    elif "id" in msg:
        threading.Thread(target=handle, args=(msg,), daemon=True).start()
# stdin closed: exit with code 0 (PROTOCOL.md "Lifetime")
