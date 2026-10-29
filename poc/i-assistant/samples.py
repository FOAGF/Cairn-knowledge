"""A made-up workspace for the M6 prompt check, in the style of the M5 samples: mining and explosives field operations
(MMU, auger, AN, P3W, Kestrel), a software and licensing category for the Kestrel app, and a Home lab category where
"MMU" means memory management unit. Everything is invented; MMU and auger are the owner's example terms.

Distractors share ordinary words with the right item ("use", "setup", "reader", "hose") so that citation discipline can
be judged: an answer that cites them for something they do not say fails.

Items are plain dicts (formats.py reads them). Dates are fixed; the checks compute relative dates from the real today.
"""

CATEGORIES = [
    {"name": "Site operations", "key": "OPS"},
    {"name": "Kestrel software", "key": "KSW"},
    {"name": "Home lab", "key": "LAB"},
]
NAMES = [p["name"] for p in CATEGORIES]

GLOSSARY = [
    {"term": "MMU", "other": ["Mobile Manufacturing Unit"], "scope": "global",
     "desc": "Truck that mixes bulk explosive on site and pumps it into the blast holes."},
    {"term": "MMU", "other": ["memory management unit"], "scope": "Home lab",
     "desc": "The part of a processor that maps virtual addresses to physical memory (page tables)."},
    {"term": "auger", "other": [], "scope": "global",
     "desc": "The arm on an MMU that delivers the explosive into the blast hole."},
    {"term": "AN", "other": ["ammonium nitrate"], "scope": "global",
     "desc": "Ammonium nitrate, the oxidiser carried in the MMU's AN tank."},
    {"term": "P3W", "other": ["Pit 3 West"], "scope": "global",
     "desc": "The west wall of pit 3, loaded from the lower bench."},
    {"term": "Kestrel", "other": [], "scope": "global",
     "desc": "The shift handover app: loaded holes, product used and misfires are recorded in it before 06:00. Its reader is the tablet add-on that scans hole tags."},
    {"term": "gassing solution", "other": [], "scope": "global",
     "desc": "The additive the MMU pumps into the emulsion to make it sensitive."},
    {"term": "PM", "other": ["preventive maintenance"], "scope": "Site operations",
     "desc": "The scheduled service of a truck, every 250 operating hours."},
]

# ------------------------------------------------------------------ pages: title, category, group, body (title line included)

PAGES = {
    "kestrel-reader": {
        "title": "Kestrel reader setup on the MMU tablet", "category": "Site operations", "group": "Kestrel & handover",
        "body": """# Kestrel reader setup on the MMU tablet

The Kestrel reader scans the hole tags at the collar, so the loaded holes land in Kestrel without typing.

## Setup
1. Charge the reader fully before the first pairing (about 3 hours).
2. On the tablet, open Kestrel > Settings > Reader and tap "Pair". The pairing PIN is 2468.
3. Hold the reader within 1 m of the tablet until the light turns green.
4. Scan the test tag on the back of the tablet mount. Kestrel shows "Reader OK".

## Rules
- Sync the tablet before 06:00, or the night shift's holes are missing from the handover.
- The reader works only with Kestrel 4.2 or later; older tablets must be updated first.
- A reader that will not pair after three tries goes back to the depot office; do not open it."""},
    "barcode-reader": {
        "title": "Barcode reader setup for the magazine", "category": "Site operations", "group": "Magazine",
        "body": """# Barcode reader setup for the magazine

The magazine stock count uses a wired barcode reader on the office PC, not the Kestrel reader.

## Setup
1. Plug the reader into the USB port on the left of the PC.
2. Open the stock sheet and put the cursor in the "Scan" cell.
3. Scan the configuration card in the drawer to set the reader to "Enter after scan".

## Use
Scan each case label once. A double beep means the label was scanned already today."""},
    "hose-tester": {
        "title": "How to use the auger hose tester", "category": "Site operations", "group": "Equipment maintenance",
        "body": """# How to use the auger hose tester

The tester checks a replaced auger hose for leaks before the truck goes back to the pit.

## Steps
1. Connect the hose to the tester's outlet and close the far end with the blanking cap.
2. Zero the gauge.
3. Pump up to 12 bar and wait 5 minutes.
4. The pressure must not drop by more than 0.5 bar. A larger drop means a leak: replace the hose, do not patch it.
5. Release the pressure through the bleed valve before disconnecting.

## Care
Store the tester in the workshop cabinet, with the gauge cover on."""},
    "hose-leak": {
        "title": "MMU-07 auger hose leak", "category": "Site operations", "group": "Equipment maintenance",
        "body": """# MMU-07 auger hose leak

The auger on MMU-07 lost pressure on day shift at P3W. The swivel hose at the auger elbow had split along the outer braid.

## Cause
The hose rubbed on the boom clamp when the auger was fully slewed to the left.

## Fix
- Replaced the swivel hose (part HX-2210).
- Moved the boom clamp 40 mm towards the cab and added a protective sleeve (part SL-75).
- Pressure tested at 12 bar for 5 minutes: no leak.

## Follow-up
Record the repair in Kestrel and check the other trucks' boom clamps at their next PM."""},
    "ep40": {
        "title": "EP-40 flow meter calibration", "category": "Site operations", "group": "Equipment maintenance",
        "body": """# EP-40 flow meter calibration

## Safety
Isolate the pump at the MMU control panel and bleed the line pressure before any service work.

## Calibration
Calibrate the flow meter every 250 operating hours, and always after replacing the stator.

1. Flush the line with water until no emulsion comes out.
2. Run water at 120 kg/min for 2 minutes into the calibration tank.
3. Weigh the tank. The weighed amount must be within 2% of the meter reading.
4. If it is not, enter the new K-factor in the MMU controller under Settings > Flow. The factory K-factor is 0.982.

> **Warning:** Never calibrate with emulsion in the line. Emulsion in the calibration tank must be disposed of as explosive waste."""},
    "wet-holes": {
        "title": "P3W wet hole loading", "category": "Site operations", "group": "Blasting procedures",
        "body": """# P3W wet hole loading

The lower bench at P3W collects water after rain, so most holes in the front two rows are wet.

## Procedure
1. Dewater the hole with the pump truck first.
2. Prime with the booster at the bottom of the column.
3. Load within 30 minutes of dewatering, or dewater again.
4. Record the wet holes in Kestrel as "wet, dewatered".

## Limits
Do not load a hole with more than 0.5 m of water left in it."""},
    "winter-zh": {
        "title": "MMU 冬季启动前检查", "category": "Site operations", "group": "Pre-start & inspections",
        "body": """# MMU 冬季启动前检查

气温低于 5 度时，MMU 出车前要做以下检查：

1. AN 储罐加热器开启，罐温不低于 10 度。
2. 乳化基质管路用温水冲洗，确认没有结块。
3. 气化液（gassing solution）流量在怠速下测一次，低于 0.8 L/min 不能出车。
4. 螺旋臂（auger）软管在零下 20 度以下容易开裂，出车前目视检查一遍。

检查结果记在 MMU 日志本上。"""},
    "walkaround": {
        "title": "Depot pre-start walk-around", "category": "Site operations", "group": "Pre-start & inspections",
        "body": """# Depot pre-start walk-around

Every morning before the first MMU leaves the depot:

- AN tank level and heater.
- Auger hoses and the boom clamps.
- Emulsion pump pressure between 8 and 12 bar at idle.
- Gassing solution flow between 0.8 and 1.2 L/min.

Sign the logbook. A truck that fails any item stays in the depot until the fitter has looked at it."""},
    "licence": {
        "title": "Kestrel licence renewal", "category": "Kestrel software", "group": "Licensing",
        "body": """# Kestrel licence renewal

Kestrel is licensed per tablet. We have 40 seats on the licence server, 34 in use.

## Renewal
- The licence runs to 30 November each year. The vendor sends the quote in October; the renewal needs the purchase order before 15 November, or the tablets drop to read-only on 1 December.
- Vendor contact: the support desk, ticket category "Licensing".
- The licence server certificate is separate and expires every two years."""},
    "firmware": {
        "title": "Kestrel reader firmware 3.2 notes", "category": "Kestrel software", "group": "Releases",
        "body": """# Kestrel reader firmware 3.2 notes

Firmware 3.2 for the Kestrel reader fixes the double scan of a tag within 2 seconds and adds the battery level to the tablet screen.

## Rollout
- Update one reader per truck first and run a shift with it.
- Readers on 3.0 must go to 3.1 before 3.2; the updater refuses the direct jump.
- Known issue: after the update the reader needs to be paired again (PIN unchanged)."""},
    "board-crash": {
        "title": "Board crash on boot", "category": "Home lab", "group": "Firmware",
        "body": """# Board crash on boot

The dev board page-faults on boot. The MMU mapping for the DMA buffer is wrong in the bootloader: the buffer is mapped cacheable, so the first DMA transfer reads stale data and the driver walks into an unmapped page.

## Fix
Map the DMA buffer as device memory (non-cacheable) in the bootloader's page tables before enabling the MMU. Tested on two boards, boots cleanly."""},
    "lab-network": {
        "title": "Home lab network setup", "category": "Home lab", "group": "Network",
        "body": """# Home lab network setup

The lab switch has VLAN 10 for the boards and VLAN 20 for the NAS. The boards get addresses from the lab router (10.0.10.50 to 10.0.10.99); the serial console server is on 10.0.10.2.

## Setup of a new board
1. Plug it into ports 1 to 8 (VLAN 10).
2. Add its MAC to the router's reservation list.
3. Use the console server for the first boot; the network comes up only after the bootloader is flashed."""},
}
for _k, _p in PAGES.items():
    _p["id"] = _k

# ------------------------------------------------------------------ tasks

TASKS = {
    "OPS-41": {"title": "MMU-07 auger hose keeps cracking", "category": "Site operations", "status": "in_progress", "priority": "high",
               "type": "bug", "due": "2026-10-20", "labels": ["hose", "mmu-07"], "parent": None, "links": ["OPS-47"],
               "description": "The auger swivel hose on MMU-07 has split three times since August. Each time it fails at the elbow. Find out why and fix it properly.",
               "comments": [("2026-09-03", "Hose split again at the elbow. Replaced it with the same part (HX-2210). Noticed shiny wear marks on the boom clamp."),
                            ("2026-09-10", "Found it: with the auger slewed fully left, the hose rubs on the boom clamp. Moved the clamp 40 mm towards the cab and added a protective sleeve (part SL-75). Pressure tested at 12 bar for 5 min, no leak."),
                            ("2026-09-24", "Two weeks later, no new wear. MMU-03 and MMU-05 have the same clamp position, so check them at their next PM.")]},
    "OPS-42": {"title": "Replace the EP-40 stator on MMU-05", "category": "Site operations", "status": "todo", "priority": "medium",
               "type": "general", "due": "2026-10-22", "labels": ["pump"], "parent": None, "links": [],
               "description": "MMU-05 is at 1,480 hours; the stator is due at 1,500. Calibrate the flow meter afterwards.", "comments": []},
    "OPS-43": {"title": "Order EP2 grease for the auger bearings", "category": "Site operations", "status": "todo", "priority": "low",
               "type": "general", "due": "2026-10-17", "labels": ["parts"], "parent": None, "links": [],
               "description": "Two cartons; the depot has four cartridges left.", "comments": []},
    "OPS-44": {"title": "P3W bench access berm", "category": "Site operations", "status": "blocked", "priority": "high",
               "type": "general", "due": "2026-10-16", "labels": ["p3w"], "parent": None, "links": [],
               "description": "The berm on the lower bench edge at P3W must be rebuilt before the MMU can park there for the inner row.",
               "comments": [("2026-10-13", "Blocked: waiting for the dozer, which is on the haul road until Thursday.")]},
    "OPS-45": {"title": "Laminated copies of the pre-start checklist", "category": "Site operations", "status": "done", "priority": "low",
               "type": "general", "due": None, "labels": [], "parent": None, "links": [],
               "description": "One per truck cab.", "comments": [("2026-10-02", "Done, six copies in the cabs.")]},
    "OPS-46": {"title": "Kestrel tablet mount for MMU-03", "category": "Site operations", "status": "todo", "priority": "medium",
               "type": "feature", "due": "2026-10-23", "labels": ["kestrel"], "parent": None, "links": [],
               "description": "The tablet slides off the dash on the haul road. Fit the same mount as MMU-07 (the one with the test tag on the back).", "comments": []},
    "OPS-47": {"title": "Check boom clamps on MMU-03 and MMU-05", "category": "Site operations", "status": "todo", "priority": "medium",
               "type": "general", "due": None, "labels": ["hose"], "parent": None, "links": ["OPS-41"],
               "description": "Same clamp position as MMU-07 (see OPS-41). Check for wear marks at the next PM and move the clamp if needed.", "comments": []},
    "KSW-7": {"title": "Renew the Kestrel licences", "category": "Kestrel software", "status": "blocked", "priority": "high",
              "type": "general", "due": "2026-11-01", "labels": ["licensing"], "parent": None, "links": ["KSW-9"],
              "description": "40 seats. The quote has arrived; the purchase order is needed before 15 November.",
              "comments": [("2026-10-10", "Blocked: the purchase order is waiting for the cost centre sign-off.")]},
    "KSW-8": {"title": "Kestrel reader firmware 3.2 rollout", "category": "Kestrel software", "status": "in_progress", "priority": "medium",
              "type": "feature", "due": "2026-10-21", "labels": ["reader"], "parent": None, "links": [],
              "description": "One reader per truck first, then the rest. Readers on 3.0 go to 3.1 first.",
              "comments": [("2026-10-14", "MMU-07's reader is on 3.2 and ran a shift without a double scan.")]},
    "KSW-9": {"title": "Licence server certificate expiring", "category": "Kestrel software", "status": "blocked", "priority": "medium",
              "type": "bug", "due": "2026-10-30", "labels": ["licensing", "server"], "parent": None, "links": ["KSW-7"],
              "description": "The certificate on the licence server expires on 2 November. Blocked on the IT ticket for a new certificate.", "comments": []},
    "KSW-10": {"title": "Kestrel reader crashes on sync", "category": "Kestrel software", "status": "todo", "priority": "high",
               "type": "bug", "due": None, "labels": ["reader"], "parent": None, "links": [],
               "description": "Two readers restart when the tablet syncs with more than 200 holes. Happens on 3.1 only so far.", "comments": []},
    "LAB-3": {"title": "Fix the MMU page table setup in the bootloader", "category": "Home lab", "status": "in_progress", "priority": "low",
              "type": "bug", "due": None, "labels": ["bootloader"], "parent": None, "links": [],
              "description": "The DMA buffer must be mapped non-cacheable before the MMU is enabled.", "comments": []},
    "LAB-4": {"title": "Replace the lab switch", "category": "Home lab", "status": "wont_do", "priority": "low",
              "type": "general", "due": None, "labels": [], "parent": None, "links": [],
              "description": "The old switch is fine once the fan was cleaned.", "comments": []},
}
for _k, _t in TASKS.items():
    _t["key"] = _k


def task_pieces(key):
    """A task's pieces as the search chunks them: the description, then each comment."""
    t = TASKS[key]
    out = [{"item": t, "part": "description", "text": t["description"]}]
    out += [{"item": t, "part": ("comment", d), "text": c} for d, c in t["comments"]]
    return out


def page_pieces(pid):
    """A page's pieces, one per heading section (the title line is not a piece)."""
    p = PAGES[pid]
    out, path, buf = [], [], []
    lines = p["body"].split("\n")[1:]

    def flush():
        text = "\n".join(buf).strip()
        if text:
            out.append({"item": p, "part": list(path), "text": text})
        buf.clear()
    for line in lines:
        if line.startswith("## "):
            flush()
            path = [line[3:].strip()]
        else:
            buf.append(line)
    flush()
    return out


def page_piece(pid, heading=None):
    """One section of a page, or the whole body (after the title) as one piece when heading is None."""
    p = PAGES[pid]
    if heading is None:
        body = p["body"].split("\n", 1)[1].strip()
        return {"item": p, "part": [], "text": body}
    return next(x for x in page_pieces(pid) if x["part"] == [heading])


# ------------------------------------------------------------------ an attached file (for file_question and doc-terms)

TORQUE_SHEET = {
    "name": "torque-sheet.pdf",
    "parts": [
        "EP-40 pump: bolt torques (field sheet)\n\nStator housing bolts: 210 Nm, in a star pattern, after every stator change.\nInlet flange bolts: 85 Nm.\nMounting bolts to the MMU frame: 160 Nm, check at every PM.",
        "Notes\n\nUse a calibrated torque wrench; the depot wrench is calibrated every six months (sticker on the handle).\nDo not re-use the stator housing bolts more than twice.",
    ],
}

SHOT_REPORT = {
    "name": "shot-report-2214.txt",
    "text": """Shot report, pattern 2214, P3W lower bench, 14 October

Holes: 86 drilled, 86 loaded, 4 wet (dewatered first). Product: 18.4 t emulsion, 1.2 t AN prill in the dry collars.
Timing was laid out in Tanager and exported to the firing box at 11:40; the shot fired at 12:05.
Misfires: none. Flyrock: none past the exclusion line.
The PDR (post-detonation report, the sheet the shotfirer fills in within an hour of the shot) was handed to Kestrel at 13:10.
Note for next time: the dust from the inner row drifts towards the haul road; a water cart on the ramp before firing would help.
Fragmentation was fine; the digger had no oversize at the face by the end of the shift.""",
}

# Never acceptable as suggested terms from the shot report or the create-task request. Wanted: Tanager (unknown),
# PDR (high: the text explains it), SiteTrak (unknown, in the create-task request).
NOT_TERMS = {"mmu", "auger", "an", "p3w", "kestrel", "pm", "gassing solution", "emulsion", "prill", "shot", "shot report",
             "pattern", "pattern 2214", "misfire", "misfires", "flyrock", "fragmentation", "oversize", "haul road", "water cart",
             "exclusion line", "firing box", "shotfirer", "collar", "collars", "dry collars", "digger", "face", "bench", "lower bench",
             "inner row", "wet", "dewatered", "t", "kg", "hx-2210", "sl-75", "hose", "spare hose", "swivel hose", "mmu-07", "ep-40",
             "ep2", "boom clamp", "depot", "fitter", "pdf", "vpn", "ops-41", "high priority"}
