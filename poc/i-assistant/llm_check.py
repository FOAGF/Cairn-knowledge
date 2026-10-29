#!/usr/bin/env python
"""Owner-run: the M6 assistant prompt templates (crates/cairn-core/prompts/ask-*.md, edit-page.md, doc-terms.md, rendered by
poc/h-create/prompts.py with the list formats of formats.py) on real models, through the real sidecar (`llm.complete`).

    sidecar/.venv/bin/python poc/i-assistant/llm_check.py poc/a-llm/config.toml [--only NAME] [--check ID ...]
    sidecar/.venv/bin/python poc/i-assistant/llm_check.py --dry-run [--check ID ...]      (render only, no model)

Uses the [[target]] entries of poc/a-llm/config.toml, converted as sidecar/tools/live_check.py does (keys from `key_env`
or a hidden prompt; Entra targets need `az login`). Sends only the made-up workspace of samples.py. Prints one line per
check and writes every reply in full to poc/i-assistant/out/<target>/ (gitignored).

Checks: 39 checks, 39 calls per target, plus at most one `check-retry` call per JSON check (22 of them). The judges
apply the app-side checks of the README first (formats.check_plan, check_action, markers; prompts.filter_terms), so what
is judged is what the app would show.

  ask-plan (12 calls)
    plan-filler     a question with filler words: focused search words, the names kept
    plan-find       "blocked tasks in Kestrel software about licensing": kind find, category, type, status, intro line
    plan-due        "what's due next week?": a date range computed from today (Monday to Sunday)
    plan-follow     "tell me more about the second one" after an answer that cited two items: refs c2, nothing to search
    plan-scope      a question naming a category outside the scope: outside_scope, not a category filter
    plan-general    a general-knowledge question: kind general, no search
    plan-create     a create-task request: kind create
    plan-change     "mark the hose task as done": kind change, search "hose"
    plan-file-q     a file attached with a question: file_question
    plan-file-alone a file attached alone: file_draft
    plan-file-page  a file attached with "make a page from this": file_draft
    plan-zh         a Chinese question with codes: search in Chinese, "MMU-07" and "HX-2210" kept
  ask-answer (9 calls, streamed)
    ans-cite        a setup question with distractor pages ("setup", "reader", "use"): cites the right source only, valid numbers
    ans-gloss       the Home lab "MMU" note with both MMU entries in the glossary: the category entry, not the global one
    ans-unknown     an internal name that is in no note and no glossary entry: says so, no invented meaning, and no citation on
                    a sentence that names it (pointing at a related note with a correct citation is fine)
    ans-general     a general question with no sources: labelled general knowledge, no markers
    ans-outscope    the out-of-scope note: the answer says the category was not searched
    ans-follow      a follow-up with history and refs_note: answers about the referred item and cites it
    ans-zh          a Chinese question over English and Chinese sources: answers in Chinese with citations
    ans-file        a question about an attached file: cites the file part (the EP-40 page between the parts may be cited too:
                    it is relevant to the stator change the sheet mentions)
    ans-left-out    "what did the fitter find at the PM on 16 September?" about OPS-48, sent cut with the fitter's report
                    left out: says that comment was not sent, or asks for it; never takes the cut task for the whole
                    ("the notes don't say", with nothing anywhere about a part not sent); guesses no finding
  ask-action (9 calls)
    act-create      create a task: category, title, priority, due date from "in two weeks", type; conservative terms
    act-three       "set all three to high priority" after a find list: three update_task changes, the distractor untouched
    act-comment     add a comment to OPS-41: one comment change with the user's words
    act-ambig       "mark the Kestrel task as done" with four Kestrel tasks: a question, no change
    act-delete      "delete OPS-45": no change, the reply says the chat cannot delete
    act-from-latest the owner's case, "Create me another task from #KSW-12 latest comment about wifi problem?", with
                    KSW-12 sent whole: one create_task carrying the latest comment, not the older wifi one; relates to
                    KSW-12; nothing from the other sources; no [n]
    act-left-out    the same request with KSW-12 cut, its latest comment left out (the note): no change, and the reply
                    says, in English, that comment was not sent (the new act-* checks all judge the request's language)
    act-create-src  act-create with the sources the app sends for it (OPS-41, the hose-leak page): the description still
                    adds nothing from them
    act-quote-page  "add a comment to #OPS-41 with the pressure test step from [[How to use the auger hose tester]]",
                    both sent whole: one comment on t1 with the page's step, nothing from OPS-41's comments, no [n]
  ask-page (2 calls, streamed)
    page-from-chat  facts kept, nothing invented, no [n] markers, the distractor left out
    page-left-out   a page from OPS-48 cut to what the request needs, without the note (as ask-page gets it): those
                    facts kept, no "not available" or "paste" text
  edit-page (2 calls, streamed)
    edit-page       the change made, every other line unchanged (judged by diff)
    edit-left-out   a line from OPS-48 sent cut, added to the EP-40 page: a page, not a reply about what is missing;
                    no trace of the note
  doc-terms (1 call)            doc-terms: conservative terms from the shot report (after the app-side filter)
  web probes (4 calls)          web-plain, web-stream: the model with web search on, built with the sidecar's own
                                build_call plus LiteLLM's web_search_options; records the raw shapes to out/, never
                                sends anything from the samples; expected to work on Claude and be refused or ignored
                                by DeepSeek (Chat Completions on Foundry ignores the option silently, 2026-10-15).
                                web-stable: the same call with a stable general-knowledge question that a model
                                would answer from memory, to see that ask-web makes it search anyway; the summary
                                line says "searched N times ..." or "no search was made ...", the owner judges.
                                web-off: ask-web in its no-search mode (the re-answer after a provider refused the
                                search), called through the sidecar without web search; judged: the reply says it is
                                not from a web search, in its first sentence, and contains no URL.

The *-left-out checks send a long made-up task defined here (OPS-48; KSW-12 for act-left-out) as the app sends a
referenced item it has to cut (D-83): the pieces kept, in order, and the LEFT_OUT note of retrieve.rs on the last one
(page-left-out without it, as ask-page gets it since the owner's run of 2026-10-29). Which pieces are kept is set by hand; the pieces are short, to keep the calls cheap. ask-action gets the turn's sources
too (D-83), so the act-* checks pass the ones the app would send; those without sources check that "" still reads well.
"""
import argparse, datetime as dt, difflib, importlib.util, json, os, re, sys, tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..", "..")
sys.path.insert(0, os.path.join(ROOT, "sidecar", "tools"))
sys.path.insert(0, os.path.join(ROOT, "sidecar"))
sys.path.insert(0, os.path.join(ROOT, "poc", "h-create"))
import live_check as lc  # noqa: E402  (sidecar/tools/live_check.py: to_target, get_key)
import prompts as P  # noqa: E402  (poc/h-create/prompts.py: load, render, glossary_block, parse_json, filter_terms)
sys.path.insert(0, HERE)
import formats as F  # noqa: E402
import samples as S  # noqa: E402

OUT = os.path.join(HERE, "out")
TODAY = dt.date.today()
TODAY_LINE = F.today_line(TODAY)
ALL_SCOPE = list(S.NAMES)
OPS_SCOPE = ["Site operations"]
WEB_QUESTION = "Which version of Python is the latest stable release right now, and when was it released? Answer in two sentences."
WEB_STABLE_QUESTION = "What is the boiling point of water at sea level in °C?"  # answerable from memory: does the prompt make it search?
# The two `search` texts of ask-web, word for word as in the README and crates/cairn-core/src/prompts.rs.
WEB_SEARCH_ON = ("The user turned web search on for this message, so search the web before you answer, even when you think you "
                 "already know the answer, and even for a simple question: the user wants the answer checked against current "
                 "pages, not recalled. Base the answer on the pages the search returned and cite the ones you used. Do not write "
                 "web addresses from memory: the citations are attached to your answer for you. When the search finds nothing "
                 "reliable, say so plainly instead of guessing.")
WEB_SEARCH_OFF = ("Web search is not available for this message, so answer from what you already know. Say plainly in your first "
                  "sentence that this answer is not from a web search and may be out of date. Do not write web addresses from "
                  "memory. When the question needs current information you cannot know, say so instead of guessing.")


def h_sidecar():
    """The Sidecar class of poc/h-create/llm_check.py, loaded by path (this file has the same name)."""
    spec = importlib.util.spec_from_file_location("h_llm_check", os.path.join(ROOT, "poc", "h-create", "llm_check.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.Sidecar


# ------------------------------------------------------------------ building the messages

def entries_for(in_scope):
    return [e for e in S.GLOSSARY if e["scope"] == "global" or e["scope"] in in_scope]


def glossary(in_scope, *texts):
    return P.glossary_block(entries_for(in_scope), *texts)


def plan_messages(message, in_scope=ALL_SCOPE, turns=(), cited=(), attachment=None):
    return P.render("ask-plan", today=TODAY_LINE, categories=F.categories_lines(S.CATEGORIES, in_scope),
                    scope=F.scope_sentence(S.CATEGORIES, in_scope), history=F.history_text(list(turns)),
                    cited=F.cited_lines(list(cited)), attachment=attachment or "none", message=message)


def answer_messages(question, pieces, in_scope=ALL_SCOPE, turns=(), outside=(), refs=()):
    src = F.sources_block(pieces)
    msgs = P.render("ask-answer", today=TODAY_LINE, scope=F.scope_sentence(S.CATEGORIES, in_scope),
                    out_of_scope_note=F.out_of_scope_note(list(outside)), refs_note=F.refs_note(list(refs)),
                    sources=src, glossary=glossary(in_scope, question, src), question=question)
    return msgs[:1] + F.history_messages(list(turns)) + msgs[1:]


def action_messages(request, items, in_scope=ALL_SCOPE, turns=(), pieces=()):
    cand, labels = F.candidates_block(items)
    src = F.sources_block(list(pieces))
    msgs = P.render("ask-action", today=TODAY_LINE, categories=F.categories_lines(S.CATEGORIES, in_scope),
                    scope=F.scope_sentence(S.CATEGORIES, in_scope), candidates=cand, sources=src,
                    glossary=glossary(in_scope, request, cand, src), request=request)
    return msgs[:1] + F.history_messages(list(turns)) + msgs[1:], labels


def page_messages(request, category, title, content, pieces, in_scope=ALL_SCOPE, turns=()):
    src = F.sources_block(pieces)
    msgs = P.render("ask-page", today=TODAY_LINE, category=category, title=title, content=content, sources=src,
                    glossary=glossary(in_scope, request, src), request=request)
    return msgs[:1] + F.history_messages(list(turns)) + msgs[1:]


def edit_messages(page, change, pieces=(), in_scope=ALL_SCOPE, turns=()):
    src = F.sources_block(list(pieces))
    msgs = P.render("edit-page", page=page, change=change, sources=src, glossary=glossary(in_scope, page, change, src))
    return msgs[:1] + F.history_messages(list(turns)) + msgs[1:]


def doc_terms_messages(file_name, text, in_scope=ALL_SCOPE):
    return P.render("doc-terms", file_name=file_name, source=text, excerpt_note="", glossary=glossary(in_scope, text))


def web_messages(question=WEB_QUESTION, web=True):
    """ask-web with the mode's fixed search text (README, Other conventions): web on, or the re-answer after a refusal."""
    return P.render("ask-web", today=TODAY_LINE, search=WEB_SEARCH_ON if web else WEB_SEARCH_OFF, message=question)


def file_pieces(f):
    n = len(f["parts"])
    return [{"item": None, "file": f["name"], "index": i, "count": n, "text": t} for i, t in enumerate(f["parts"], 1)]


# The note the app appends to the last piece it sends of a referenced item it had to cut (D-83), word for word as
# LEFT_OUT in crates/cairn-core/src/assistant/retrieve.rs.
LEFT_OUT = ("[Note from the app: this item is too long to send in full, so it was sent without {count} of its parts: {parts}. "
            "Do not guess what is missing; if it may hold what you need, say what was not sent, so the user can paste it.]")


def task_pieces_of(t):
    """samples.task_pieces for a task that is not in samples.TASKS: the description, then each comment."""
    return [{"item": t, "part": "description", "text": t["description"]}] + \
           [{"item": t, "part": ("comment", d), "text": c} for d, c in t["comments"]]


def piece_name(piece):
    """A piece named as the sources' second line names it, as retrieve.rs left_out_note writes it into the note."""
    part = piece["part"]
    if "key" in piece["item"]:
        return "Description" if part == "description" else f"Comment {part[1]}"
    return "Section: " + (" > ".join(part) if part else "(top)")


def cut(pieces, keep):
    """A referenced item sent cut (retrieve.rs whole_item, left_out_note): the pieces whose indexes are in `keep` (always
    the first), in order, with the note naming the others (each name once) after a blank line at the end of the last."""
    assert 0 in keep
    sent = [dict(p) for i, p in enumerate(pieces) if i in keep]
    names = []
    for i, p in enumerate(pieces):
        if i not in keep and piece_name(p) not in names:
            names.append(piece_name(p))
    if names:
        sent[-1]["text"] += "\n\n" + LEFT_OUT.replace("{count}", str(len(names))).replace("{parts}", ", ".join(names))
    return sent


def without_notes(pieces):
    """The pieces as ask-page gets them (retrieve.rs without_left_out_notes): each cut where the LEFT_OUT note starts."""
    start = "\n\n" + LEFT_OUT.split("{count}")[0]
    out = []
    for p in pieces:
        p = dict(p)
        at = p["text"].rfind(start)
        if at >= 0:
            p["text"] = p["text"][:at]
        out.append(p)
    return out


# ------------------------------------------------------------------ judging helpers (as in poc/h-create)

def has(text, *options):
    for o in options:
        if o.startswith("re:"):
            if re.search(o[3:], text, re.I | re.M):
                return True
        elif o.lower() in text.lower():
            return True
    return False


def facts(text, wanted):
    res = []
    for w in wanted:
        opts = w if isinstance(w, tuple) else (w,)
        label = opts[0][1:] if opts[0].startswith("@") else opts[0]
        res.append((has(text, *[o for o in opts if not o.startswith("@")]), f"keeps {label}"))
    return res


def absent(text, unwanted, why="drops"):
    return [(not has(text, u), f"{why} {u.removeprefix('re:')}") for u in unwanted]


def han_share(text):
    han = len(re.findall(r"[一-鿿]", text))
    latin = len(re.findall(r"[A-Za-z]", text))
    return han / max(1, han + latin)


def page_shape(text):
    return [(text.lstrip().startswith("# "), "line 1 is '# title'"), (not text.lstrip().startswith("```"), "no code fence")]


FILLER = {"how", "do", "i", "the", "on", "what", "about", "tell", "me", "more", "set", "up", "use", "does", "this", "give",
          "for", "a", "an", "is", "are", "my", "please", "any", "there", "to", "of", "in", "say", "notes", "task", "tasks", "page", "pages"}


def judge_json(meta):
    return [(meta.get("json") is not None, "valid JSON" + (" (after one retry)" if meta.get("retried") else ""))]


def judge_plan(expect):
    """expect: kind, and optional search_has / search_lacks / filters / refs / outside / intro / search_empty / search_han / max_words."""
    def judge(text, meta):
        res = judge_json(meta)
        if meta.get("json") is None:
            return res
        v = meta["plan"]
        res.append((v["kind"] == expect["kind"], f"kind = {expect['kind']} (got {v['kind']})"))
        words = v["search"].split()
        if expect.get("search_empty"):
            res.append((v["search"] == "", f"no search words (got {v['search']!r})"))
        for w in expect.get("search_has", []):
            res.append((has(v["search"], w), f"search keeps {w!r} (got {v['search']!r})"))
        if expect.get("search_focused"):
            bad = [w for w in words if w.lower().strip("?,.") in FILLER]
            res.append((not bad and 1 <= len(words) <= expect.get("max_words", 6), f"search is focused, no filler ({v['search']!r})"))
        if expect.get("search_han"):
            res.append((han_share(re.sub(r"[A-Z]+-\d+", "", v["search"])) >= 0.5, f"search words are Chinese ({v['search']!r})"))
        for k, want in expect.get("filters", {}).items():
            got = v["filters"][k]
            res.append((got == want, f"filters.{k} = {want} (got {got})"))
        if "refs" in expect:
            res.append((v["refs"] == expect["refs"], f"refs = {expect['refs']} (got {v['refs']})"))
        if "outside" in expect:
            res.append((v["outside_scope"] == expect["outside"], f"outside_scope = {expect['outside']} (got {v['outside_scope']})"))
        if expect.get("intro"):
            res.append((bool(v["intro"]) and not re.search(r"^\s*[-*]|\n", v["intro"]), f"one intro line ({v['intro']!r})"))
        if meta.get("plan_notes"):
            res.append((False, "nothing repaired by the app-side checks: " + "; ".join(meta["plan_notes"])))
        return res
    return judge


def judge_answer(cite, not_cite=(), wanted=(), unwanted=(), extra=None):
    """cite: the source numbers the answer must cite (all of them); [] means it must cite nothing; None means no
    requirement. not_cite: numbers it must not cite."""
    def judge(text, meta):
        cited = meta["cited"]
        res = [] if cite is None else \
              [(all(n in cited for n in cite), f"cites {sorted(cite)} (cited {cited})")] if cite else \
              [(not cited, f"cites nothing (cited {cited})")]
        for n in not_cite:
            res.append((n not in cited, f"does not cite the distractor [{n}] (cited {cited})"))
        res.append((not meta["invalid"], f"no marker with a number that is not a source (removed: {meta['invalid']})"))
        res.append((not has(text, "re:^\\s*(sources?|references?)\\s*:?\\s*$"), "no source list at the end"))
        res += facts(text, wanted) + absent(text, unwanted)
        if extra:
            res += extra(text, meta)
        return res
    return judge


def judge_action(expect):
    def judge(text, meta):
        res = judge_json(meta)
        if meta.get("json") is None:
            return res
        kept, dropped, reply = meta["changes"], meta["dropped"], meta["reply"]
        ops = [c["op"] for c in kept]
        res.append((not has(json.dumps(meta["json"]), "re:\"op\"\\s*:\\s*\"[^\"]*delet"), "no delete op in the raw reply"))
        if "count" in expect:
            res.append((len(kept) == expect["count"], f"{expect['count']} change(s) after the app-side checks (got {ops}; dropped {[d[1] for d in dropped]})"))
        if "op" in expect:
            res.append((all(o == expect["op"] for o in ops), f"every change is {expect['op']} (got {ops})"))
        if expect.get("reply"):
            res.append((bool(reply), f"has a reply ({reply!r})"))
        if expect.get("reply_has"):
            res.append((has(reply, *expect["reply_has"]), f"the reply says {expect['reply_has']} ({reply!r})"))
        if expect.get("targets") is not None:
            got = sorted(c.get("target") for c in kept)
            res.append((got == sorted(expect["targets"]), f"targets {sorted(expect['targets'])} (got {got})"))
        if expect.get("check"):
            res += expect["check"](kept, reply, meta)
        return res
    return judge


# ------------------------------------------------------------------ the checks

FOLLOW_TURNS = [("user", "what hose issues do we have?"),
                ("assistant", "Two: the MMU-07 swivel hose that keeps cracking at the elbow [1], and the boom clamps on MMU-03 and MMU-05, "
                              "which have the same clamp position and must be checked at their next PM [2].")]
FOLLOW_CITED = [S.TASKS["OPS-41"], S.TASKS["OPS-47"]]
FIND_TURNS = [("user", "what's due next week?"),
              ("assistant", "Open tasks due next week:\n- Task OPS-42: Replace the EP-40 stator on MMU-05\n- Task OPS-46: Kestrel tablet mount for MMU-03\n"
                            "- Task KSW-8: Kestrel reader firmware 3.2 rollout")]
CREATE_REQUEST = ("create a task in Site operations to order a spare HX-2210 hose for MMU-07, high priority, due in two weeks. "
                  "Log it in SiteTrak too.")
EDIT_CHANGE = ("In the steps, change the test duration from 5 to 10 minutes, and add to step 2 that the gauge must be zeroed "
               "with the hose disconnected.")
PAGE_CONTENT = "What the notes say about EP-40 flow meter calibration: when it is done, the procedure, and the stator change due on MMU-05."

# A long task for the *-left-out checks (D-83), made up like samples.py. Pieces: 0 the description, 1-13 the comments.
FLOW_TASK = {
    "key": "OPS-48", "title": "MMU-05 emulsion pump loses flow", "category": "Site operations", "status": "done", "priority": "high",
    "type": "bug", "due": None, "labels": ["pump", "mmu-05"], "parent": None, "links": ["OPS-42"],
    "description": "Since mid-August the EP-40 pump on MMU-05 starts a load at about 120 kg/min and drops to about 90 after an hour. "
                   "The last holes of a pattern get short columns. Find the cause before the next PM.",
    "comments": [
        ("2026-08-14", "Seen again at P3W on day shift: 118 kg/min at the start, 92 after 70 minutes. Emulsion temperature normal."),
        ("2026-08-18", "Checked the flow meter against the calibration tank: within 1% of the meter reading. The meter is right; the flow really drops."),
        ("2026-08-21", "Cleaned the pump inlet strainer: a little crust, not enough to explain it. No change."),
        ("2026-08-28", "Suspect the stator: MMU-05 is at 1,350 hours. Asked the depot for a price."),
        ("2026-09-02", "Stator price received; three weeks' lead time."),
        ("2026-09-09", "Ran a full shift with the gassing solution off: the same drop, so the gassing line is not the cause."),
        ("2026-09-10", "The fitter will open the pump at the PM on 16 September."),
        ("2026-09-16", "Fitter opened the pump: stator and rotor within limits. The suction hose (HX-1180) has a loose inner liner that "
                       "pulls in when the hose warms up and starves the pump. New hose ordered."),
        ("2026-09-17", "New HX-1180 suction hose fitted."),
        ("2026-09-23", "A full shift on the new hose: 119 to 121 kg/min all day."),
        ("2026-09-30", "Cancelled the stator order."),
        ("2026-10-07", "MMU-03's suction hose is from the same batch; replaced at its PM too."),
        ("2026-10-14", "No flow loss since 17 September. Closing; the stator stays on the 1,500-hour plan (OPS-42)."),
    ]}
# Only the comments left out say these (the fitter's finding and the fix): a reply or page that has them guessed.
FLOW_HIDDEN = ("HX-1180", "suction hose", "re:\\bliner\\b")
LEFT_OUT_QUESTION = "#OPS-48 what did the fitter find at the PM on 16 September?"
LEFT_OUT_PAGE_REQUEST = ("#OPS-48 make a page for the operators: what the flow loss looked like, and how the flow meter and the "
                         "gassing line were ruled out")
LEFT_OUT_PAGE_CONTENT = ("What the flow loss on MMU-05 looked like (the flow readings), and the checks that ruled out the flow meter "
                         "and the gassing line.")
LEFT_OUT_EDIT_CHANGE = ("In the Calibration section, add one line after step 4 saying what OPS-48 found when the MMU-05 flow meter "
                        "was checked against the calibration tank.")

# The owner's case (2026-10-28, "Create me another task from #VYT-7 latest comment about wifi problem?"), on a made-up
# task: an older comment touches the wifi too, the latest one is the problem. Pieces: 0 the description, 1-4 the comments.
WIFI_LATEST = ("Wifi problem on the last two trucks, MMU-09 and MMU-11: parked at bay 4, their tablets show the depot wifi as "
               "connected, but the sync times out. At bay 1 they sync fine. The access point at bay 4 (AP-7) restarted itself "
               "twice this week.")
WIFI_TASK = {
    "key": "KSW-12", "title": "Move the truck tablets to Kestrel 4.3", "category": "Kestrel software", "status": "in_progress",
    "priority": "medium", "type": "feature", "due": "2026-11-06", "labels": ["kestrel", "tablets"], "parent": None, "links": [],
    "description": "Update all 12 truck tablets from Kestrel 4.2 to 4.3, one truck a day, and check that each tablet syncs at the "
                   "depot before 06:00.",
    "comments": [
        ("2026-09-29", "MMU-07 and MMU-03 updated. The depot wifi password had changed, so I re-entered it on both tablets before they would sync."),
        ("2026-10-06", "MMU-05 updated. Kestrel 4.3 asks for the reader to be paired again; the PIN is unchanged."),
        ("2026-10-13", "Four more trucks updated, no problems."),
        ("2026-10-20", WIFI_LATEST),
    ]}
WIFI_REQUEST = "Create me another task from #KSW-12 latest comment about wifi problem?"
# In the other sources only (the older comments, the description, the KSW-10 search result): not in the new task.
WIFI_ELSEWHERE = ("password", "re:\\bPIN\\b", "paired", "06:00", "200 holes", "re:\\b3\\.1\\b")
# Causes and fixes no source gives: a description that has them made them up.
GUESSES = ("re:\\binterference\\b", "re:\\bfirmware\\b", "re:\\bdrivers?\\b", "re:\\bchannels?\\b", "re:\\bsurvey\\b",
           "re:possible causes?", "re:\\b(likely|probably|possibly)\\b")
# In the OPS-41 and hose-leak sources only: a plain create request (act-create) must not pick them up.
HOSE_ELSEWHERE = ("SL-75", "boom clamp", "40 mm", "12 bar", "elbow", "sleeve", "braid", "re:\\bsplit", "three times")
QUOTE_REQUEST = "add a comment to #OPS-41 with the pressure test step from [[How to use the auger hose tester]]"


def next_week():
    monday = TODAY + dt.timedelta(days=7 - TODAY.weekday())
    return monday.isoformat(), (monday + dt.timedelta(days=6)).isoformat()


def plan(cid, message, expect, **kw):
    return {"cid": cid, "template": "ask-plan", "messages": plan_messages(message, **kw), "json": True,
            "in_scope": kw.get("in_scope", ALL_SCOPE), "attachment": kw.get("attachment"), "judge": judge_plan(expect)}


def answer(cid, question, pieces, judge, **kw):
    return {"cid": cid, "template": "ask-answer", "messages": answer_messages(question, pieces, **kw), "stream": True,
            "n_sources": len(pieces), "judge": judge}


def action(cid, request, items, expect, **kw):
    msgs, labels = action_messages(request, items, **kw)
    return {"cid": cid, "template": "ask-action", "messages": msgs, "json": True, "labels": labels,
            "in_scope": kw.get("in_scope", ALL_SCOPE), "judge": judge_action(expect)}


def create_check(kept, reply, meta):
    res = []
    if not kept:
        return res
    c = kept[0]
    lo, hi = TODAY + dt.timedelta(days=12), TODAY + dt.timedelta(days=16)
    res += [(c["category"] == "Site operations", f"category Site operations (got {c['category']})"),
            (has(c["title"], "HX-2210", "hose"), f"title names the hose ({c['title']!r})"),
            (c["priority"] == "high", f"priority high (got {c['priority']})"),
            (c["due"] is not None and lo.isoformat() <= c["due"] <= hi.isoformat(), f"due about two weeks from today (got {c['due']})"),
            (c["type"] == "general", f"type general (got {c['type']})"),
            (not has(c["title"] + c["description"], "re:\\b(medium|low)\\b priority"), "adds nothing the user did not say")]
    for t in meta["terms"]:
        res.append((t["term"].lower() not in S.NOT_TERMS, f"term '{t['term']}' is not an everyday word, part number or glossary word "
                                                          f"[{t['confidence']}: {t['meaning'] or '-'}]"))
    if not meta["terms"]:
        res.append((True, "no terms suggested after the filter"))
    return res


def three_check(kept, reply, meta):
    return [(all(c.get("priority") == "high" and len(c) == 3 for c in kept), f"each change sets only priority high ({kept})")]


def comment_check(kept, reply, meta):
    text = kept[0]["text"] if kept else ""
    return facts(text, ["SL-75", ("two weeks", "2 weeks"), ("no wear", "no new wear")])


def ambig_check(kept, reply, meta):
    names = sum(1 for k in ("OPS-46", "KSW-7", "KSW-8", "KSW-10") if k in reply)
    return [(names >= 2 or "?" in reply, f"the reply asks which task, naming the candidates ({reply!r})")]


# ---- ask-action with sources (D-83): content only where the request points at it

def no_markers(kept):
    return (not re.search(r"\[\d+\]", json.dumps(kept, ensure_ascii=False)), "no [n] in any field")


def in_english(kept, reply):
    """The reply and every text field in the request's language (English here), each judged on its own."""
    fields = [reply] + [str(c.get(k) or "") for c in kept for k in ("title", "description", "text", "content", "change")]
    chinese = [f for f in fields if f.strip() and han_share(f) >= 0.3]
    return (not chinese, f"the reply and every text field in English, the request's language (in Chinese: {chinese[:2]})")


def from_latest_check(kept, reply, meta):
    """act-from-latest: the new task carries the latest comment, relates to KSW-12, and adds nothing else."""
    if not kept:
        return []
    c = kept[0]
    d, links = c["description"], c["links"]
    return ([(c["category"] == "Kestrel software", f"category of KSW-12, Kestrel software (got {c['category']})"),
             (has(c["title"], "wifi", "wi-fi", "bay 4", "sync"), f"title names the wifi problem ({c['title']!r})"),
             (c["priority"] == "medium" and c["due"] is None,
              f"priority and due as the user left them, medium and none (got {c['priority']}, {c['due']})")] +
            facts(d, ["bay 4", ("AP-7", "access point"), ("MMU-09", "MMU-11")]) +
            [({"to": "t1", "kind": "relates"} in links, f"relates to KSW-12, t1 (links {links})"),
             (all(l["to"] != "t2" for l in links), "no link to KSW-10, t2"),
             no_markers(kept), in_english(kept, reply),
             (len(d) <= len(WIFI_LATEST) + 200, f"the description is the comment, give or take a line ({len(d)} characters)")] +
            absent(d, WIFI_ELSEWHERE, "takes nothing from the other sources:") + absent(d, GUESSES, "invents nothing:"))


def left_out_action_check(kept, reply, meta):
    """act-left-out: nothing proposed (count 0 is judged too), and the reply says the latest comment was not sent."""
    return [(has(reply, *SAYS_NOT_SENT[1:]), f"the reply says that comment was not sent, or asks for it ({reply!r})"),
            (has(reply, "2026-10-20", "re:20(th)? oct", "re:oct(ober)?\\.? 20", "latest", "newest", "most recent", "last comment"),
             "the reply says which comment (its date, or the latest)"),
            in_english(kept, reply)]


def create_src_check(kept, reply, meta):
    """act-create-src: act-create's judge, and nothing taken from the sources the request does not point at."""
    res = create_check(kept, reply, meta)
    if kept:
        c = kept[0]
        res += absent(c["title"] + "\n" + c["description"], HOSE_ELSEWHERE, "adds nothing from the sources:") + [no_markers(kept)]
    return res + [in_english(kept, reply)]


def quote_page_check(kept, reply, meta):
    """act-quote-page: the comment carries the page's pressure test step, nothing from elsewhere."""
    text = kept[0]["text"] if kept else ""
    return (facts(text, ["12 bar", ("5 minutes", "5 min"),
                         ("@the page's wording, not OPS-41's comment", "pump up", "re:wait(ing)? 5 min", "0.5 bar")]) +
            absent(text, ["SL-75", "40 mm", "boom clamp", "re:\\b10 min"], "nothing from elsewhere:") +
            [no_markers(kept), in_english(kept, reply)])


def no_cite_on(name):
    """No [n] marker on a sentence that names `name` (a citation there would claim a note covers it)."""
    def check(text, meta):
        sentences = [s for s in re.split(r"(?<=[.!?。！？])\s+|\n+", text) if name.lower() in s.lower()]
        bad = [s.strip() for s in sentences if re.search(r"\[\d+\]", s)]
        return [(not bad, f"no citation on a sentence that names {name} (found: {bad[:2]})")]
    return check


def judge_page(text, meta):
    return (page_shape(text) + facts(text, ["250", "0.982", "2%", ("1,480", "1480", "1 480"), "Settings > Flow"]) +
            [(not re.search(r"\[\d+\]", text), "no [n] markers left in the page")] +
            absent(text, ["barcode", "magazine"], "leaves out the distractor:") +
            absent(text, ["re:\\b210 Nm", "re:1,500 hours\\b.*replace"], "invents nothing:"))


def judge_edit(text, meta):
    page = S.PAGES["hose-tester"]["body"]
    res = page_shape(text)
    if not text.lstrip().startswith("# "):
        return res + [(False, "a page, not a plain reply")]
    old, new = page.splitlines(), text.strip().splitlines()
    removed = [l for l in old if l not in new]
    added = [l for l in new if l not in old]
    changeable = {"2. Zero the gauge.", "3. Pump up to 12 bar and wait 5 minutes."}
    res += [(has(text, "10 minutes"), "test duration is now 10 minutes"),
            (has(text, "disconnected"), "step 2 says the hose is disconnected"),
            (not has(text, "wait 5 minutes"), "the old duration is gone"),
            (set(removed) <= changeable, f"only steps 2 and 3 changed (also removed: {[l for l in removed if l not in changeable]})"),
            (len(added) <= 3, f"at most 3 lines added (added {len(added)}: {added[:4]})"),
            (new[0] == old[0], "the title is unchanged"),
            (not re.search(r"\[\d+\]", text), "no [n] markers")]
    return res


# ---- the *-left-out checks: OPS-48 sent cut, with the LEFT_OUT note

SAYS_NOT_SENT = ("@says that comment was not sent, or asks for it",
                 "re:(not|n't|never)\\b[^.\\n]{0,40}\\b(sent|included|shared|provided|available)\\b",
                 "re:(don'?t|do not|didn'?t|did not) (have|get|receive|see) (the|that|this) (comment|part|report)",
                 "paste", "left out", "re:\\b(missing|omitted)\\b")
# Saying that the notes or the task have no such thing, which is wrong when it was only not sent...
NO_SUCH = ("re:notes (don'?t|do not|doesn'?t|does not) (cover|mention|say|contain|include|record)",
           "re:not (covered|mentioned|recorded) in (your|the) notes",
           "re:\\b(task|OPS-48) (doesn'?t|does not|didn'?t|did not) (say|mention|record|include|cover)",
           "re:\\bno (such |matching )?(comment|record|note|entry)\\b",
           "re:(isn'?t|is not|wasn'?t|was not) (a|any) (comment|record|note|entry) (from|on|of|dated|for|about)")
# ...unless the sentence puts it down to what was sent, or the reply says anywhere that the part was not sent or asks for
# it: only a reply that takes the cut item for the whole fails (relaxed after the owner's second run, 2026-10-29).
SENT_WORDS = ("sent", "received", "included", "left out", "provided", "shared", "given", "paste", "available")
# A finding stated as fact: "the fitter found the stator worn", "the rotor was damaged".
FINDING_CLAIMS = ("re:\\bfound (that )?(the |a |an )?([\\w-]+ ){0,3}(was |were |is |had )?(worn|damaged|cracked|broken|loose|"
                  "faulty|blocked|collapsed|failed|split|leaking|nothing)\\b",
                  "re:\\b(stator|rotor|pump) (was|were) (worn|replaced|changed|damaged|cracked|faulty|the cause|to blame)\\b")
# What the note would leave in a page: its own words, a reply about what was not sent, a question to the user.
NOTE_TRACES = ("Note from the app", "re:(not|n't) (been )?(sent|included)", "left out", "paste", "re:too long to send",
               "re:\\b(not available|unavailable)\\b", "re:\\b(would you like|do you want|let me know|could you|can you)\\b")


def left_out_answer(text, meta):
    """The extra judge of ans-left-out: the cut item not taken for the whole, no finding guessed."""
    sentences = [s.strip() for s in re.split(r"(?<=[.!?。！？])\s+|\n+", text) if s.strip()]
    told = has(text, *SAYS_NOT_SENT[1:])
    lack = [] if told else [s for s in sentences if has(s, *NO_SUCH) and not has(s, *SENT_WORDS)]
    claims = [s for s in sentences if has(s, *FINDING_CLAIMS)]
    return ([(not lack, f"does not say the notes or the task lack it, unless it also says what was not sent (found: {lack[:2]})"),
             (not claims, f"states no finding as fact (found: {claims[:2]})")] +
            absent(text, FLOW_HIDDEN, "guesses nothing:"))


def judge_page_left_out(text, meta):
    return (page_shape(text) + facts(text, [("118", "120"), ("92", "90"), ("1%", "1 %", "1 percent", "one percent"), "gassing"]) +
            [(not re.search(r"\[\d+\]", text), "no [n] markers left in the page")] +
            absent(text, NOTE_TRACES, "no trace of the note:") + absent(text, FLOW_HIDDEN, "invents nothing:"))


def judge_edit_left_out(text, meta):
    page = S.PAGES["ep40"]["body"]
    res = page_shape(text)
    if not text.lstrip().startswith("# "):
        return res + [(False, "a page, not a plain reply")]
    old, new = page.splitlines(), text.strip().splitlines()
    removed = [l for l in old if l not in new]
    added = [l for l in new if l not in old]
    res += [(has(text, "1%", "1 %", "1 percent", "one percent"), "the meter check of the comment that was sent is added (within 1%)"),
            (not removed, f"no line removed or changed (removed: {removed[:3]})"),
            (len(added) <= 3, f"at most 3 lines added (added {len(added)}: {added[:4]})"),
            (new[0] == old[0], "the title is unchanged"),
            (not re.search(r"\[\d+\]", text), "no [n] markers")]
    return res + absent(text, NOTE_TRACES, "no trace of the note:") + absent(text, FLOW_HIDDEN, "invents nothing:")


def judge_doc_terms(text, meta):
    res = judge_json(meta)
    if meta.get("json") is None:
        return res
    for t in meta["json"].get("terms") or []:
        res.append((t.get("confidence") in P.CONFIDENCES, f"term '{t.get('term')}' has a valid confidence ({t.get('confidence')})"))
        if t.get("confidence") == "unknown":
            res.append((not t.get("meaning"), f"term '{t.get('term')}' is unknown, so it has no meaning"))
    for t in meta["terms"]:
        res.append((t["term"].lower() not in S.NOT_TERMS, f"term '{t['term']}' is not an everyday word, a trade word in the glossary or a name "
                                                          f"[{t['confidence']}: {t['meaning'] or '-'}]"))
    if not meta["terms"]:
        res.append((True, "no terms suggested after the filter"))
    return res


NO_SEARCH_SAID = ("re:(not|no|without|isn'?t|is not|wasn'?t|was not|cannot|can'?t|couldn'?t|could not|unable to|don'?t have|do not have|didn'?t|did not)"
                  "[^.\\n]{0,60}(web search|search(ed|ing)? (the )?(web|internet|online)|internet access|browse|browsing|live (search|data))",
                  "re:(from|based on) (my|its) (own )?(knowledge|training|memory)", "re:not (from|based on) (a |the )?(web|internet)")
URL_IN_TEXT = ("re:https?://", "re:\\bwww\\.", "re:\\b[a-z0-9-]+\\.(org|com|net|io|dev|edu|gov)(/|\\b)")


def judge_web_off(text, meta):
    """The no-search mode: the reply says it is not from a web search, up front, and writes no web address."""
    first = re.split(r"(?<=[.!?。！？])\s+|\n+", text.strip(), maxsplit=1)[0] if text.strip() else ""
    return [(has(text, *NO_SEARCH_SAID), "says the answer is not from a web search"),
            (has(first, *NO_SEARCH_SAID), f"says so in the first sentence ({first[:120]!r})"),
            (has(text, "re:out of date|outdated|may (have )?change|may not be current|as of|not (be )?(the )?(latest|current|up[- ]to[- ]date)"),
             "says the answer may be out of date")] + absent(text, URL_IN_TEXT, "no web address:")


def build_checks():
    pg, tk, tp = S.page_piece, S.TASKS, S.task_pieces
    nw_from, nw_to = next_week()
    torque = file_pieces(S.TORQUE_SHEET)
    flow = task_pieces_of(FLOW_TASK)
    checks = [
        # ---- ask-plan
        plan("plan-filler", "how do I set up the Kestrel reader on the tablet?",
             {"kind": "question", "search_has": ["Kestrel", "reader"], "search_focused": True, "filters": {"categories": [], "statuses": []}}),
        plan("plan-find", "blocked tasks in Kestrel software about licensing",
             {"kind": "find", "search_has": ["licens"], "search_focused": True, "intro": True,
              "filters": {"categories": ["Kestrel software"], "types": ["task"], "statuses": ["blocked"]}}),
        plan("plan-due", "what's due next week?",
             {"kind": "find", "intro": True, "filters": {"types": ["task"], "due": {"from": nw_from, "to": nw_to}}}),
        plan("plan-follow", "tell me more about the second one", {"kind": "question", "refs": ["c2"], "search_empty": True},
             turns=FOLLOW_TURNS, cited=FOLLOW_CITED),
        plan("plan-scope", "what do the Home lab notes say about the bootloader crash?",
             {"kind": "question", "outside": ["Home lab"], "search_has": ["bootloader"], "filters": {"categories": []}}, in_scope=OPS_SCOPE),
        plan("plan-general", "what is the flash point of diesel?", {"kind": "general", "search_empty": True}),
        plan("plan-create", "create a task to order a spare HX-2210 hose for MMU-07, high priority, due in two weeks", {"kind": "create"}),
        plan("plan-change", "mark the hose task as done", {"kind": "change", "search_has": ["hose"], "search_focused": True}),
        plan("plan-file-q", "what torque does this give for the stator housing bolts?",
             {"kind": "file_question", "search_has": ["torque"], "search_focused": True}, attachment=S.TORQUE_SHEET["name"]),
        plan("plan-file-alone", "", {"kind": "file_draft"}, attachment=S.TORQUE_SHEET["name"]),
        plan("plan-file-page", "make a page from this", {"kind": "file_draft"}, attachment=S.TORQUE_SHEET["name"]),
        plan("plan-zh", "MMU-07 的 HX-2210 软管为什么老是开裂？",
             {"kind": "question", "search_has": ["MMU-07", "HX-2210"], "search_han": True, "max_words": 6, "search_focused": True}),
        # ---- ask-answer
        answer("ans-cite", "how do I set up the Kestrel reader on the tablet?",
               [pg("barcode-reader", "Setup"), pg("hose-tester", "Steps"), pg("kestrel-reader", "Setup"), pg("kestrel-reader", "Rules"), pg("firmware", "Rollout")],
               judge_answer(cite=[3], not_cite=[1, 2], wanted=["2468", ("@test tag", "test tag")])),
        answer("ans-gloss", "In the Home lab board crash note, what is the MMU and what was wrong with it?",
               [pg("hose-leak"), pg("board-crash"), tp("LAB-3")[0]],
               judge_answer(cite=[2], not_cite=[1], wanted=[("memory management", "内存管理")],
                            unwanted=["Mobile Manufacturing Unit", "mixes bulk explosive"])),
        answer("ans-unknown", "What does Pelican do, and who looks after it?",
               [pg("kestrel-reader", "Setup"), pg("licence")],
               judge_answer(cite=None, wanted=[("@says the notes don't cover it", "re:(don'?t|do not|doesn'?t|does not|no(t|thing))\\b[^.]{0,40}\\b(cover|mention|say|contain|include|explain|have|found|appear)",
                                                 "re:not (in|from) (your|the) notes", "re:no (note|information|record)")],
                            unwanted=["re:Pelican is (a|an|the) (?!name)", "re:Pelican (stands|refers) (for|to) (a|an|the)"],
                            extra=no_cite_on("Pelican"))),  # a correct citation of a related note ("the closest match is Kestrel [2]") is fine
        answer("ans-general", "what is the flash point of diesel?", [],
               judge_answer(cite=[], wanted=[("@the general-knowledge label", "General knowledge, not from your notes"), ("re:5\\d\\s*°?\\s*C", "re:6\\d\\s*°?\\s*C", "re:1[2-6]\\d\\s*°?\\s*F")])),
        answer("ans-outscope", "what do the Home lab notes say about the bootloader crash?", [],
               judge_answer(cite=[], wanted=["Home lab", ("@says it was not searched", "not searched", "outside", "scope", "wasn't searched", "weren't searched")]),
               in_scope=OPS_SCOPE, outside=["Home lab"]),
        answer("ans-follow", "tell me more about the second one",
               [tp("OPS-41")[0], tp("OPS-47")[0], tp("OPS-41")[3]],
               judge_answer(cite=[2], wanted=[("MMU-03", "MMU-05"), "clamp"]),
               turns=FOLLOW_TURNS, refs=[(2, tk["OPS-47"])]),
        answer("ans-zh", "P3W 湿孔装药前要先做什么？",
               [pg("walkaround"), pg("wet-holes", "Procedure"), pg("wet-holes", "Limits")],
               judge_answer(cite=[2], wanted=[("30 分钟", "30分钟", "30 min", "三十分钟"), ("抽水", "排水", "脱水", "除水")],
                            extra=lambda t, m: [(han_share(t) >= 0.5, f"answers in Chinese ({han_share(t):.0%} Han characters)")])),
        answer("ans-file", "what torque does this sheet give for the stator housing bolts?",
               [torque[0], pg("ep40", "Calibration"), torque[1]],
               judge_answer(cite=[1], wanted=["210"])),  # [2] is not a distractor: its recalibration note is relevant to a stator change
        # OPS-48 referenced, cut: the description, the symptoms, the stator suspicion, the PM plan and the closing comment
        # (with the note) are sent; the fitter's report of 16 September is not. OPS-42 follows as a search result.
        answer("ans-left-out", LEFT_OUT_QUESTION, cut(flow, {0, 1, 4, 7, 13}) + [tp("OPS-42")[0]],
               judge_answer(cite=None, wanted=[SAYS_NOT_SENT], extra=left_out_answer)),
        # ---- ask-action
        action("act-create", CREATE_REQUEST, [tk["OPS-41"], S.PAGES["hose-leak"]], {"count": 1, "op": "create_task", "check": create_check}),
        action("act-three", "set all three to high priority", [tk["OPS-42"], tk["OPS-46"], tk["KSW-8"], tk["OPS-43"]],
               {"count": 3, "op": "update_task", "targets": ["t1", "t2", "t3"], "check": three_check}, turns=FIND_TURNS),
        action("act-comment", "add a comment to OPS-41: the SL-75 sleeve held for two weeks, no wear", [tk["OPS-41"], tk["OPS-47"]],
               {"count": 1, "op": "comment", "targets": ["t1"], "check": comment_check}),
        action("act-ambig", "mark the Kestrel task as done", [tk["OPS-46"], tk["KSW-7"], tk["KSW-8"], tk["KSW-10"]],
               {"count": 0, "reply": True, "check": ambig_check}),
        action("act-delete", "delete OPS-45, it's done anyway", [tk["OPS-45"]],
               {"count": 0, "reply": True, "reply_has": ["can't", "cannot", "can not", "not possible", "unable", "no way", "isn't possible", "don't"]}),
        # ---- ask-action with the turn's sources (D-83): KSW-12 referenced, KSW-10 found by the search
        action("act-from-latest", WIFI_REQUEST, [WIFI_TASK, tk["KSW-10"]], {"count": 1, "op": "create_task", "check": from_latest_check},
               pieces=task_pieces_of(WIFI_TASK) + [tp("KSW-10")[0]]),
        action("act-left-out", WIFI_REQUEST, [WIFI_TASK, tk["KSW-10"]], {"count": 0, "reply": True, "check": left_out_action_check},
               pieces=cut(task_pieces_of(WIFI_TASK), {0, 1, 2, 3}) + [tp("KSW-10")[0]]),
        action("act-create-src", CREATE_REQUEST, [tk["OPS-41"], S.PAGES["hose-leak"]], {"count": 1, "op": "create_task", "check": create_src_check},
               pieces=tp("OPS-41")[:3] + [pg("hose-leak", "Cause"), pg("hose-leak", "Fix")]),
        # both referenced, so both sent whole: the page first (pages before tasks, references.rs)
        action("act-quote-page", QUOTE_REQUEST, [tk["OPS-41"], S.PAGES["hose-tester"]],
               {"count": 1, "op": "comment", "targets": ["t1"], "check": quote_page_check},
               pieces=S.page_pieces("hose-tester") + tp("OPS-41")),
        # ---- ask-page, edit-page, doc-terms
        {"cid": "page-from-chat", "template": "ask-page", "stream": True, "judge": judge_page,
         "messages": page_messages("make a page about what we know on EP-40 calibration", "Site operations", "", PAGE_CONTENT,
                                   [pg("ep40", "Calibration"), tp("OPS-42")[0], pg("ep40", "Safety"), pg("barcode-reader", "Setup")])},
        # OPS-48 cut to what the request needs (description, symptoms, meter check, gassing test), without the note, as
        # ask-page gets it; then the calibration section as a search result.
        {"cid": "page-left-out", "template": "ask-page", "stream": True, "judge": judge_page_left_out,
         "messages": page_messages(LEFT_OUT_PAGE_REQUEST, "Site operations", "", LEFT_OUT_PAGE_CONTENT,
                                   without_notes(cut(flow, {0, 1, 2, 6})) + [pg("ep40", "Calibration")])},
        {"cid": "edit-page", "template": "edit-page", "stream": True, "judge": judge_edit,
         "messages": edit_messages(S.PAGES["hose-tester"]["body"], EDIT_CHANGE)},
        # The change step's page edit: OPS-48 cut with the meter check sent (the note on it), everything after left out.
        {"cid": "edit-left-out", "template": "edit-page", "stream": True, "judge": judge_edit_left_out,
         "messages": edit_messages(S.PAGES["ep40"]["body"], LEFT_OUT_EDIT_CHANGE, cut(flow, {0, 1, 2}))},
        {"cid": "doc-terms", "template": "doc-terms", "json": True, "judge": judge_doc_terms, "doc": S.SHOT_REPORT["text"],
         "messages": doc_terms_messages(S.SHOT_REPORT["name"], S.SHOT_REPORT["text"])},
        # ---- web probes
        {"cid": "web-plain", "template": "ask-web", "web": True, "stream": False, "question": WEB_QUESTION, "messages": web_messages()},
        {"cid": "web-stream", "template": "ask-web", "web": True, "stream": True, "question": WEB_QUESTION, "messages": web_messages()},
        {"cid": "web-stable", "template": "ask-web", "web": True, "stream": False, "question": WEB_STABLE_QUESTION,
         "messages": web_messages(WEB_STABLE_QUESTION)},
        # the re-answer after a refusal: rendered with the no-search text, called through the sidecar without web search
        {"cid": "web-off", "template": "ask-web", "stream": False, "judge": judge_web_off,
         "messages": web_messages(WEB_QUESTION, web=False)},
    ]
    return {c["cid"]: c for c in checks}


# ------------------------------------------------------------------ running

def complete_json(sc, target, msgs, meta):
    """A JSON call with the one check-retry the app does. Returns the final reply text."""
    r, meta["s"], _ = sc.complete(target, msgs)
    meta["json"] = P.parse_json(r["text"])
    if meta["json"] is None:
        meta["retried"] = True
        msgs = msgs + [{"role": "assistant", "content": r["text"]}] + P.render("check-retry")
        r, s2, _ = sc.complete(target, msgs)
        meta["s"] += s2
        meta["json"] = P.parse_json(r["text"])
    meta["usage"] = r.get("usage")
    return r["text"]


def run_check(sc, target, c):
    meta = {}
    if c.get("json"):
        text = complete_json(sc, target, c["messages"], meta)
        v = meta["json"]
        if c["template"] == "ask-plan":
            meta["plan"], meta["plan_notes"] = F.check_plan(v, S.CATEGORIES, c["in_scope"], c["attachment"])
        elif c["template"] == "ask-action":
            meta["changes"], meta["dropped"], meta["reply"] = F.check_action(v, c["labels"], S.CATEGORIES, c["in_scope"])
            created = [x for x in meta["changes"] if x["op"] == "create_task"]
            if created:
                page = created[0]["title"] + "\n" + created[0]["description"]
                meta["terms"], meta["term_dropped"] = P.filter_terms(created[0]["terms"], page, entries_for([created[0]["category"]]))
            else:
                meta["terms"], meta["term_dropped"] = [], []
        elif c["template"] == "doc-terms":
            meta["terms"], meta["term_dropped"] = P.filter_terms((v or {}).get("terms"), c["doc"], entries_for(ALL_SCOPE))
        return text, meta
    r, meta["s"], meta["first"] = sc.complete(target, c["messages"], stream=c.get("stream", False))
    meta["usage"] = r.get("usage")
    meta["finish"] = r.get("finish_reason")
    text = r["text"]
    if c["template"] == "ask-answer":
        cleaned, meta["cited"] = F.markers(text, c["n_sources"])
        meta["invalid"] = sorted({int(n) for n in re.findall(r"\[(\d+)\]", text) if not 1 <= int(n) <= c["n_sources"]})
        meta["shown"] = cleaned
    return text, meta


# ------------------------------------------------------------------ the web probes (API shape discovery, not prompt judgements)

def _dump(o):
    if hasattr(o, "model_dump"):
        try:
            return o.model_dump()
        except Exception:  # noqa: BLE001
            pass
    if isinstance(o, dict):
        return o
    try:
        return json.loads(json.dumps(o, default=str))
    except Exception:  # noqa: BLE001
        return str(o)


def _search_count(usage):
    u = _dump(usage) or {}
    stu = u.get("server_tool_use") if isinstance(u, dict) else None
    if isinstance(stu, dict) and stu.get("web_search_requests") is not None:
        return stu["web_search_requests"]
    ptd = u.get("prompt_tokens_details") if isinstance(u, dict) else None
    if isinstance(ptd, dict) and ptd.get("web_search_requests") is not None:
        return ptd["web_search_requests"]
    return 0


def web_probe(target, cid, stream, question, out_dir):
    """One call with web search on, through the sidecar's own build_call. Returns the summary line; writes the raw shapes."""
    from cairn_sidecar import ai
    from cairn_sidecar.errors import from_exception
    t = ai.Target(target)
    api, kw = ai.build_call(t, web_messages(question), stream=stream)
    record = {"route": kw.get("model"), "api": api, "stream": stream, "question": question}
    path = os.path.join(out_dir, f"{cid}.json")
    if api != "chat":
        record["note"] = "Responses API route: LiteLLM's web_search_options is a Chat Completions parameter; the Responses tool {'type': 'web_search'} was not probed."
        json.dump(record, open(path, "w"), indent=1, default=str)
        return "skipped: Responses API route (see the json)"
    kw["web_search_options"] = {"search_context_size": "low"}
    lm = ai.litellm()
    try:
        r = lm.completion(**kw)
        if not stream:
            d = _dump(r)
            msg = r.choices[0].message
            psf = _dump(getattr(msg, "provider_specific_fields", None) or {}) or {}
            record.update({"text": msg.content or "", "finish_reason": r.choices[0].finish_reason, "usage": _dump(r.usage),
                           "provider_specific_fields": psf, "raw": d})
            cites = sum(len(x) if isinstance(x, list) else 1 for x in (psf.get("citations") or []))
            results = len(psf.get("web_search_results") or [])
        else:
            text, chunks, cites, results, usage, finish = [], [], 0, 0, None, None
            for ev in r:
                d = _dump(ev)
                if len(chunks) < 60:
                    chunks.append(d)
                if ev.choices:
                    delta = ev.choices[0].delta
                    text.append(delta.content or "")
                    finish = ev.choices[0].finish_reason or finish
                    psf = _dump(getattr(delta, "provider_specific_fields", None) or {}) or {}
                    if psf.get("citation"):
                        cites += 1
                    if psf.get("web_search_results"):
                        results = max(results, len(psf["web_search_results"]))
                if getattr(ev, "usage", None):
                    usage = _dump(ev.usage)
            record.update({"text": "".join(text), "finish_reason": finish, "usage": usage, "chunks": chunks,
                           "citation_chunks": cites, "web_search_results": results})
        n = _search_count(record.get("usage"))
        record["summary"] = f"searched {n} times, {cites} citations, {results} search result blocks" if n else "no search was made (the provider ignored the option)"
    except Exception as e:  # noqa: BLE001
        err = from_exception(e, t.secrets())
        record["error"] = {"class": type(e).__name__, "status": getattr(e, "status_code", None), "kind": err.kind,
                           "litellm_type": getattr(e, "llm_provider", None), "message": err.message}
        record["summary"] = f"refused: {record['error']['status']} {err.kind} {err.message}"
    json.dump(record, open(path, "w"), indent=1, default=str, ensure_ascii=False)
    return record["summary"]


# ------------------------------------------------------------------ main

def safe(name):
    return re.sub(r"[^\w.-]+", "_", name)


def dry_run(checks):
    for cid, c in checks.items():
        n = sum(len(m["content"]) for m in c["messages"])
        roles = "".join(m["role"][0] for m in c["messages"])
        print(f"{cid:<16} {c['template']:<11} {len(c['messages'])} messages ({roles}), {n:>6} chars, ~{n // 4:>5} tokens")
    print(f"\n{len(checks)} checks rendered without errors")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("config", nargs="?")
    ap.add_argument("--only", help="one target name")
    ap.add_argument("--check", nargs="*", help="run only these checks")
    ap.add_argument("--dry-run", action="store_true", help="render every check and print its size; call no model")
    a = ap.parse_args()
    checks = build_checks()
    unknown = [x for x in a.check or [] if x not in checks]
    if unknown:
        sys.exit(f"unknown checks: {unknown}; known: {list(checks)}")
    if a.check:
        checks = {k: checks[k] for k in a.check}
    if a.dry_run:
        dry_run(checks)
        return
    if not a.config:
        sys.exit("a config file is needed unless --dry-run")
    configs = [c for c in tomllib.load(open(a.config, "rb"))["target"] if not a.only or c["name"] == a.only]
    if not configs:
        sys.exit("no target with that name in the config")
    Sidecar = h_sidecar()
    for cfg in configs:
        target = lc.to_target(cfg, lc.get_key(cfg))
        out_dir = os.path.join(OUT, safe(cfg["name"]))
        os.makedirs(out_dir, exist_ok=True)
        print(f"\n==================== {cfg['name']} ({cfg['model']}, {target['format']})")
        summary, sc = [], Sidecar()

        def say(line):
            print(line)
            summary.append(line)
        try:
            for cid, c in checks.items():
                if c.get("web"):
                    try:
                        say(f"WEB  {cid:<16} {web_probe(target, cid, c['stream'], c['question'], out_dir)}")
                    except Exception as e:  # noqa: BLE001
                        say(f"ERR  {cid:<16} {type(e).__name__}: {str(e)[:300]}")
                    continue
                try:
                    text, meta = run_check(sc, target, c)
                except Exception as e:  # noqa: BLE001
                    say(f"ERR  {cid:<16} {str(e)[:300]}")
                    continue
                results = c["judge"](text, meta)
                bad = [label for ok, label in results if not ok]
                u = meta.get("usage") or {}
                timing = f"{meta['s']:.1f} s" + (f", text after {meta['first']:.1f} s" if meta.get("first") else "")
                say(f"{'PASS' if not bad else 'FAIL'} {cid:<16} {len(results) - len(bad)}/{len(results)}  "
                    f"({timing}, {u.get('input_tokens', '?')} in / {u.get('output_tokens', '?')} out)")
                for label in bad:
                    say(f"       - {label}")
                for t in meta.get("terms") or []:
                    say(f"       term: {t['term']} [{t['confidence']}] {t['meaning']}")
                for name, why in meta.get("term_dropped") or []:
                    say(f"       filter dropped: {name} ({why})")
                with open(os.path.join(out_dir, f"{cid}.md"), "w", encoding="utf-8") as f:
                    f.write(f"<!-- {cid} | {timing} | usage {u} | finish {meta.get('finish')} -->\n{text}\n")
                    if meta.get("json") is not None:
                        f.write("\n<!-- parsed -->\n" + json.dumps(meta["json"], ensure_ascii=False, indent=2) + "\n")
                    for k in ("plan", "changes", "dropped", "reply", "cited", "invalid"):
                        if k in meta:
                            f.write(f"\n<!-- app-side {k} -->\n" + json.dumps(meta[k], ensure_ascii=False, default=str) + "\n")
                    f.write("\n<!-- judged -->\n" + "\n".join(f"{'ok ' if ok else 'BAD'} {label}" for ok, label in results) + "\n")
                with open(os.path.join(out_dir, f"{cid}.sent.json"), "w", encoding="utf-8") as f:
                    json.dump(c["messages"], f, ensure_ascii=False, indent=1)
        finally:
            sc.close()
        with open(os.path.join(out_dir, "summary.txt"), "w") as f:
            f.write("\n".join(summary) + "\n")
    print(f"\nFull replies: {OUT}/<target>/")


if __name__ == "__main__":
    main()
