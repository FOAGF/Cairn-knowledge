"""The M5 prompts under test (D-41), loaded from the app's template files in crates/cairn-core/prompts/ and
rendered by the rules in its README, so llm_check.py sends exactly what the app will send.

Each builder returns `messages` for the sidecar's `llm.complete` (sidecar/PROTOCOL.md). The list variables are
formatted here as the README says the app does. `glossary_block`, `occurs` and `filter_terms` are the app-side
helpers the README describes (the glossary matcher of FR-KN-GL-4 and the local term filter of FR-KN-GL-7).
"""
import json, os, re

PROMPTS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "crates", "cairn-core", "prompts")
EXCERPT_NOTE = ('These are excerpts of a longer text, picked by meaning for the instruction or request because the whole '
                'text is too long to send; "[...]" marks where text was left out. Work from the excerpts only.')

# ------------------------------------------------------------------ the renderer (README: format, rendering rules)

_INCLUDE = re.compile(r"\{\{>\s*([\w-]+)\s*\}\}")
_PLACEHOLDER = re.compile(r"\{\{\s*([\w-]+)\s*\}\}")
_MARKER = re.compile(r"^<<<(system|user)>>>[ \t]*$", re.M)
_cache = {}


def _front_matter(text):
    """The front matter's keys; `variables` becomes {name: meaning}. A YAML subset: scalars and one list of `name: text`."""
    meta, variables = {}, {}
    for line in text.splitlines():
        if line.startswith("  - "):
            name, _, meaning = line[4:].partition(":")
            variables[name.strip()] = meaning.strip()
        elif line.strip():
            key, _, value = line.partition(":")
            meta[key.strip()] = value.strip()
    meta["variables"] = {} if meta.get("variables") == "[]" else variables
    return meta


def load(tid):
    """A template file -> {"meta": {...}, "sections": [(role, text)]}, with its fragments expanded."""
    if tid in _cache:
        return _cache[tid]
    raw = open(os.path.join(PROMPTS, f"{tid}.md"), encoding="utf-8").read()
    m = re.match(r"---\n(.*?)\n---\n(.*)\Z", raw, re.S)
    if not m:
        raise ValueError(f"{tid}: no front matter")
    meta = _front_matter(m.group(1))
    body = _INCLUDE.sub(lambda mm: open(os.path.join(PROMPTS, f"_{mm.group(1)}.md"), encoding="utf-8").read(), m.group(2))
    pieces = _MARKER.split(body)
    if pieces[0].strip():
        raise ValueError(f"{tid}: text before the first message marker")
    sections = [(pieces[i], pieces[i + 1].strip("\n")) for i in range(1, len(pieces), 2)]
    used = {p for _, text in sections for p in _PLACEHOLDER.findall(text)}
    undeclared = used - set(meta["variables"])
    if undeclared:
        raise ValueError(f"{tid}: placeholders not declared in `variables`: {sorted(undeclared)}")
    _cache[tid] = {"meta": meta, "sections": sections}
    return _cache[tid]


def render(tid, **values):
    """The template's messages with the values inserted in one pass (values are not scanned for placeholders)."""
    t = load(tid)
    declared = set(t["meta"]["variables"])
    if set(values) != declared:
        raise ValueError(f"{tid}: expected variables {sorted(declared)}, got {sorted(values)}")
    return [{"role": role, "content": _PLACEHOLDER.sub(lambda m: str(values[m.group(1)]), text).strip("\n")}
            for role, text in t["sections"]]


# ------------------------------------------------------------------ glossary (FR-KN-GL-4, FR-LLM-AS-8)

def occurs(name, text):
    """FR-KN-GL-4: whole words; an all-capital name matches only in capitals; codes and plurals (`MMU-07`, `MMUs`) count;
    a name with Chinese characters matches anywhere."""
    if re.search(r"[一-鿿]", name):
        return name in text
    flags = 0 if name.isupper() else re.IGNORECASE
    return re.search(rf"(?<!\w){re.escape(name)}(?:s|-\w+)?(?!\w)", text, flags) is not None


def glossary_block(entries, *texts):
    """The `glossary` variable: the entries whose term or other names occur in the texts, plus the entries related to those
    (one step, FR-KN-GL-3), one line each as the README formats them. "" when none match."""
    blob = "\n".join(texts)
    hit = [e for e in entries if any(occurs(n, blob) for n in [e["term"], *e["other"]])]
    related = [e for e in entries if e not in hit and any(occurs(h["term"], e["desc"]) or occurs(e["term"], h["desc"]) for h in hit)]
    lines = []
    for e in hit + related:
        other = f" ({', '.join(e['other'])})" if e["other"] else ""
        scope = "global" if e["scope"] == "global" else f"category: {e['scope']}"
        lines.append(f"- {e['term']}{other} [{scope}]: {e['desc']}")
    return "\n".join(lines)


# ------------------------------------------------------------------ the list variables (README: how the app formats them)

def groups_lines(groups):
    return "\n".join(f"- {g['name']}: {g['desc']}" if g.get("desc") else f"- {g['name']}" for g in groups)


def comments_lines(comments):
    return "\n".join(f"[{d}] {t}" for d, t in comments)


def topics_lines(topics):
    return "\n".join(f"- {t}" for t in topics)


def parts_block(drafts):
    n = len(drafts)
    return "\n\n".join(f"Part {i} of {n}:\n<part>\n{d}\n</part>" for i, d in enumerate(drafts, 1))


def pages_lines(pages):
    """pages: [(id, title, body)]; the start of the body on one line."""
    lines = []
    for pid, title, body in pages:
        start = re.sub(r"\s+", " ", body)[:300].strip()
        lines.append(f"- [{pid}] {title}: {start}")
    return "\n".join(lines)


# ------------------------------------------------------------------ the builders, one per template

def ocr_messages(data_url):
    msgs = render("ocr")
    msgs[-1]["content"] = [{"type": "text", "text": msgs[-1]["content"]}, {"type": "image", "data_url": data_url}]
    return msgs


def draft_messages(file_name, text, instruction, glossary, excerpts=False):
    return render("draft-file", file_name=file_name, source=text, instruction=instruction.strip(),
                  excerpt_note=EXCERPT_NOTE if excerpts else "", glossary=glossary)


def draft_part_messages(file_name, text, index, count, glossary):
    return render("draft-part", file_name=file_name, source=text, part_index=index, part_count=count, glossary=glossary)


def draft_combine_messages(file_name, part_drafts, glossary):
    return render("draft-combine", file_name=file_name, part_count=len(part_drafts), parts=parts_block(part_drafts), glossary=glossary)


def revise_messages(file_name, text, instruction, glossary, draft, request, earlier=(), excerpts=False):
    """earlier: the chat turns about this draft so far, as (role, text) pairs."""
    msgs = render("revise", file_name=file_name, source=text, instruction=instruction.strip(),
                  excerpt_note=EXCERPT_NOTE if excerpts else "", glossary=glossary)
    msgs.append({"role": "assistant", "content": draft})
    msgs += [{"role": r, "content": t} for r, t in earlier]
    msgs.append({"role": "user", "content": request})
    return msgs


def task_messages(task, glossary):
    return render("task-draft", task_key=task["key"], task_title=task["title"], category=task["category"],
                  description=task["description"], comments=comments_lines(task["comments"]), glossary=glossary)


def check_messages(category, groups, glossary, page_text):
    return render("check", category=category, groups=groups_lines(groups), glossary=glossary, page=page_text)


def check_retry_messages():
    return render("check-retry")


def keep_one_topic_messages(topic, topics, draft, glossary):
    return render("keep-one-topic", topic=topic, topics=topics_lines(topics), draft=draft, glossary=glossary)


def suggest_merge_messages(existing, new_content, glossary):
    return render("suggest-merge", existing=existing, new_content=new_content, glossary=glossary)


def suggest_groups_messages(category, groups, pages, glossary):
    return render("suggest-groups", category=category, groups=groups_lines(groups), pages=pages_lines(pages), glossary=glossary)


# ------------------------------------------------------------------ reading the replies (README: local checks)

def parse_json(text):
    """The first {...} object in the reply, tolerating code fences and text around it. None if unreadable."""
    s = re.sub(r"^```(?:json)?\s*|\s*```$", "", text.strip())
    start, end = s.find("{"), s.rfind("}")
    if start < 0 or end <= start:
        return None
    try:
        v = json.loads(s[start:end + 1])
    except ValueError:
        return None
    return v if isinstance(v, dict) else None


CONFIDENCES = ("high", "medium", "low", "unknown")


def filter_terms(terms, page, glossary_entries, ignored=(), shown=()):
    """The app-side term filter (README, `check` checks 1-8). Returns (kept, dropped), dropped as (term, reason)."""
    kept, dropped, seen = [], [], set()
    names = [n for e in glossary_entries for n in [e["term"], *e["other"]]]
    for t in terms or []:
        if not isinstance(t, dict):
            continue
        name = str(t.get("term") or "").strip()
        if not name:
            continue
        conf = t.get("confidence") if t.get("confidence") in CONFIDENCES else "low"
        entry = {"term": name, "meaning": str(t.get("meaning") or "").strip(), "confidence": conf}

        def drop(why):
            dropped.append((name, why))

        if not occurs(name, page):
            drop("not in the text (FR-KN-GL-4)")
        elif any(occurs(n, name) for n in names):
            drop("already a glossary entry, or contains one")
        elif name.lower() in {i.lower() for i in ignored}:
            drop("ignored in this category")
        elif conf == "unknown" and not re.search(r"[A-Z0-9]|[^\x00-\x7F]", name):
            drop("unknown confidence, but written as everyday lowercase words")
        elif re.fullmatch(r"[\d\s.,:/%°+-]+(?:[A-Za-z]{1,4})?", name) or len(name.split()) > 4:
            drop("a quantity or date, or a phrase of more than 4 words")
        elif name.lower() in seen:
            drop("duplicate")
        elif name.lower() in {s.lower() for s in shown}:
            drop("already shown for this draft")
        else:
            seen.add(name.lower())
            kept.append(entry)
    return kept, dropped
