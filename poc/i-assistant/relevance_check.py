#!/usr/bin/env python
"""Calibrates the assistant's minimum relevance (FR-LLM-AS-1, D-44) with the real search model (local, no LLM).

    nice sidecar/.venv/bin/python poc/i-assistant/relevance_check.py

Builds a small made-up workspace (pages cut at headings, tasks as one piece, glossary lines appended to each piece as
the index does: vectors.rs `embed_text`, FR-RAG-IX-3), embeds it with `python -I -m cairn_sidecar --role embed`
(EmbeddingGemma q4, 256 dims, as the app), and scores each question against every item (best piece, cosine).
Each question is scored twice: as the first step (`ask-plan`) would pass it (focused search words) and as typed.
Prints, per question, the relevant items' scores and the best irrelevant ones, then precision/recall per threshold,
and the same with the glossary lines left out (does a match through glossary lines alone lift unrelated items?).
The keyword side runs the app's FTS5 query shape (words ANDed, last word a prefix; bm25 weights title 10, body 1,
glossary 3) and marks hits that matched only through glossary lines.
"""
import base64, json, os, re, sqlite3, subprocess, sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
MODEL_DIR = os.path.join(HERE, "..", "..", "sidecar", "models", "embeddinggemma-300m")
sys.path.insert(0, os.path.join(HERE, "..", "h-create"))
from prompts import occurs  # noqa: E402  (FR-KN-GL-4 matching, as the app)

# ------------------------------------------------------------------ the made-up workspace

GLOSSARY = [  # (term, other names, description, category or None for global)
    ("MMU", ["Mobile Manufacturing Unit"], "Truck that mixes bulk explosive on site and pumps it into the blast holes.", None),
    ("auger", [], "The arm on an MMU that delivers the explosive into the blast hole.", None),
    ("AN", ["ammonium nitrate"], "Oxidiser; the main ingredient of bulk explosive.", None),
    ("Kestrel", [], "The site's blast design and loading app; loading records are entered there.", "OPS"),
    ("P3W", ["Pit 3 West"], "Pit 3 West, a mining area at the site.", "OPS"),
    ("MMU", ["memory management unit"], "CPU part that maps virtual addresses to physical memory.", "LAB"),
    ("SSO", ["single sign-on"], "Signing in once with the work account for all company apps.", None),
]

PAGES = {  # id: (category, Markdown)
    "p1": ("OPS", """# EP-40 flow meter calibration
## When
Calibrate the flow meter every 250 operating hours, and always after replacing the stator.
## Steps
1. Flush the line with water until no emulsion comes out.
2. Run water at 120 kg/min for 2 minutes into the calibration tank.
3. Weigh the tank. The weight must be within 2% of the meter reading.
4. Otherwise enter the new K-factor in the MMU controller under Settings > Flow. Factory K-factor: 0.982."""),
    "p2": ("OPS", """# EP-40 pump maintenance intervals
| Part | Interval | Note |
|---|---|---|
| Auger slew bearing | weekly | 6 strokes of EP2 grease |
| Pump stator | 1,500 hours | replace, then calibrate |
| Inlet strainer | daily | clean at the end of shift |
Torque the pump mounting bolts to 210 Nm after every stator change."""),
    "p3": ("OPS", """# MMU-07 auger hose repair
## Problem
The auger swivel hose on MMU-07 split three times since August, each time at the elbow.
## Cause
With the auger slewed fully left, the hose rubs on the boom clamp.
## Fix
Moved the boom clamp 40 mm towards the cab and added a protective sleeve (SL-75). Pressure tested at 12 bar for 10 minutes."""),
    "p4": ("OPS", """# Wet hole loading at P3W
Holes at P3W often hold water after rain. Dewater first, then prime with the booster at the bottom of the column.
Load within 30 minutes of dewatering and use the wet-hole blend (more AN prill is not allowed in wet holes).
Record every dewatered hole in Kestrel."""),
    "p5": ("OPS", """# Kestrel: entering loading records
After each pattern, open Kestrel on the tablet, pick the pattern number and enter the kilograms per hole.
Holes that were not loaded get the reason code NL. Sync before the end of shift; the supervisor approves the records."""),
    "p6": ("OPS", """# Pre-start checks for MMUs
Before the first MMU leaves the depot: AN tank level, hoses and couplings, pump pressure, gassing flow, lights and horn.
Sign the logbook. A failed check means the truck stays at the depot until the fitter clears it."""),
    "p7": ("LIC", """# Contactless reader setup for the badge kiosk
## Connecting
Plug the reader into the kiosk's USB-C port. It shows a green light when the driver is loaded.
## Using it
Hold the badge flat on the reader for one second. A beep means the badge was read; two beeps mean an unknown badge.
## Problems
If nothing happens, unplug the reader for ten seconds and check that the kiosk app has reader access enabled."""),
    "p8": ("LIC", """# Importing licence data from CSV
Use the Import button in the portal and pick the CSV file. The first row must hold the column names.
Dates must use YYYY-MM-DD. Files over 50,000 rows are split automatically."""),
    "p9": ("LIC", """# Search over licence documents
The portal uses a search index over the licence PDFs. To use it, type a few words; results show the matching clause.
Re-index after uploading new contracts."""),
    "p10": ("LIC", """# Licence renewal process
Licences are renewed every year in March. Start four weeks ahead: ask the vendor for a quote, get the budget owner's
approval, then raise the purchase order. Upload the new licence key in the portal."""),
    "p11": ("LIC", """# Portal SSO login problems
If the portal keeps asking you to sign in, clear the browser's cookies for the portal and sign in with SSO again.
Sessions end after 10 minutes without activity; this is a known issue (see the open task)."""),
    "p12": ("LAB", """# MMU page faults on the Raspberry Pi
The kernel log showed MMU page faults after enabling the 64-bit kernel. Updating the firmware and reducing the
overclock fixed it."""),
    "p13": ("LAB", """# Setting up the NAS backup
The NAS backs up the laptop every night at 02:00 over SMB. Keep 30 daily snapshots. Test a restore every quarter."""),
    "p14": ("GEN", """# VPN from home
Open the VPN client, choose the Perth gateway and sign in with your work account. Accept the push on your phone.
If it hangs at "connecting", switch from Wi-Fi to a phone hotspot to rule out the home router."""),
    "p15": ("GEN", """# Travel booking rules
Book flights and hotels through the travel desk at least two weeks ahead. Receipts go to the admin team within five days."""),
    "p16": ("OPS", """# Mixing gassing solution
Mix the gassing solution in the blue tank: 20 litres of water to one bag of nitrite. Stir for five minutes.
The mixed solution keeps for 48 hours; label the tank with the mixing time."""),
    "p17": ("LIC", """# Card reader driver on Windows laptops
Install the reader driver from the software centre, then restart. Device Manager lists it under Smart card readers."""),
    "p18": ("GEN", """# Using the office printer
Use your badge at the printer to release print jobs. Colour printing needs the manager's approval."""),
}

TASKS = {  # id: (category, key, title, status, description)
    "t1": ("OPS", "OPS-12", "Replace swivel hose on MMU-03", "blocked", "Waiting for part HX-2210. Same rubbing as on MMU-07."),
    "t2": ("OPS", "OPS-15", "Calibrate EP-40 on MMU-05", "todo", "Due after the stator change; follow the calibration page."),
    "t3": ("LIC", "LIC-4", "Contactless reader firmware update", "in_progress", "Vendor firmware 2.3 fixes the double beep on some badges."),
    "t4": ("LIC", "LIC-7", "Licence key export fails for large CSV", "blocked", "Export times out over 20,000 rows; waiting for the vendor."),
    "t5": ("LIC", "LIC-9", "Renew the reporting licence", "todo", "Quote requested; budget approval pending."),
    "t6": ("LAB", "LAB-2", "Replace NAS disk", "done", "Disk 2 had SMART errors; replaced and resilvered."),
    "t7": ("OPS", "OPS-18", "Update Kestrel loading template", "todo", "Add the reason code column for holes not loaded."),
    "t8": ("LIC", "LIC-11", "SSO timeout after 10 minutes", "blocked", "Sessions end too early; the identity team is looking at it."),
}

# (focused search words as the first step would give them, the question as typed, relevant items)
QUESTIONS = [
    ("contactless reader", "How to use the contactless reader?", {"p7", "t3"}),
    ("EP-40 flow meter calibration", "How do I calibrate the EP-40 flow meter?", {"p1", "t2"}),
    ("auger hose cracking", "What did we do about the auger hose cracking?", {"p3", "t1"}),
    ("wet holes P3W loading", "How do we load wet holes at P3W?", {"p4"}),
    ("Kestrel loading records", "How do I enter the loading records?", {"p5", "t7"}),
    ("licence renewal", "When and how do we renew licences?", {"p10", "t5"}),
    ("portal SSO sign-in logged out", "Why does the portal keep logging me out?", {"p11", "t8"}),
    ("Raspberry Pi page faults", "What fixed the page faults on the Pi?", {"p12"}),
    ("NAS backup", "How is the NAS backup set up?", {"p13", "t6"}),
    ("checks before MMU leaves depot", "What do we check before a truck leaves the depot in the morning?", {"p6"}),
    ("VPN from home", "How do I connect to work from home?", {"p14"}),
    ("licence CSV import", "How do I import licence data from a CSV file?", {"p8", "t4"}),
    ("gassing solution mixing", "How do we mix the gassing solution?", {"p16"}),
    ("arm that delivers explosive hose problem", "Any problems with the arm that delivers the explosive?", {"p3", "t1"}),
    ("流量计 校准", "怎么校准 EP-40 流量计？", {"p1", "t2"}),
    # Nothing in the notes answers these; whatever comes back should stay below the minimum relevance.
    ("forklift tyre pressure", "What's the tyre pressure for the forklift?", set()),
    ("Azure cost report", "Where is the monthly Azure cost report?", set()),
    ("canteen contactless payment", "Can I pay contactless at the canteen?", set()),
    ("parental leave policy", "How much parental leave do we get?", set()),
]
# Related but not the answer: neither counted as relevant nor as noise.
RELATED = {"contactless reader": {"p17", "p18"}, "EP-40 flow meter calibration": {"p2"}, "auger hose cracking": {"p2"},
           "流量计 校准": {"p2"}, "portal SSO sign-in logged out": {"p14"}, "canteen contactless payment": {"p7", "t3", "p18"}}


def glossary_lines(category, text):
    """The lines of the entries a piece uses: global entries, and the category's own (which override a global of the same
    term), as glossary::terms_in does."""
    own = {t for t, _, _, p in GLOSSARY if p == category}
    out = []
    for term, others, desc, p in GLOSSARY:
        if p not in (None, category) or (p is None and term in own):
            continue
        if any(occurs(n, text) for n in [term, *others]):
            names = f"{term} ({', '.join(others)})" if others else term
            out.append(f"{names}: {desc}")
    return out


def page_pieces(md):
    """(title, heading, text) per `## ` section (chunking::split_page for short pages)."""
    title = re.match(r"#\s+(.+)", md).group(1)
    out, heading, buf = [], "", []
    for line in md.splitlines()[1:]:
        if line.startswith("## "):
            if "".join(buf).strip():
                out.append((title, heading, "\n".join(buf).strip()))
            heading, buf = line[3:].strip(), []
        else:
            buf.append(line)
    if "".join(buf).strip():
        out.append((title, heading, "\n".join(buf).strip()))
    return out


def items():
    """(item id, category, title, [(title, heading, text)])."""
    out = []
    for pid, (proj, md) in PAGES.items():
        out.append((pid, proj, re.match(r"#\s+(.+)", md).group(1), page_pieces(md)))
    for tid, (proj, key, title, status, desc) in TASKS.items():
        out.append((tid, proj, f"{key} {title}", [(f"{key} {title}", "", f"{key} {title}\nStatus: {status}\n{desc}")]))
    return out


def embed_text(heading, text, terms):
    """vectors.rs `embed_text`."""
    return (heading + "\n" if heading else "") + text + ("\n" + ", ".join(terms) if terms else "")


# ------------------------------------------------------------------ the search model

class Embedder:
    def __init__(self):
        self.p = subprocess.Popen([sys.executable, "-I", "-m", "cairn_sidecar", "--role", "embed"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.n = 0

    def _call(self, method, params):
        self.n += 1
        params = {"model_dir": MODEL_DIR, "dims": 256, "threads": 4, **params}
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params}) + "\n")
        self.p.stdin.flush()
        for line in self.p.stdout:
            m = json.loads(line)
            if m.get("id") == self.n:
                if "error" in m:
                    raise RuntimeError(m["error"])
                return m["result"]
        raise RuntimeError("the embed process exited")

    def documents(self, pairs):
        vs = self._call("embed.documents", {"items": [{"title": t, "text": x} for t, x in pairs]})["vectors"]
        return [np.frombuffer(base64.b64decode(v), dtype="<f4") for v in vs]

    def query(self, text):
        return np.frombuffer(base64.b64decode(self._call("embed.query", {"text": text})["vector"]), dtype="<f4")

    def close(self):
        self.p.stdin.close()
        self.p.wait(30)


# ------------------------------------------------------------------ keyword side (search.rs)

def build_match(words):
    """search.rs `build_match`: each word a quoted phrase, ANDed, the last one a prefix."""
    parts = [w.replace('"', '""') for w in words.split()]
    return " AND ".join(f'"{p}"' for p in parts[:-1]) + (" AND " if len(parts) > 1 else "") + f'"{parts[-1]}"*'


def keyword_index(its):
    db = sqlite3.connect(":memory:")
    db.execute("CREATE VIRTUAL TABLE ix USING fts5(ref, title, body, extra, tokenize='unicode61 remove_diacritics 2')")
    for iid, proj, title, pieces in its:
        body = "\n".join(x for _, _, x in pieces)
        extra = "\n".join(glossary_lines(proj, title + "\n" + body))
        db.execute("INSERT INTO ix VALUES (?, ?, ?, ?)", (iid, title, body, extra))
    return db


def keyword_hits(db, words):
    if re.search(r"[一-鿿]", words):
        return []  # the app spaces Han characters first (M0-c); not modelled here
    q = build_match(words)
    rows = db.execute("SELECT ref FROM ix WHERE ix MATCH ? ORDER BY bm25(ix, 0.0, 10.0, 1.0, 3.0)", (q,)).fetchall()
    out = []
    for (ref,) in rows:
        without = db.execute("SELECT 1 FROM ix WHERE ix MATCH ? AND ref = ?", ("{title body}: (" + q + ")", ref)).fetchone()
        out.append((ref, without is None))
    return out


# ------------------------------------------------------------------ report

def main():
    its = items()
    e = Embedder()
    try:
        vec = {}  # (with glossary?) -> item id -> [piece vectors]
        for with_terms in (True, False):
            vec[with_terms] = {}
            for iid, proj, title, pieces in its:
                pairs = [(t, embed_text(h, x, glossary_lines(proj, t + "\n" + x) if with_terms else [])) for t, h, x in pieces]
                vec[with_terms][iid] = e.documents(pairs)
        rows = []  # (label, mode, relevant scores, noise scores)
        for focused, typed, relevant in QUESTIONS:
            related = RELATED.get(focused, set())
            for mode, text in (("focused", focused), ("typed", typed)):
                q = e.query(text)
                for with_terms in (True, False):
                    s = {iid: max(float(v @ q) for v in vs) for iid, vs in vec[with_terms].items()}
                    rel = sorted(((s[i], i) for i in relevant), reverse=True)
                    noise = sorted(((v, i) for i, v in s.items() if i not in relevant and i not in related), reverse=True)
                    rows.append((focused, mode, with_terms, rel, noise))
    finally:
        e.close()

    print("Best piece cosine per item (with glossary lines, as the index). R = relevant, then the 3 best unrelated items.\n")
    for focused, typed, relevant in QUESTIONS:
        for mode in ("focused", "typed"):
            _, _, _, rel, noise = next(r for r in rows if r[0] == focused and r[1] == mode and r[2])
            words = focused if mode == "focused" else typed
            print(f"  {mode:<7} {words[:44]:<44} R " + (" ".join(f"{v:.2f}:{i}" for v, i in rel) or "-") +
                  "  | noise " + " ".join(f"{v:.2f}:{i}" for v, i in noise[:3]))
        print()

    for with_terms in (True, False):
        print(f"Precision / recall by minimum relevance ({'with' if with_terms else 'without'} glossary lines in the pieces):")
        for mode in ("focused", "typed"):
            sel = [r for r in rows if r[1] == mode and r[2] == with_terms]
            rel_all = [v for r in sel for v, _ in r[3]]
            noise_all = [v for r in sel for v, _ in r[4]]
            print(f"  {mode}: relevant min {min(rel_all):.2f} median {np.median(rel_all):.2f}; "
                  f"noise max {max(noise_all):.2f}, 99th pct {np.percentile(noise_all, 99):.2f}")
            for t in np.arange(0.30, 0.62, 0.02):
                kept_rel = sum(v >= t for v in rel_all)
                kept_noise = sum(v >= t for v in noise_all)
                prec = kept_rel / max(1, kept_rel + kept_noise)
                print(f"    {t:.2f}: recall {kept_rel}/{len(rel_all)}  noise kept {kept_noise:>3}  precision {prec:.2f}")
        print()

    print("Floor plus a margin below the question's best item (with glossary lines; focused words):")
    sel = [r for r in rows if r[1] == "focused" and r[2]]
    for floor in (0.38, 0.40, 0.42, 0.44):
        for margin in (None, 0.30, 0.25, 0.20):
            kept_rel = kept_noise = total = empty_ok = 0
            for _, _, _, rel, noise in sel:
                best = max([v for v, _ in rel] + [v for v, _ in noise])
                cut = floor if margin is None else max(floor, best - margin)
                kept_rel += sum(v >= cut for v, _ in rel)
                kept_noise += sum(v >= cut for v, _ in noise)
                total += len(rel)
                empty_ok += not rel and not any(v >= cut for v, _ in noise)
            print(f"  floor {floor:.2f} margin {'-' if margin is None else f'{margin:.2f}'}: recall {kept_rel}/{total}  "
                  f"noise kept {kept_noise:>3}  unanswerable with nothing sent {empty_ok}/{sum(1 for r in sel if not r[3])}")
    print()

    print("Keyword side (focused words, ANDed, last word a prefix); * = matched only through glossary lines:")
    db = keyword_index(its)
    for focused, _, relevant in QUESTIONS:
        hits = keyword_hits(db, focused)
        shown = " ".join(f"{ref}{'*' if only_gl else ''}{'' if ref in relevant else '(x)'}" for ref, only_gl in hits)
        print(f"  {focused:<44} {shown or '-'}")
    print("  (x) = not a relevant item")


if __name__ == "__main__":
    main()
