#!/usr/bin/env python
"""Owner-run: the M5 prompt templates (crates/cairn-core/prompts/, rendered by prompts.py) on real models, through
the real sidecar (`llm.complete`).

    sidecar/.venv/bin/python poc/h-create/llm_check.py poc/a-llm/config.toml [--only NAME] [--check ID ...]

Uses the [[target]] entries of poc/a-llm/config.toml, converted the same way as
sidecar/tools/live_check.py (keys from `key_env` or a hidden prompt; Entra targets need `az login`).
Sends only the made-up sample files in samples/ (make_samples.py). Prints one line per check, and
writes every reply to poc/h-create/out/<target>/ (gitignored) for reading in full.

Checks (17 checks, 19 calls per target plus at most one retry per check-*/groups; the two OCR checks are skipped
for targets with `image = false`):
  ocr-image      the scanned handover sheet (JPEG): exact text, the Chinese line, the "handwritten" line, no picture description
  ocr-pdf        the same sheet as an image-only PDF page, drawn by pypdfium2 as the app will
  draft-full     the pump manual PDF: every fact kept, filler (cover, contents, legal text, headers, footers) dropped, the flattened table rebuilt
  draft-part     the same with the instruction "only the calibration part"
  draft-docx     the pre-start procedure DOCX: the table kept as a table, the picture not described
  draft-zh       a Chinese .txt note with the English instruction "Tidy this up": the page stays Chinese
  draft-long     the no-instruction long-text path (3 calls): the manual in two parts (draft-part), then draft-combine;
                 judged like draft-full, plus no fact twice
  check-two      the checks on a note mixing calibration and travel budget: two topics
  check-one      the checks on an MMU repair note: one topic, the right group, conservative terms
  check-docx     the checks on the pre-start procedure
  task           task -> knowledge from a task with chatty comments: facts kept, chatter dropped, nothing invented
  revise-change  chat revision: new title, drop the maintenance part
  revise-ask     a question in the chat: a short answer, not a page
  revise-add     "also add the safety note": taken from the source, which the draft didn't include
  keep-one       keep-one-topic on the two-topics note: the calibration part stays, the travel part goes
  merge          suggest-merge of the repair note with a task page about the same repair: facts from both, the
                 conflicting test duration shown as a conflict
  groups         suggest-groups for five ungrouped pages: four fit existing groups, one fits nowhere

The suggested terms are judged after the app-side filter (prompts.filter_terms); what the filter dropped is printed.
"""
import argparse, base64, difflib, json, os, re, subprocess, sys, time, tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "sidecar", "tools"))
sys.path.insert(0, HERE)
import live_check as lc  # noqa: E402  (sidecar/tools/live_check.py: to_target, get_key)
import prompts as P  # noqa: E402
from extract import extract  # noqa: E402

SAMPLES = os.path.join(HERE, "samples")
OUT = os.path.join(HERE, "out")

GLOSSARY = [
    {"term": "MMU", "other": ["Mobile Manufacturing Unit"], "scope": "global",
     "desc": "Truck that mixes bulk explosive on site and pumps it into the blast holes."},
    {"term": "auger", "other": [], "scope": "global",
     "desc": "The arm on an MMU that delivers the explosive into the blast hole."},
]
CATEGORY = "Site operations"
GROUPS = [
    {"name": "Equipment maintenance", "desc": "Servicing, repairs and parts for MMUs, pumps and augers."},
    {"name": "Pre-start & inspections", "desc": "Daily checks and inspections before work starts."},
    {"name": "Blasting procedures", "desc": "Loading, priming and firing patterns."},
    {"name": "Admin & travel", "desc": "Budgets, bookings and paperwork."},
]

# The current draft in the revision checks is fixed, so every model revises the same text.
FULL_DRAFT = """# EP-40 emulsion pump: calibration and maintenance

## Safety
Isolate the pump at the MMU control panel and bleed the line pressure before any service work.

## Calibration
Calibrate the flow meter every 250 operating hours, and always after replacing the stator.

1. Flush the line with water until no emulsion comes out.
2. Run water at 120 kg/min for 2 minutes into the calibration tank.
3. Weigh the tank. The weighed amount must be within 2% of the meter reading.
4. If it is not, enter the new K-factor in the MMU controller under **Settings > Flow**. The factory K-factor is 0.982.

> **Warning:** Never calibrate with emulsion in the line. Emulsion in the calibration tank must be disposed of as explosive waste.

## Maintenance
| Part | Interval | Note |
|---|---|---|
| Auger slew bearing | weekly | 6 strokes of EP2 grease |
| Pump stator | 1,500 hours | replace; then calibrate |
| Inlet strainer | daily | clean at the end of shift |

Torque the pump mounting bolts to 210 Nm after every stator change. Record every service in the MMU logbook."""
CALIB_DRAFT = FULL_DRAFT.split("## Maintenance")[0].replace("# EP-40 emulsion pump: calibration and maintenance",
                                                            "# EP-40 flow meter calibration").replace(
    "## Safety\nIsolate the pump at the MMU control panel and bleed the line pressure before any service work.\n\n", "").strip()

# Never acceptable as glossary suggestions in these samples: glossary terms, everyday words, well-known terms,
# names, part numbers, generic component names made of ordinary words, vendor product models, unit numbers.
# (Wanted, for reference: P3W, Kestrel, AN, gassing solution; PM is borderline but acceptable.)
NOT_TERMS = {"mmu", "mobile manufacturing unit", "auger", "hose", "swivel hose", "pressure", "boom clamp", "day shift",
             "pdf", "vpn", "kg", "bar", "hx-2210", "sl-75", "flow meter", "calibration tank", "depot", "ortiz",
             "r. ortiz", "flights", "hotel", "travel budget", "logbook", "gauge", "test port", "supervisor",
             "emulsion pump", "pre-start", "walk-around",
             "ep-40", "an tank", "mmu-03", "mmu-07", "slew bearing", "auger slew bearing", "inlet strainer", "stator",
             "pump stator", "protective sleeve", "elbow", "auger elbow", "cab", "braid", "outer braid", "rev c",
             "settings > flow", "acme", "acme bulk systems", "admin team", "site visit", "site visits"}

# suggest-merge: the new content is a task page about the same repair as samples/one-topic.md. It adds the part
# SL-75, the history and the other trucks, and disagrees on the test duration (10 min against the page's 5).
MERGE_NEW = """# MMU-07 auger swivel hose cracking at the elbow

## Problem
The auger swivel hose on MMU-07 split three times since August, each time at the elbow. It was replaced once
with the same part (HX-2210); shiny wear marks were noticed on the boom clamp.

## Cause
With the auger slewed fully left, the hose rubs on the boom clamp.

## Fix
- Moved the boom clamp 40 mm towards the cab.
- Added a protective sleeve (part SL-75).
- Pressure tested at 12 bar for 10 minutes, no leak.

## Follow-up
Two weeks later, no new wear. MMU-03 and MMU-05 have the same clamp position: check them at their next PM."""

# suggest-groups: five ungrouped pages (id, title, start of the body) and the group each should get.
UNGROUPED = [
    ("p1", "MMU-05 stator replacement", "Replaced the EP-40 pump stator on MMU-05 at 1,480 hours. Torqued the mounting bolts to 210 Nm and recalibrated the flow meter afterwards.", "Equipment maintenance"),
    ("p2", "Depot walk-around before the first shift", "Every morning before the first MMU leaves: AN tank level, hoses, pump pressure, gassing flow. Sign the logbook.", "Pre-start & inspections"),
    ("p3", "Hotel booking rules for site visits", "Book hotels through the travel desk, two weeks ahead. Receipts to the admin team within five days of returning.", "Admin & travel"),
    ("p4", "Priming sequence for wet holes", "Dewater first, then prime with the booster at the bottom of the column. Load within 30 minutes of dewatering.", "Blasting procedures"),
    ("p5", "Team lunch poll results", "Thai won with 6 votes, pizza 4, sandwiches 1. Booked for Friday 12:30.", None),
]


# ------------------------------------------------------------------ sidecar link

class Sidecar:
    """One `--role ai` process per target; requests are sent one at a time."""

    def __init__(self):
        env = {k: v for k, v in os.environ.items() if k not in ("AZURE_OPENAI_API_KEY", "AZURE_API_KEY", "AZURE_AI_API_KEY")}
        self.p = subprocess.Popen([sys.executable, "-I", "-m", "cairn_sidecar", "--role", "ai"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)
        self.next_id = 1

    def complete(self, target, messages, stream=False):
        """Returns (result, seconds, seconds to the first streamed text or None)."""
        rid, self.next_id = self.next_id, self.next_id + 1
        params = {"target": target, "messages": messages, "stream": stream, "effort": None, "max_tokens": None}
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": "llm.complete", "params": params}) + "\n")
        self.p.stdin.flush()
        t0, first = time.time(), None
        for line in self.p.stdout:
            m = json.loads(line)
            if m.get("method") == "stream" and m.get("params", {}).get("id") == rid:
                first = first or time.time() - t0
            elif m.get("id") == rid:
                if "error" in m:
                    e = m["error"]
                    d = e.get("data") or {}
                    raise RuntimeError(f"[{d.get('kind', e.get('code'))}] {e.get('message')}" + (f" ({d['hint']})" if d.get("hint") else ""))
                return m["result"], time.time() - t0, first
        raise RuntimeError("the sidecar exited")

    def close(self):
        self.p.stdin.close()
        self.p.wait(10)


# ------------------------------------------------------------------ helpers for judging replies

def has(text, *options):
    """True if any option occurs (case-insensitive; options starting with 're:' are regexes)."""
    for o in options:
        if o.startswith("re:"):
            if re.search(o[3:], text, re.I | re.M):
                return True
        elif o.lower() in text.lower():
            return True
    return False


def facts(text, wanted):
    """Each wanted item is a string, or a tuple of alternatives; a tuple may start with "@label"."""
    res = []
    for w in wanted:
        opts = w if isinstance(w, tuple) else (w,)
        label = opts[0][1:] if opts[0].startswith("@") else opts[0]
        res.append((has(text, *[o for o in opts if not o.startswith("@")]), f"keeps {label}"))
    return res


def absent(text, unwanted, why="drops"):
    return [(not has(text, u), f"{why} {u.removeprefix('re:')}") for u in unwanted]


def page_shape(text):
    return [(text.lstrip().startswith("# "), "line 1 is '# title'"), (not text.lstrip().startswith("```"), "no code fence")]


TABLE = r"re:^\|.*\|\s*\n\|\s*:?-{3,}"
PICTURE_WORDS = ["drawing", "sketch", "diagram", "illustration", "truck", "vehicle", "picture", "photo", "re:\\bimage\\b"]


def norm(s):
    return re.sub(r"\s+", " ", re.sub(r"[#*_`>|\-]", " ", s)).strip().lower()


def judge_ocr(text, _):
    truth = open(os.path.join(SAMPLES, "handover-scan.truth.txt")).read()
    ratio = difflib.SequenceMatcher(None, norm(text), norm(truth)).ratio()
    return ([(ratio >= 0.9, f"text matches {ratio:.0%} (want 90%+)")] + facts(text, ["18.4", "02:30", "P3W", "Kestrel"]) +
            [(has(text, "北侧台阶有积水"), "reads the Chinese line"), (has(text, "Ortiz"), "reads the 'handwritten' line")] +
            absent(text, PICTURE_WORDS, "doesn't mention the picture:"))


def han_share(text):
    han = len(re.findall(r"[一-鿿]", text))
    latin = len(re.findall(r"[A-Za-z]", text))
    return han / max(1, han + latin)


def judge_check(expect):
    def judge(text, meta):
        v = meta.get("json")
        res = [(v is not None, "valid JSON" + (" (after one retry)" if meta.get("retried") else ""))]
        if v is None:
            return res
        topics = v.get("topics") or []
        res.append((v.get("one_topic") is expect["one_topic"], f"one_topic = {expect['one_topic']} (topics: {topics})"))
        g = v.get("group") or {}
        if expect.get("group"):
            res.append((isinstance(g, dict) and g.get("name") == expect["group"] and g.get("existing") is True,
                        f"group = {expect['group']} (got {g.get('name') if isinstance(g, dict) else g})"))
        tags = v.get("tags") or []
        res.append((1 <= len(tags) <= 4, f"1-4 tags ({tags})"))
        for t in v.get("terms") or []:   # the model's own list: confidence values and "unknown" means no meaning
            res.append((t.get("confidence") in P.CONFIDENCES, f"term '{t.get('term')}' has a valid confidence ({t.get('confidence')})"))
            if t.get("confidence") == "unknown":
                res.append((not t.get("meaning"), f"term '{t.get('term')}' is unknown, so it has no meaning"))
        for t in meta["terms"]:   # after the app-side filter (prompts.filter_terms)
            name = t["term"]
            res.append((name.lower() not in NOT_TERMS, f"term '{name}' is not an everyday word, a component name, a model number "
                                                        f"or a glossary word [{t['confidence']}: {t['meaning'] or '-'}]"))
        if not meta["terms"]:
            res.append((True, "no terms suggested after the filter"))
        return res
    return judge


def judge_groups(text, meta):
    v = meta.get("json")
    res = [(v is not None, "valid JSON" + (" (after one retry)" if meta.get("retried") else ""))]
    if v is None:
        return res
    got = {e.get("id"): e for e in v.get("pages") or [] if isinstance(e, dict)}
    names = {g["name"].lower(): g["name"] for g in GROUPS}
    for pid, title, _, want in UNGROUPED:
        e = got.get(pid) or {}
        g = e.get("group")
        if want:
            res.append((isinstance(g, str) and names.get(g.lower()) == want and e.get("existing") is True,
                        f"{pid} '{title}' -> {want} (got {g}, existing={e.get('existing')})"))
        else:
            res.append((g is None or (isinstance(g, str) and g.lower() not in names),
                        f"{pid} '{title}' -> null or a new group (got {g})"))
    new = [n.get("name") for n in v.get("new_groups") or [] if isinstance(n, dict)]
    used = {e.get("group") for e in got.values() if isinstance(e.get("group"), str) and e.get("existing") is False}
    res.append((set(new) == used, f"new_groups lists exactly the new names used ({new} against {sorted(used)})"))
    res.append((all(isinstance(e.get("group"), str) and "/" not in e["group"] for e in got.values() if e.get("group")), "no nested group names"))
    return res


def count(text, s):
    return len(re.findall(re.escape(s), text, re.I))


# ------------------------------------------------------------------ the checks

def data_url(part):
    return f"data:{part['mime']};base64,{base64.b64encode(part['image']).decode()}"


def sample_text(name):
    parts = extract(os.path.join(SAMPLES, name))["parts"]
    return "\n\n".join(p["text"] for p in parts if "text" in p)


def run_check(sc, target, cid):
    """Returns (reply text, meta). Raises on a failed call."""
    meta = {}
    if cid == "ocr-image":
        part = extract(os.path.join(SAMPLES, "handover-scan.jpg"))["parts"][0]
        r, meta["s"], _ = sc.complete(target, P.ocr_messages(data_url(part)))
    elif cid == "ocr-pdf":
        part = [p for p in extract(os.path.join(SAMPLES, "site-visit-scan.pdf"))["parts"] if "image" in p][0]
        r, meta["s"], _ = sc.complete(target, P.ocr_messages(data_url(part)))
    elif cid.startswith("draft-") and cid != "draft-long":
        name, instruction = {"draft-full": ("pump-manual.pdf", ""),
                             "draft-part": ("pump-manual.pdf", "Only the calibration part."),
                             "draft-docx": ("pre-start-checks.docx", ""),
                             "draft-zh": ("winter-care-zh.txt", "Tidy this up.")}[cid]
        text = sample_text(name)
        r, meta["s"], meta["first"] = sc.complete(target, P.draft_messages(name, text, instruction, P.glossary_block(GLOSSARY, text)), stream=True)
    elif cid == "draft-long":   # the manual cut after page 3: part 1 = safety + calibration, part 2 = maintenance
        parts = [p["text"] for p in extract(os.path.join(SAMPLES, "pump-manual.pdf"))["parts"] if "text" in p]
        pieces = ["\n\n".join(parts[:3]), parts[3]]
        drafts, meta["s"] = [], 0
        for i, piece in enumerate(pieces, 1):
            r, s, _ = sc.complete(target, P.draft_part_messages("pump-manual.pdf", piece, i, len(pieces), P.glossary_block(GLOSSARY, piece)))
            drafts.append(r["text"])
            meta["s"] += s
        meta["parts"] = drafts
        r, s, meta["first"] = sc.complete(target, P.draft_combine_messages("pump-manual.pdf", drafts, P.glossary_block(GLOSSARY, *drafts)), stream=True)
        meta["s"] += s
    elif cid.startswith("check-"):
        name = {"check-two": "two-topics.md", "check-one": "one-topic.md", "check-docx": "pre-start-checks.docx"}[cid]
        page = meta["page"] = sample_text(name)
        msgs = P.check_messages(CATEGORY, GROUPS, P.glossary_block(GLOSSARY, page), page)
        r, meta["s"], _ = sc.complete(target, msgs)
        meta["json"] = P.parse_json(r["text"])
        if meta["json"] is None:   # one retry, as the app will do
            meta["retried"] = True
            msgs += [{"role": "assistant", "content": r["text"]}] + P.check_retry_messages()
            r, s2, _ = sc.complete(target, msgs)
            meta["s"] += s2
            meta["json"] = P.parse_json(r["text"])
        meta["terms"], meta["dropped"] = P.filter_terms((meta["json"] or {}).get("terms"), page, GLOSSARY)
    elif cid == "groups":
        pages = [(pid, title, body) for pid, title, body, _ in UNGROUPED]
        blob = " ".join(f"{t} {b}" for _, t, b in pages)
        msgs = P.suggest_groups_messages(CATEGORY, GROUPS, pages, P.glossary_block(GLOSSARY, blob))
        r, meta["s"], _ = sc.complete(target, msgs)
        meta["json"] = P.parse_json(r["text"])
        if meta["json"] is None:
            meta["retried"] = True
            msgs += [{"role": "assistant", "content": r["text"]}] + P.check_retry_messages()
            r, s2, _ = sc.complete(target, msgs)
            meta["s"] += s2
            meta["json"] = P.parse_json(r["text"])
    elif cid == "keep-one":
        draft = sample_text("two-topics.md")
        topics = ["Flow meter calibration", "Travel budget for site visits"]
        r, meta["s"], meta["first"] = sc.complete(target, P.keep_one_topic_messages(topics[0], topics, draft, P.glossary_block(GLOSSARY, draft)), stream=True)
    elif cid == "merge":
        existing = sample_text("one-topic.md")
        r, meta["s"], meta["first"] = sc.complete(target, P.suggest_merge_messages(existing, MERGE_NEW, P.glossary_block(GLOSSARY, existing, MERGE_NEW)), stream=True)
    elif cid == "task":
        task = json.load(open(os.path.join(SAMPLES, "task-ops-41.json")))
        blob = task["description"] + " ".join(t for _, t in task["comments"])
        r, meta["s"], meta["first"] = sc.complete(target, P.task_messages(task, P.glossary_block(GLOSSARY, blob)), stream=True)
    elif cid.startswith("revise-"):
        text = sample_text("pump-manual.pdf")
        draft, request = {"revise-change": (FULL_DRAFT, "Change the title to \"EP-40 flow meter calibration\" and drop the maintenance part."),
                          "revise-ask": (FULL_DRAFT, "What K-factor does the manual give, and where do I enter it?"),
                          "revise-add": (CALIB_DRAFT, "Also add the safety note from the manual.")}[cid]
        msgs = P.revise_messages("pump-manual.pdf", text, "", P.glossary_block(GLOSSARY, text), draft, request)
        r, meta["s"], meta["first"] = sc.complete(target, msgs, stream=True)
    else:
        raise ValueError(cid)
    meta["usage"] = r.get("usage")
    return r["text"], meta


JUDGES = {
    "ocr-image": judge_ocr,
    "ocr-pdf": judge_ocr,
    "draft-full": lambda t, m: page_shape(t) + facts(t, ["250", "120 kg/min", "2%", "0.982", "Settings > Flow", "210 Nm",
                                                         ("1,500", "1500", "1 500"), "EP2", "explosive waste", "Isolate"]) +
        [(has(t, TABLE), "rebuilds the maintenance table")] +
        absent(t, ["Page 3 of 4", "All rights reserved", "re:\\.{5,}", "re:^#*\\s*Contents", "Revision C"]),
    "draft-part": lambda t, m: page_shape(t) + facts(t, ["250", "0.982", "2%", "explosive waste"]) +
        absent(t, ["210 Nm", "EP2", "Inlet strainer"], "leaves out (not calibration):"),
    "draft-docx": lambda t, m: page_shape(t) + [(has(t, TABLE), "keeps the limits table")] +
        facts(t, [("@8 to 12 bar", "re:8\\s*(to|–|-)\\s*12\\s*bar"), ("@0.8 to 1.2 L/min", "re:0\\.8\\s*(to|–|-)\\s*1\\.2\\s*L/min"), "20%", "logbook"]) +
        absent(t, PICTURE_WORDS, "doesn't mention the picture:"),
    "draft-zh": lambda t, m: page_shape(t) + [(han_share(t) >= 0.6, f"stays Chinese ({han_share(t):.0%} Han characters)")] +
        facts(t, [("@5 度", "re:5\\s*度", "5°"), ("十分钟", "10分钟", "10 分钟"), ("@零下20", "re:零下\\s*20", "-20")]),
    "check-two": judge_check({"one_topic": False}),
    "check-one": judge_check({"one_topic": True, "group": "Equipment maintenance"}),
    "check-docx": judge_check({"one_topic": True, "group": "Pre-start & inspections"}),
    "draft-long": lambda t, m: JUDGES["draft-full"](t, m) +
        [(count(t, "0.982") == 1, f"K-factor stated once ({count(t, '0.982')}x)"), (count(t, "210 Nm") == 1, f"torque stated once ({count(t, '210 Nm')}x)"),
         (len(re.findall(r"^# ", t, re.M)) == 1, "one title"), (not has(t, "re:^#+\\s*part \\d"), "no 'Part N' headings")],
    "task": lambda t, m: page_shape(t) + facts(t, ["HX-2210", "SL-75", "40 mm", "12 bar", "boom clamp", "MMU-03", "MMU-05", ("three times", "3 times")]) +
        [(len(re.findall(r"^## ", t, re.M)) >= 2, "has sections")] + absent(t, ["tomorrow", "thanks", "re:\\bok\\b"]) +
        absent(t, ["re:\\btwice\\b", "re:\\btwo times\\b", "re:replaced (it )?(two|three|several|multiple) times"], "doesn't invent a count:"),
    "keep-one": lambda t, m: page_shape(t) + facts(t, ["3.5%", "Thursday", "calibration tank", "MMU-03", "MMU-07"]) +
        absent(t, ["flights", "hotel", "four trips", "travel", "receipts"], "drops (other topic):") +
        [(not has(t, "re:^# Notes from the Tuesday meeting"), "title names the kept topic")],
    "merge": lambda t, m: page_shape(t) + facts(t, ["HX-2210", "40 mm", "12 bar", "Kestrel", "P3W", "braid"]) +
        facts(t, ["SL-75", "MMU-03", "MMU-05", ("since August", "August"), ("three times", "3 times"), "wear marks"]) +
        [(has(t, "re:5 min"), "keeps the existing 5 min"), (has(t, "re:10 min"), "keeps the new 10 min"),
         (has(t, "**Conflict:**"), "marks the conflict"), (count(t, "40 mm") <= 2, f"40 mm not repeated ({count(t, '40 mm')}x)"),
         (has(t, "re:\\A\\s*# MMU-07 auger hose leak"), "keeps the existing title")],
    "groups": judge_groups,
    "revise-change": lambda t, m: [(has(t, "re:\\A\\s*# EP-40 flow meter calibration\\s*$"), "new title")] +
        facts(t, ["0.982", "250"]) + absent(t, ["210 Nm", "EP2"]),
    "revise-ask": lambda t, m: [(not t.lstrip().startswith("#"), "answers in text, not a page")] + facts(t, ["0.982", "Settings", "Flow"]) +
        [(len(t) < 600, f"short answer ({len(t)} chars)")],
    "revise-add": lambda t, m: page_shape(t) + facts(t, [("Isolate", "isolat"), ("bleed",), "0.982"]),
}
ALL = list(JUDGES)


def safe(name):
    return re.sub(r"[^\w.-]+", "_", name)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("config")
    ap.add_argument("--only", help="one target name")
    ap.add_argument("--check", nargs="*", choices=ALL, help="run only these checks")
    a = ap.parse_args()
    configs = [c for c in tomllib.load(open(a.config, "rb"))["target"] if not a.only or c["name"] == a.only]
    if not configs:
        sys.exit("no target with that name in the config")
    for c in configs:
        target = lc.to_target(c, lc.get_key(c))
        out_dir = os.path.join(OUT, safe(c["name"]))
        os.makedirs(out_dir, exist_ok=True)
        print(f"\n==================== {c['name']} ({c['model']}, {target['format']})")
        summary, sc = [], Sidecar()
        try:
            for cid in a.check or ALL:
                if cid.startswith("ocr-") and not target["vision"]:
                    line = f"SKIP {cid:<14} (not a vision model)"
                    print(line); summary.append(line)
                    continue
                try:
                    text, meta = run_check(sc, target, cid)
                except Exception as e:  # noqa: BLE001
                    line = f"ERR  {cid:<14} {str(e)[:300]}"
                    print(line); summary.append(line)
                    continue
                results = JUDGES[cid](text, meta)
                bad = [label for ok, label in results if not ok]
                u = meta.get("usage") or {}
                timing = f"{meta['s']:.1f} s" + (f", text after {meta['first']:.1f} s" if meta.get("first") else "")
                line = (f"{'PASS' if not bad else 'FAIL'} {cid:<14} {len(results) - len(bad)}/{len(results)}  "
                        f"({timing}, {u.get('input_tokens', '?')} in / {u.get('output_tokens', '?')} out)")
                print(line); summary.append(line)
                for label in bad:
                    print(f"       - {label}"); summary.append(f"       - {label}")
                if cid.startswith("check-"):   # the suggested terms, pass or fail, are worth reading
                    for t in meta["terms"]:
                        s = f"       term: {t['term']} [{t['confidence']}] {t['meaning']}"
                        print(s); summary.append(s)
                    for name, why in meta["dropped"]:
                        s = f"       filter dropped: {name} ({why})"
                        print(s); summary.append(s)
                with open(os.path.join(out_dir, f"{cid}.md"), "w") as f:
                    f.write(f"<!-- {cid} | {timing} | usage {u} -->\n{text}\n")
                    for i, d in enumerate(meta.get("parts") or [], 1):
                        f.write(f"\n<!-- part draft {i} -->\n{d}\n")
                    if meta.get("json") is not None:
                        f.write("\n<!-- parsed -->\n" + json.dumps(meta["json"], ensure_ascii=False, indent=2) + "\n")
                    f.write("\n<!-- judged -->\n" + "\n".join(f"{'ok ' if ok else 'BAD'} {label}" for ok, label in results) + "\n")
        finally:
            sc.close()
        with open(os.path.join(out_dir, "summary.txt"), "w") as f:
            f.write("\n".join(summary) + "\n")
    print(f"\nFull replies: {OUT}/<target>/")


if __name__ == "__main__":
    main()
