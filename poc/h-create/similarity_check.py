#!/usr/bin/env python
"""Calibrates the M5 similarity thresholds with the real search model (local, no LLM):
- Settings › Knowledge "likely the same topic" (`duplicateScore`, FR-KN-DU-1): page vs page, scored as the
  duplicate check does (best cosine over piece pairs, pieces cut at headings);
- the merge screen's block actions (`merging::IGNORE_AT`, `REPLACE_AT`, FR-KN-DU-3): block vs block.

    sidecar/.venv/bin/python poc/h-create/similarity_check.py

Starts `python -I -m cairn_sidecar --role embed` (EmbeddingGemma q4, 256 dims, as the app) and prints the scores
per expected class, so a threshold can be put between them. Texts are made up, in the style of the samples.
"""
import base64, json, os, re, subprocess, sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
MODEL_DIR = os.path.join(HERE, "..", "..", "sidecar", "models", "embeddinggemma-300m")

CALIB = """# EP-40 flow meter calibration
## When
Calibrate the flow meter every 250 operating hours, and always after replacing the stator.
## Steps
1. Flush the line with water until no emulsion comes out.
2. Run water at 120 kg/min for 2 minutes into the calibration tank.
3. Weigh the tank; the weight must be within 2% of the meter reading.
4. Otherwise enter the new K-factor in the MMU controller under Settings > Flow. The factory K-factor is 0.982.
> Never calibrate with emulsion in the line."""

CALIB_REWORDED = """# Calibrating the EP-40 pump's flow meter
The flow meter needs calibrating after 250 hours of running and whenever the stator has been changed.
## Procedure
- Flush with water first; there must be no emulsion left in the line.
- Pump water into the calibration tank for two minutes at 120 kg/min.
- Weigh it. If the difference to the meter is more than 2%, put the new K-factor into the controller (Settings > Flow). Factory value: 0.982.
## Warning
Emulsion in the line during calibration is not allowed."""

MAINT = """# EP-40 pump maintenance
## Intervals
| Part | Interval | Note |
|---|---|---|
| Auger slew bearing | weekly | 6 strokes of EP2 grease |
| Pump stator | 1,500 hours | replace, then calibrate |
| Inlet strainer | daily | clean at the end of shift |
## Bolts
Torque the pump mounting bolts to 210 Nm after every stator change."""

HOSE_NOTE = """# MMU-07 auger hose leak
The auger on MMU-07 lost pressure on day shift at P3W. The swivel hose at the auger elbow had split along the outer braid.
## Cause
The hose rubbed on the boom clamp when the auger was fully slewed to the left.
## Fix
- Replaced the swivel hose (part HX-2210).
- Moved the boom clamp 40 mm towards the cab and added a protective sleeve.
- Pressure tested at 12 bar for 5 minutes: no leak."""

HOSE_TASK = """# MMU-07 auger swivel hose cracking at the elbow
## Problem
The auger swivel hose on MMU-07 split three times since August, each time at the elbow. Replacing it with the same part (HX-2210) did not stop the failures.
## Cause
With the auger slewed fully left, the hose rubs on the boom clamp.
## Fix
- Moved the boom clamp 40 mm towards the cab.
- Added a protective sleeve (part SL-75).
- Pressure tested at 12 bar for 5 min, no leak.
## Follow-up
MMU-03 and MMU-05 have the same clamp position: check them at their next PM."""

PRESTART = """# Pre-start checks for the MMU
Do these checks at the depot before every shift.
## Walk-around
1. Check the AN tank level on the gauge.
2. Inspect the auger hoses for cracks and leaks.
3. Run the emulsion pump for 30 seconds and read the pressure.
## Limits
| Check | Limit | If outside |
|---|---|---|
| AN tank level | at least 20% | refill at the depot |
| Emulsion pump pressure | 8 to 12 bar | stop and report |"""

WINTER_ZH = """# MMU 冬季保养记录
## 乳化剂泵
气温低于5度时，乳化剂泵启动前要先预热十分钟，否则压力不稳。
## 螺旋输送臂（auger）软管
软管冬天容易变硬开裂，每周检查一次，发现裂纹立即更换。
## 防冻液
防冻液每月检查一次，冰点要低于零下20度。"""

WINTER_ZH_2 = """# 冬天 MMU 的保养要点
天冷（低于5度）时乳化剂泵要先预热10分钟再启动，不然压力会不稳定。
auger 软管在冬天会变硬、开裂，所以每周都要查一遍，有裂纹就马上换。
防冻液每个月检查，冰点必须低于零下20度。"""

TRAVEL = """# Travel budget for site visits
Site visits next quarter are capped at four trips. Flights must be booked two weeks ahead, and hotel receipts go to
the admin team within five days of returning."""

VPN = """# Laptop VPN from home
## Connect
Open the VPN client, choose the Perth gateway and sign in with your work account. Accept the push on your phone.
## Problems
If it hangs at "connecting", restart the client and switch from Wi-Fi to a phone hotspot to rule out the home router."""

PAGE_PAIRS = [
    ("same", "calibration, reworded", CALIB, CALIB_REWORDED),
    ("same", "hose repair: note vs task page", HOSE_NOTE, HOSE_TASK),
    ("same", "zh winter care, reworded", WINTER_ZH, WINTER_ZH_2),
    ("related", "pump calibration vs pump maintenance", CALIB, MAINT),
    ("related", "hose repair vs pre-start checks", HOSE_NOTE, PRESTART),
    ("related", "pre-start vs winter care (zh)", PRESTART, WINTER_ZH),
    ("unrelated", "calibration vs travel", CALIB, TRAVEL),
    ("unrelated", "hose repair vs VPN", HOSE_NOTE, VPN),
    ("unrelated", "pre-start vs VPN", PRESTART, VPN),
]

BLOCK_PAIRS = [
    ("ignore", "identical", "Torque the pump mounting bolts to 210 Nm after every stator change.",
     "Torque the pump mounting bolts to 210 Nm after every stator change."),
    ("ignore", "tiny edit", "Torque the pump mounting bolts to 210 Nm after every stator change.",
     "Torque the pump mounting bolts to 210 Nm after each stator change."),
    ("ignore", "same list item", "- Replaced the swivel hose (part HX-2210).", "- Replaced the swivel hose, part HX-2210."),
    ("replace", "same fact, new number", "- Pressure tested at 12 bar for 5 minutes: no leak.", "- Pressure tested at 12 bar for 10 min, no leak."),
    ("replace", "reworded paragraph", "The hose rubbed on the boom clamp when the auger was fully slewed to the left.",
     "With the auger slewed fully left, the hose rubs on the boom clamp."),
    ("replace", "updated step", "3. Weigh the tank; the weight must be within 2% of the meter reading.",
     "3. Weigh the tank. The difference to the meter reading must be under 1.5% (new limit from 2027)."),
    ("add", "same page, other fact", "- Moved the boom clamp 40 mm towards the cab and added a protective sleeve.",
     "MMU-03 and MMU-05 have the same clamp position: check them at their next PM."),
    ("add", "same subject, other step", "1. Flush the line with water until no emulsion comes out.",
     "4. Otherwise enter the new K-factor in the MMU controller under Settings > Flow."),
    ("add", "unrelated", "Torque the pump mounting bolts to 210 Nm after every stator change.",
     "Flights must be booked two weeks ahead."),
]


class Embedder:
    def __init__(self):
        self.p = subprocess.Popen([sys.executable, "-I", "-m", "cairn_sidecar", "--role", "embed"], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        self.n = 0

    def documents(self, items):
        self.n += 1
        req = {"jsonrpc": "2.0", "id": self.n, "method": "embed.documents",
               "params": {"model_dir": MODEL_DIR, "dims": 256, "threads": 4,
                          "items": [{"title": t, "text": x} for t, x in items]}}
        self.p.stdin.write(json.dumps(req) + "\n")
        self.p.stdin.flush()
        for line in self.p.stdout:
            m = json.loads(line)
            if m.get("id") == self.n:
                if "error" in m:
                    raise RuntimeError(m["error"])
                return [np.frombuffer(base64.b64decode(v), dtype="<f4") for v in m["result"]["vectors"]]

    def close(self):
        self.p.stdin.close()
        self.p.wait(30)


def pieces(page):
    """(title, heading path + text) per `## ` section, as chunking::split_page does for short pages."""
    title = re.match(r"#\s+(.+)", page).group(1)
    out, heading, buf = [], "", []
    for line in page.splitlines()[1:]:
        if line.startswith("## "):
            if "".join(buf).strip():
                out.append((title, (heading + "\n" if heading else "") + "\n".join(buf).strip()))
            heading, buf = line[3:].strip(), []
        else:
            buf.append(line)
    if "".join(buf).strip():
        out.append((title, (heading + "\n" if heading else "") + "\n".join(buf).strip()))
    return out or [(title, "")]


def main():
    e = Embedder()
    try:
        print("Page vs page (best piece pair; FR-KN-DU-1):")
        for cls, label, a, b in PAGE_PAIRS:
            va, vb = e.documents(pieces(a)), e.documents(pieces(b))
            best = max(float(x @ y) for x in va for y in vb)
            print(f"  {cls:<9} {best:.3f}  {label}")
        print("\nBlock vs block (merge screen; FR-KN-DU-3):")
        for cls, label, a, b in BLOCK_PAIRS:
            va, vb = e.documents([("", a), ("", b)])
            print(f"  {cls:<8} {float(va @ vb):.3f}  {label}")
    finally:
        e.close()


if __name__ == "__main__":
    main()
