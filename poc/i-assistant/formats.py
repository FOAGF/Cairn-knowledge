"""The list formats and the app-side checks of the M6 templates (D-44), exactly as crates/cairn-core/prompts/README.md
specifies them ("How the app formats the list variables" and "Local checks"). The Rust core will implement the same
formats, so llm_check.py sends what the app will send and judges what the app will show.

Items are plain dicts, in the shape of samples.py: a task has key, title, category, status, priority, type, due, labels,
description, comments [(date, text)], parent, links; a page has title, category, group, body.
"""
import datetime as dt
import re

OUT_OF_SCOPE = ('Not searched, because outside this conversation\'s scope: {names}. Say so in the answer; the user can '
                'widen the search with "Search all categories".')
KINDS = ("question", "find", "create", "change", "general", "file_question", "file_draft")
STATUSES = ("todo", "in_progress", "blocked", "done", "wont_do")
PRIORITIES = ("high", "medium", "low")
TYPES = ("bug", "feature", "general")
LINK_KINDS = ("relates", "parent", "page")
OPS = ("create_task", "update_task", "comment", "create_page", "update_page")


# ------------------------------------------------------------------ the list variables

def today_line(day):
    """`today`: "2026-10-15 (Thursday)"."""
    return f"{day.isoformat()} ({day.strftime('%A')})"


def categories_lines(categories, in_scope):
    """`categories`: one line per category; in_scope is the list of in-scope names."""
    return "\n".join(f"- {p['name']} ({p['key']}): {'in scope' if p['name'] in in_scope else 'not in scope'}" for p in categories)


def scope_sentence(categories, in_scope):
    """`scope`: one sentence."""
    if len(in_scope) == len(categories):
        return "The conversation searches all categories."
    return "The conversation searches only these categories: " + ", ".join(in_scope) + "."


def strip_markers(text):
    """`[n]` markers removed from an earlier answer (history), with the space before them."""
    return re.sub(r" ?\[\d+(?:\s*,\s*\d+)*\]", "", text)


def history_text(turns, limit=6, cut=600):
    """`history` for ask-plan: the last `limit` turns as plain text, markers removed, each cut to `cut` characters."""
    lines = []
    for role, text in turns[-limit:]:
        t = strip_markers(text).strip()
        if len(t) > cut:
            t = t[:cut].rstrip() + " [...]"
        lines.append(f"{'User' if role == 'user' else 'Assistant'}: {t}")
    return "\n\n".join(lines)


def history_messages(turns):
    """What the app appends between the system and the user message of ask-answer, ask-action, ask-page and edit-page."""
    return [{"role": role, "content": strip_markers(text) if role == "assistant" else text} for role, text in turns]


def item_head(item):
    """The head of a task or page without the number or label."""
    if "key" in item:
        return (f"Task {item['key']} \"{item['title']}\" ({item['category']}; status: {item['status']}; "
                f"priority: {item['priority']}; due: {item.get('due') or 'none'})")
    return f"Page \"{item['title']}\" ({item['category']}; group: {item.get('group') or 'none'})"


def cited_lines(items):
    """`cited` for ask-plan: one line per item shown with the previous answer, in display order, labelled c1, c2, ..."""
    out = []
    for i, it in enumerate(items, 1):
        if "key" in it:
            out.append(f"- c{i}: task {it['key']} \"{it['title']}\" ({it['category']})")
        else:
            out.append(f"- c{i}: page \"{it['title']}\" ({it['category']})")
    return "\n".join(out)


def source_block(n, piece):
    """One source. piece: {"item": task|page|None, "part": ..., "text": ...}.
    A task piece has part = "description" or ("comment", date); a page piece has part = heading path (list) ;
    a file piece has item None and file, index, count."""
    item = piece.get("item")
    if item is None:
        head = f"[{n}] File \"{piece['file']}\" (attached to this message; part {piece['index']} of {piece['count']})"
        return f"{head}\n{piece['text']}"
    head = f"[{n}] {item_head(item)}"
    part = piece.get("part")
    if "key" in item:
        sub = "Description:" if part == "description" else f"Comment {part[1]}:"
    else:
        sub = "Section: " + (" > ".join(part) if part else "(top)")
    return f"{head}\n{sub}\n{piece['text']}"


def sources_block(pieces):
    """`sources`: numbered blocks separated by a blank line; "" when there are none."""
    return "\n\n".join(source_block(i, p) for i, p in enumerate(pieces, 1))


def out_of_scope_note(names):
    return OUT_OF_SCOPE.format(names=", ".join(names)) if names else ""


def refs_note(refs):
    """refs: [(source number, item)]."""
    if not refs:
        return ""
    parts = []
    for n, it in refs:
        name = f"task {it['key']}" if "key" in it else f"page \"{it['title']}\""
        parts.append(f"source {n} ({name})")
    return "The message refers to these earlier items: " + "; ".join(parts) + "."


def _start(text, cut=600):
    t = re.sub(r"\s+", " ", text).strip()
    return t if len(t) <= cut else t[:cut].rstrip() + " [...]"


def candidates_block(items):
    """`candidates` for ask-action: labelled blocks (t1.. tasks, p1.. pages) with the current fields and the start of the text.
    Returns (text, {label: item})."""
    blocks, labels, ti, pi = [], {}, 0, 0
    for it in items:
        if "key" in it:
            ti += 1
            label = f"t{ti}"
            head = (f"{label}: Task {it['key']} \"{it['title']}\" ({it['category']}; status: {it['status']}; priority: {it['priority']}; "
                    f"type: {it['type']}; due: {it.get('due') or 'none'}; labels: {', '.join(it.get('labels') or []) or 'none'}; "
                    f"parent: {it.get('parent') or 'none'}; links: {', '.join(it.get('links') or []) or 'none'})")
            text = _start(it.get("description") or "") or "(no description)"
        else:
            pi += 1
            label = f"p{pi}"
            head = f"{label}: Page \"{it['title']}\" ({it['category']}; group: {it.get('group') or 'none'})"
            text = _start(re.sub(r"^# .*\n", "", it["body"], count=1))
        labels[label] = it
        blocks.append(f"{head}\n{text}")
    return "\n\n".join(blocks), labels


# ------------------------------------------------------------------ app-side checks (README "Local checks")

MARKER = re.compile(r"\[(\d+(?:\s*,\s*\d+)*)\]")


def markers(text, count):
    """ask-answer: (text with invalid markers removed and "[1, 2]" split, the cited numbers in order of first use)."""
    cited = []

    def fix(m):
        nums = [int(x) for x in re.split(r"\s*,\s*", m.group(1))]
        keep = []
        for n in nums:
            if 1 <= n <= count:
                keep.append(n)
                if n not in cited:
                    cited.append(n)
        return " ".join(f"[{n}]" for n in keep)
    out = MARKER.sub(fix, text)
    out = re.sub(r"[ \t]+(?=[.,;:])", "", re.sub(r"[ \t]{2,}", " ", out))
    return out, cited


def parse_date(s):
    if not isinstance(s, str):
        return None
    try:
        return dt.date.fromisoformat(s.strip()).isoformat()
    except ValueError:
        return None


def _range(v):
    if not isinstance(v, dict):
        return None
    r = {"from": parse_date(v.get("from")), "to": parse_date(v.get("to"))}
    if r["from"] and r["to"] and r["from"] > r["to"]:
        r["from"], r["to"] = r["to"], r["from"]
    return r if r["from"] or r["to"] else None


def _enum_list(v, allowed):
    return [x for x in (v if isinstance(v, list) else []) if isinstance(x, str) and x in allowed]


def check_plan(obj, categories, in_scope, attachment=None):
    """The validated plan with the README's safe fallbacks. Returns (plan, notes) where notes say what was repaired."""
    notes = []
    obj = obj if isinstance(obj, dict) else {}
    kind = obj.get("kind")
    if kind not in KINDS:
        notes.append(f"kind {kind!r} -> question")
        kind = "question"
    if kind in ("file_question", "file_draft") and not attachment:
        notes.append(f"{kind} without a file -> question")
        kind = "question"
    if attachment and kind not in ("file_question", "file_draft"):
        notes.append(f"{kind} with a file -> file_question")   # the file stays a source; the kind only steers the search
        kind = "file_question"
    search = obj.get("search")
    search = " ".join(search.split()) if isinstance(search, str) else ""
    f = obj.get("filters") if isinstance(obj.get("filters"), dict) else {}
    by_lower = {p["name"].lower(): p["name"] for p in categories}
    names = []
    for n in f.get("categories") if isinstance(f.get("categories"), list) else []:
        canon = by_lower.get(str(n).lower())
        if canon is None:
            notes.append(f"unknown category {n!r} dropped")
        elif canon not in in_scope:
            notes.append(f"category {canon!r} is not in scope -> outside_scope")
        elif canon not in names:
            names.append(canon)
    outside = []
    for n in obj.get("outside_scope") if isinstance(obj.get("outside_scope"), list) else []:
        canon = by_lower.get(str(n).lower())
        if canon and canon not in in_scope and canon not in outside:
            outside.append(canon)
        elif canon is None:
            notes.append(f"unknown outside_scope category {n!r} dropped")
    for n in [by_lower.get(str(n).lower()) for n in (f.get("categories") or []) if isinstance(f.get("categories"), list)]:
        if n and n not in in_scope and n not in outside:
            outside.append(n)
    filters = {"categories": names, "types": _enum_list(f.get("types"), ("task", "page")),
               "statuses": _enum_list(f.get("statuses"), STATUSES), "priorities": _enum_list(f.get("priorities"), PRIORITIES),
               "due": _range(f.get("due")), "updated": _range(f.get("updated"))}
    refs = [r for r in (obj.get("refs") if isinstance(obj.get("refs"), list) else []) if isinstance(r, str) and re.fullmatch(r"c\d+", r)]
    intro = obj.get("intro") if isinstance(obj.get("intro"), str) and kind == "find" else ""
    return {"kind": kind, "search": search, "filters": filters, "refs": refs, "outside_scope": outside, "intro": intro.strip()}, notes


def check_action(obj, labels, categories, in_scope):
    """The validated proposal. Returns (changes kept, dropped as (change, reason), reply)."""
    obj = obj if isinstance(obj, dict) else {}
    kept, dropped = [], []
    reply = obj.get("reply") if isinstance(obj.get("reply"), str) else ""
    by_lower = {p["name"].lower(): p["name"] for p in categories}
    seen = set()

    def links_of(c):
        out = []
        for l in c.get("links") if isinstance(c.get("links"), list) else []:
            if isinstance(l, dict) and l.get("to") in labels and l.get("kind") in LINK_KINDS:
                is_page = str(l["to"]).startswith("p")
                if (l["kind"] == "page") == is_page and l.get("to") != c.get("target"):
                    out.append({"to": l["to"], "kind": l["kind"]})
        return out

    for c in obj.get("changes") if isinstance(obj.get("changes"), list) else []:
        if not isinstance(c, dict) or c.get("op") not in OPS:
            dropped.append((c, "unknown op"))
            continue
        op = c["op"]
        if op in ("update_task", "comment", "update_page"):
            t = c.get("target")
            if t not in labels:
                dropped.append((c, f"unknown target {t!r}"))
                continue
            if (op == "update_page") != t.startswith("p"):
                dropped.append((c, f"target {t} is the wrong type for {op}"))
                continue
        if op in ("create_task", "create_page"):
            category = by_lower.get(str(c.get("category", "")).lower())
            if category is None:
                category = in_scope[0]   # the user can change it on the card
                dropped.append((None, f"category {c.get('category')!r} unknown -> {category}"))
            title = str(c.get("title") or "").strip()
            if not title:
                dropped.append((c, "no title"))
                continue
        if op == "create_task":
            kept.append({"op": op, "category": category, "title": title,
                         "type": c.get("type") if c.get("type") in TYPES else "general",
                         "priority": c.get("priority") if c.get("priority") in PRIORITIES else "medium",
                         "due": parse_date(c.get("due")), "labels": [str(x).strip() for x in (c.get("labels") or []) if str(x).strip()],
                         "description": str(c.get("description") or "").strip(), "links": links_of(c),
                         "terms": c.get("terms") if isinstance(c.get("terms"), list) else []})
        elif op == "update_task":
            u = {"op": op, "target": c["target"]}
            if c.get("status") in STATUSES:
                u["status"] = c["status"]
            if c.get("priority") in PRIORITIES:
                u["priority"] = c["priority"]
            if c.get("type") in TYPES:
                u["type"] = c["type"]
            if "due" in c:
                d = parse_date(c["due"])
                if d or c["due"] is None:
                    u["due"] = d
            if isinstance(c.get("labels"), list):
                u["labels"] = [str(x).strip() for x in c["labels"] if str(x).strip()]
            if isinstance(c.get("description"), str):
                u["description"] = c["description"]
            links = links_of(c)
            if links:
                u["links"] = links
            if len(u) == 2:
                dropped.append((c, "changes nothing"))
                continue
            key = (c["target"], tuple(sorted(u)))
            if key in seen:
                dropped.append((c, "duplicate"))
                continue
            seen.add(key)
            kept.append(u)
        elif op == "comment":
            text = str(c.get("text") or "").strip()
            if not text:
                dropped.append((c, "empty comment"))
                continue
            kept.append({"op": op, "target": c["target"], "text": text})
        elif op == "create_page":
            kept.append({"op": op, "category": category, "title": title, "content": str(c.get("content") or "").strip()})
        elif op == "update_page":
            change = str(c.get("change") or "").strip()
            if not change:
                dropped.append((c, "empty change"))
                continue
            kept.append({"op": op, "target": c["target"], "change": change})
    return kept, dropped, reply.strip()
