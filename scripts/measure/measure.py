"""Measures Cairn against NFR-PERF-1 to 5 (M7, NFR-PERF-6, D-51) and appends a dated section to
docs/measurements.md. Start it with `make measure` (scripts/measure/measure.sh), which builds the
data generator and picks the AppImage. Standard library only; run with `python3 -I`.

What it does, in order:
  1. makes full-scale data in a throw-away folder (crates/cairn-core/examples/measure_data.rs);
  2. starts the mock LLM (scripts/mock_llm.py) on its own port;
  3. starts the app four times on that data, once per thing to measure (scripts/measure/measure.js
     runs inside the window and reports through the `automation_report` command):
       idle       start-up, memory at idle, CPU over 60 s idle, keyword search times
       meaning    memory after a search by meaning, hybrid search times
       editing    memory while editing a page (spelling and grammar checks running)
       assistant  memory after one Assistant answer
     A fresh start per state keeps each state's memory its own. Memory is PSS summed over the
     app's whole process tree (shell, WebKit processes, the AI workers), read from
     /proc/<pid>/smaps_rollup every 0.5 s.
  4. stops every process it started (also on failure or Ctrl+C) and removes the data folder.
Going over a target is reported, never a failure.
"""
import argparse
import atexit
import datetime
import json
import os
import platform
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
CLK = os.sysconf("SC_CLK_TCK")

# Targets from docs/requirements.md (guidelines, not limits).
TARGET_IDLE_MB = 400
TARGET_PEAK_MB = 1500
TARGET_STARTUP_S = 3.0
TARGET_KEYWORD_MS = 300
TARGET_HYBRID_MS = 1000
IDLE_CPU_OK_PERCENT = 1.0  # NFR-PERF-5 says "no CPU use"; under 1 % of one core counts as none.

# Fixed query sets (the words come from WORDS in measure_data.rs; a task key needs a large data set).
KEYWORD_QUERIES = [
    "pipeline", "pipeline data", "sens", "maintenance schedule", "数据", "管道", "泵",
    "pressure valve safety", "w17", "zzzz", "PMP-123", "handover shift checklist",
]
HYBRID_QUERIES = [
    "how do we calibrate the pump", "safety isolation procedure before service",
    "supplier invoice questions", "what was decided in the budget meeting",
    "firmware upgrade steps", "泵的校准和维护", "weekly handover checklist",
    "battery training schedule", "drainage inspection results", "backup and restore",
]
ASSISTANT_QUESTION = "How do we calibrate the pump?"

# ---------------------------------------------------------------- process tree

# Every process this script started, as (pid, start time): the clean-up kills these and nothing else.
REGISTRY = {}
# The Popen objects of those, so a stopped child is collected and not left as a zombie.
POPENS = []


def read_stat(pid):
    """(ppid, start time in ticks, utime + stime in ticks) or None."""
    try:
        with open(f"/proc/{pid}/stat") as f:
            data = f.read()
    except OSError:
        return None
    rest = data[data.rindex(")") + 2:].split()
    return int(rest[1]), int(rest[19]), int(rest[11]) + int(rest[12])


def cmdline(pid):
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as f:
            return f.read().replace(b"\0", b" ").decode("utf-8", "replace").strip()
    except OSError:
        return ""


def descendants(root):
    """root and every process below it: {pid: (start, ticks)}."""
    children = {}
    stats = {}
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        st = read_stat(int(name))
        if st:
            stats[int(name)] = st
            children.setdefault(st[0], []).append(int(name))
    out = {}
    todo = [root]
    while todo:
        pid = todo.pop()
        if pid in stats and pid not in out:
            out[pid] = (stats[pid][1], stats[pid][2])
            todo.extend(children.get(pid, []))
    return out


def role(pid):
    c = cmdline(pid)
    if "cairn_sidecar" in c:
        return "ai worker" if "--role ai" in c else "search worker" if "--role embed" in c else "worker"
    if "WebKit" in c:
        return "webkit"
    return "app"


def pss_kb(pid):
    try:
        with open(f"/proc/{pid}/smaps_rollup") as f:
            for line in f:
                if line.startswith("Pss:"):
                    return int(line.split()[1])
    except OSError:
        pass
    return None


def sample(root):
    """One reading of the tree: total PSS (kB), PSS by role, CPU ticks by pid."""
    tree = descendants(root)
    by_role = {}
    total = 0
    for pid, (start, _ticks) in tree.items():
        REGISTRY[pid] = start
        kb = pss_kb(pid)
        if kb is None:
            continue
        r = role(pid)
        by_role[r] = by_role.get(r, 0) + kb
        total += kb
    return {"t": time.time(), "total": total, "roles": by_role, "n": len(tree),
            "ticks": {pid: v[1] for pid, v in tree.items()},
            "roleOf": {pid: role(pid) for pid in tree}}


def stop_everything(skip=()):
    """Stops what this script started (except the pids in `skip`): SIGTERM, a wait, then SIGKILL for
    what's left. A pid counts only while its start time still matches, so a reused pid is never
    touched."""
    def alive():
        for p in POPENS:
            p.poll()
        out = []
        for pid, start in list(REGISTRY.items()):
            if pid in skip:
                continue
            st = read_stat(pid)
            if st and st[1] == start:
                out.append(pid)
            else:
                REGISTRY.pop(pid, None)
        return out

    for sig, wait in ((signal.SIGTERM, 8.0), (signal.SIGKILL, 3.0)):
        pids = alive()
        if not pids:
            return
        for pid in pids:
            try:
                os.kill(pid, sig)
            except OSError:
                pass
        end = time.time() + wait
        while time.time() < end and alive():
            time.sleep(0.2)


# ------------------------------------------------------------------- helpers


def free_port_or_die(port):
    s = socket.socket()
    s.settimeout(0.5)
    try:
        s.connect(("127.0.0.1", port))
    except OSError:
        return
    finally:
        s.close()
    sys.exit(f"measure: port {port} is already in use (by something this script must not stop). Pass --mock-port.")


def wait_port(port, seconds=15):
    end = time.time() + seconds
    while time.time() < end:
        s = socket.socket()
        s.settimeout(0.3)
        try:
            s.connect(("127.0.0.1", port))
            return True
        except OSError:
            time.sleep(0.2)
        finally:
            s.close()
    return False


def app_env(data_dir, out_file, mock_port, mode, deadline, script_path):
    keep = ("PATH", "HOME", "USER", "LANG", "LC_ALL", "DISPLAY", "XAUTHORITY", "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR", "XDG_SESSION_TYPE", "DBUS_SESSION_BUS_ADDRESS", "XDG_DATA_DIRS",
            "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "TMPDIR")
    env = {k: os.environ[k] for k in keep if k in os.environ}
    # Debug builds find the AI worker through these (docs/development.md); a release build ignores them.
    for k in ("CAIRN_SIDECAR_PYTHON", "CAIRN_EMBED_MODEL_DIR"):
        if k in os.environ:
            env[k] = os.environ[k]
    env.update({
        "CAIRN_DATA_DIR": data_dir,
        "CAIRN_AUTOMATION": "1",
        "CAIRN_AUTOMATION_OUT": out_file,
        "CAIRN_DEV_SCRIPT_FILE": script_path,
        "CAIRN_DEV_SCRIPT_AFTER": "load",
        "CAIRN_EXIT_AFTER_SECS": str(int(deadline) + 30),  # only a safety net
    })
    return env


def median(xs):
    return statistics.median(xs) if xs else None


# ---------------------------------------------------------------- one app run


def run_app(target, mode, cfg, data_dir, work, mock_port, log):
    """Starts the app for one mode and returns what it measured."""
    cfg = dict(cfg, mode=mode)
    deadline = {"idle": 120 + cfg["cpuSecs"] + 90, "meaning": 300, "editing": 200, "assistant": 260}[mode]
    out_file = os.path.join(work, f"report-{mode}.txt")
    open(out_file, "w").close()
    script = os.path.join(work, f"script-{mode}.js")
    with open(os.path.join(HERE, "measure.js")) as f, open(script, "w") as g:
        g.write("const CFG = " + json.dumps(cfg) + "\n" + f.read())
    env = app_env(data_dir, out_file, mock_port, mode, deadline, script)
    log(f"  {mode}: starting {os.path.basename(target)}")
    spawned = time.time()
    before = set(REGISTRY)  # the mock LLM is in there and keeps running between starts
    app_log = open(os.path.join(work, f"app-{mode}.log"), "wb")
    proc = subprocess.Popen([target], env=env, stdin=subprocess.DEVNULL, stdout=app_log,
                            stderr=subprocess.STDOUT, start_new_session=True, cwd=work)
    POPENS.append(proc)
    REGISTRY[proc.pid] = read_stat(proc.pid)[1] if read_stat(proc.pid) else 0
    res = {"mode": mode, "states": {}, "info": [], "errors": []}
    samples = []
    marks = {}
    offset = 0
    done = False
    try:
        while time.time() - spawned < deadline:
            time.sleep(0.5)
            s = sample(proc.pid)
            samples.append(s)
            with open(out_file, "rb") as f:
                f.seek(offset)
                chunk = f.read()
            # Only whole lines; the rest waits for the next round.
            whole = chunk[: chunk.rfind(b"\n") + 1]
            offset += len(whole)
            for line in whole.decode("utf-8", "replace").splitlines():
                head, _, rest = line.partition(" ")
                if head == "STARTUP":
                    kv = dict(p.split("=") for p in rest.split())
                    res["startup"] = int(kv["usableAt"]) / 1000.0 - spawned
                    res["pageLoad"] = int(kv["usableAt"]) / 1000.0 - int(kv["timeOrigin"]) / 1000.0
                    log(f"  {mode}: usable window after {res['startup']:.2f} s")
                elif head == "STATE":
                    marks[rest] = [time.time(), None]
                elif head == "STATE_END":
                    marks[rest][1] = time.time()
                    log(f"  {mode}: sampled state {rest}")
                elif head == "TIMES":
                    kind, _, js = rest.partition(" ")
                    res[kind] = json.loads(js)
                    log(f"  {mode}: {kind} search median {res[kind]['median']:.0f} ms, p95 {res[kind]['p95']:.0f} ms")
                elif head == "INFO":
                    res["info"].append(rest)
                elif head == "ERROR":
                    res["errors"].append(rest)
                    log(f"  {mode}: {line}")
                elif head == "DONE":
                    done = True
            if done:
                break
            if proc.poll() is not None and not descendants(proc.pid).get(proc.pid):
                res["errors"].append("the app exited before the run was done")
                break
        else:
            res["errors"].append(f"no DONE after {deadline:.0f} s")
    finally:
        samples_final = samples
        stop_everything(skip=before)
        app_log.close()
    # Per state: median PSS over the hold (the first second is skipped), and CPU over it.
    for name, (t0, t1) in marks.items():
        if t1 is None:
            continue
        win = [s for s in samples_final if t0 + 1.0 <= s["t"] <= t1]
        if not win:
            continue
        med = median([s["total"] for s in win])
        roles = {}
        for r in {r for s in win for r in s["roles"]}:
            roles[r] = median([s["roles"].get(r, 0) for s in win])
        entry = {"pssKb": med, "maxKb": max(s["total"] for s in win), "roles": roles,
                 "processes": median([s["n"] for s in win]), "seconds": win[-1]["t"] - win[0]["t"]}
        common = set(win[0]["ticks"]) & set(win[-1]["ticks"])
        ticks = sum(win[-1]["ticks"][p] - win[0]["ticks"][p] for p in common)
        entry["cpuSeconds"] = ticks / CLK
        entry["cpuRoles"] = {}
        for p in common:
            r = win[-1]["roleOf"].get(p, "app")
            entry["cpuRoles"][r] = entry["cpuRoles"].get(r, 0) + (win[-1]["ticks"][p] - win[0]["ticks"][p]) / CLK
        res["states"][name] = entry
    res["peakKb"] = max((s["total"] for s in samples_final), default=0)
    res["samples"] = len(samples_final)
    return res


# -------------------------------------------------------------------- report


def machine_info():
    cpu = "unknown"
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    mem = "unknown"
    try:
        with open("/proc/meminfo") as f:
            mem = f"{int(f.readline().split()[1]) / 1048576:.1f} GB"
    except OSError:
        pass
    distro = platform.system()
    try:
        with open("/etc/os-release") as f:
            for line in f:
                if line.startswith("PRETTY_NAME="):
                    distro = line.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    return f"{cpu}, {os.cpu_count()} logical cores, {mem} RAM, Linux {platform.release()}, {distro}"


def mb(kb):
    return kb / 1024.0


def verdict(ok):
    return "within" if ok else "**over**"


def build_section(a, results, data, size_mb, version):
    by = {r["mode"]: r for r in results}
    startups = [r["startup"] for r in results if "startup" in r]
    idle = by.get("idle", {}).get("states", {}).get("idle")
    cpu = by.get("idle", {}).get("states", {}).get("cpu")
    rows = []

    def row(what, result, target="", status=""):
        rows.append(f"| {what} | {result} | {target} | {status} |")

    def roles(entry):
        return ", ".join(f"{r} {mb(v):.0f}" for r, v in sorted(entry["roles"].items(), key=lambda x: -x[1]))

    if startups:
        m = median(startups)
        row("Start-up to a usable window", f"{m:.2f} s (median of {len(startups)} starts: "
            + ", ".join(f"{s:.2f}" for s in startups) + ")", f"under {TARGET_STARTUP_S:g} s (NFR-PERF-2)", verdict(m <= TARGET_STARTUP_S))
    mem_states = [("idle", "idle", "Memory, idle (no models loaded)", f"about {TARGET_IDLE_MB} MB (NFR-PERF-1)", TARGET_IDLE_MB),
                  ("meaning", "meaning", "Memory, after a search by meaning (search model loaded)", f"peak about {TARGET_PEAK_MB} MB", TARGET_PEAK_MB),
                  ("editing", "editing", "Memory, editing a page (spelling and grammar checks)", f"peak about {TARGET_PEAK_MB} MB", TARGET_PEAK_MB),
                  ("assistant", "assistant", "Memory, after an Assistant answer (mock LLM)", f"peak about {TARGET_PEAK_MB} MB", TARGET_PEAK_MB)]
    for mode, state, label, target, limit in mem_states:
        e = by.get(mode, {}).get("states", {}).get(state)
        if e:
            row(label, f"{mb(e['pssKb']):.0f} MB PSS ({roles(e)}; {e['processes']:.0f} processes)", target, verdict(mb(e["pssKb"]) <= limit))
        else:
            row(label, "not measured (" + "; ".join(by.get(mode, {}).get("errors", ["no result"])) + ")", target, "")
    peak = max((r.get("peakKb", 0) for r in results), default=0)
    row("Peak memory over all four runs", f"{mb(peak):.0f} MB PSS", f"about {TARGET_PEAK_MB} MB (NFR-PERF-1)", verdict(mb(peak) <= TARGET_PEAK_MB))
    for kind, label, limit, nfr in (("keyword", "Keyword search", TARGET_KEYWORD_MS, "NFR-PERF-3"), ("hybrid", "Hybrid search (model loaded)", TARGET_HYBRID_MS, "NFR-PERF-3")):
        src = by.get("idle" if kind == "keyword" else "meaning", {}).get(kind)
        if src:
            row(label, f"median {src['median']:.0f} ms, 95th percentile {src['p95']:.0f} ms, max {src['max']:.0f} ms ({src['n']} searches: {src['queries']} queries x {src['rounds']} rounds)",
                f"under {limit} ms (the 95th percentile is compared) ({nfr})", verdict(src["p95"] <= limit))
        else:
            row(label, "not measured", f"under {limit} ms ({nfr})", "")
    if cpu:
        pct = 100.0 * cpu["cpuSeconds"] / max(cpu["seconds"], 1)
        row(f"CPU over {cpu['seconds']:.0f} s idle", f"{pct:.2f} % of one core ({cpu['cpuSeconds']:.2f} CPU seconds in {cpu['seconds']:.0f} s; "
            + ", ".join(f"{r} {v:.2f} s" for r, v in sorted(cpu["cpuRoles"].items(), key=lambda x: -x[1])) + ")",
            f"none beyond indexing; under {IDLE_CPU_OK_PERCENT:g} % counts as none (NFR-PERF-5)", verdict(pct <= IDLE_CPU_OK_PERCENT))
    row("Design scale", f"{data['pages']:,} pages, {data['tasks']:,} tasks, {data['chunks']:,} search pieces",
        "10,000 pages and 20,000 tasks (NFR-PERF-4)", "full scale" if data["pages"] >= 10000 and data["tasks"] >= 20000 else "**reduced scale**")
    row("AppImage size", f"{size_mb:.0f} MB" if size_mb is not None else "n/a (not an AppImage)", "no target", "")

    notes = []
    for r in results:
        for e in r["errors"]:
            notes.append(f"- {r['mode']}: {e}")
        for i in r["info"]:
            notes.append(f"- {r['mode']}: {i}")
    notes.append("- Memory is PSS summed over the app's whole process tree (median of the samples over a "
                 f"{a.hold_ms / 1000:g} s hold). Each state comes from its own fresh start of the app.")
    notes.append("- The Assistant state includes the search worker too: the Assistant looks up sources by meaning before it answers.")
    notes.append("- Start-up runs from launching the file until Home shows the page count and the Today list of this data "
                 "(the page's clock, `Date.now()`). It includes the AppImage's own start.")
    notes.append("- The search model loads in the first hybrid search of the meaning run; the vectors in this data are generated, "
                 "the query vector is real.")
    now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    head = [f"## {now}: Cairn {version}", "",
            f"- **Run:** `{os.path.basename(a.target)}`" + (f", commit {a.commit}" if a.commit else ""),
            f"- **Machine:** {machine_info()}",
            f"- **Data:** generated in {data['seconds']:.0f} s; model `{data['model']}`, {data['dims']} dims; the index was "
            f"{'ready' if data['chunksReady'] == data['chunks'] else 'NOT ready'} at the start.", "",
            "| Measure | Result | Target | |", "|---|---|---|---|"]
    return "\n".join(head + rows + [""] + notes + [""])


INTRO = """# Measurements

What `make measure` found on the owner's machine, one dated section per run, oldest first. The targets come from
[requirements.md](requirements.md) (NFR-PERF-1 to NFR-PERF-5); they are guidelines. Going over one is reported here
and looked into, but it never fails a build (NFR-PERF-6, D-51).

## What it measures

- **Start-up to a usable window:** from launching the file until Home shows the page count and the Today list of the
  full-scale data (NFR-PERF-2: 3 s).
- **Memory:** PSS summed over the whole process tree (the app, the WebKit processes, the AI workers), in four states:
  idle, after a search by meaning (the search model is loaded), while editing a page (spelling and grammar checks are
  loaded), and after an Assistant answer (mock LLM, never a real model). Each state has its own fresh start of the app.
  PSS counts shared pages once, so it is smaller than the sum of RSS (NFR-PERF-1: idle about 400 MB, peak about 1.5 GB).
- **Search times:** keyword search and hybrid search (keyword plus meaning) through the app's own commands, at 10,000
  pages and 20,000 tasks; median and 95th percentile over a fixed set of queries (NFR-PERF-3: 300 ms and 1 s).
- **CPU over 60 s idle** for the whole tree (NFR-PERF-5).
- **The AppImage size.**

The data is made by `crates/cairn-core/examples/measure_data.rs` in a throw-away folder: 10,000 pages and 20,000 tasks
(some Chinese), about 20 categories, groups, tags, comments, a glossary and task-page links. The search vectors are
generated (random unit vectors with the app's model id and dimensions), so no long indexing is needed; the scan's
speed doesn't depend on what the vectors say, and the real model still embeds each query.

## How to run it

`make measure` (or `scripts/measure/measure.sh [AppImage]`). It uses the newest AppImage under
`src-tauri/target/release/bundle/appimage/`, loads the CPU for several minutes and needs a screen (the app opens its
window while it runs). Run it only when the machine is free. Options and details are in `scripts/measure/measure.sh`.

## How to read it

Each section starts with the machine and the build, then one table: the result, the target and whether the result is
within it. Compare sections only from the same machine. A first run after a reboot has colder file caches, so its
start-up is slower than the others.

The results follow, one section per run. Nothing has been run yet when there is none below.

"""

# ---------------------------------------------------------------------- main


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--target", required=True, help="the AppImage, or a plain binary for testing")
    ap.add_argument("--data-bin", required=True, help="the built measure_data example")
    ap.add_argument("--pages", type=int, default=10000)
    ap.add_argument("--tasks", type=int, default=20000)
    ap.add_argument("--mock-port", type=int, default=5299)
    ap.add_argument("--out", default=os.path.join(ROOT, "docs", "measurements.md"), help="'-' prints only")
    ap.add_argument("--work", default=None, help="folder for the throw-away data (default: a new one under $TMPDIR)")
    ap.add_argument("--cpu-secs", type=int, default=60)
    ap.add_argument("--rounds", type=int, default=15, help="measured rounds of every search query")
    ap.add_argument("--hold-ms", type=int, default=6000, help="how long each memory state is held and sampled")
    ap.add_argument("--only", default="idle,meaning,editing,assistant", help="modes to run")
    ap.add_argument("--keep", action="store_true", help="keep the data folder")
    a = ap.parse_args()

    a.target = os.path.abspath(a.target)
    if not os.access(a.target, os.X_OK):
        sys.exit(f"measure: {a.target} is not an executable file")
    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        sys.exit("measure: no screen (DISPLAY or WAYLAND_DISPLAY): the app needs a window")
    free_port_or_die(a.mock_port)
    a.commit = ""
    try:
        a.commit = subprocess.run(["git", "-C", ROOT, "rev-parse", "--short", "HEAD"], capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass

    def log(msg):
        print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)

    atexit.register(stop_everything)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda *_: sys.exit(130))

    work = tempfile.mkdtemp(prefix="cairn-measure-", dir=a.work) if a.work else tempfile.mkdtemp(prefix="cairn-measure-")
    data_dir = os.path.join(work, "data")
    log(f"working folder {work}")
    try:
        log(f"making data: {a.pages:,} pages, {a.tasks:,} tasks")
        gen = subprocess.run([a.data_bin, data_dir, "--pages", str(a.pages), "--tasks", str(a.tasks),
                              "--mock-port", str(a.mock_port)], capture_output=True, text=True)
        if gen.returncode != 0:
            sys.exit("measure: the data generator failed:\n" + gen.stderr[-2000:])
        data = json.loads(gen.stdout.strip().splitlines()[-1])
        log(f"data ready in {data['seconds']:.0f} s: {data['chunks']:,} pieces, vectors for {data['chunksReady']:,}")

        mock_log = os.path.join(work, "mock.log")
        mock = subprocess.Popen(["python3", "-I", os.path.join(ROOT, "scripts", "mock_llm.py"), str(a.mock_port), mock_log],
                                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        POPENS.append(mock)
        REGISTRY[mock.pid] = read_stat(mock.pid)[1]
        if not wait_port(a.mock_port):
            sys.exit("measure: the mock LLM did not start")

        cfg = {"expectPages": data["pages"], "keywordQueries": KEYWORD_QUERIES, "hybridQueries": HYBRID_QUERIES,
               "rounds": a.rounds, "holdMs": a.hold_ms, "cpuSecs": a.cpu_secs, "assistantQuestion": ASSISTANT_QUESTION}
        results = []
        for mode in a.only.split(","):
            results.append(run_app(a.target, mode.strip(), cfg, data_dir, work, a.mock_port, log))
            time.sleep(2)

        version = "(unknown version)"
        for r in results:
            for i in r["info"]:
                if i.startswith("version "):
                    version = i.split()[1]
        if version.startswith("("):
            try:
                with open(os.path.join(ROOT, "Cargo.toml")) as f:
                    for line in f:
                        if line.strip().startswith("version") and "=" in line:
                            version = line.split("=")[1].strip().strip('"')
                            break
            except OSError:
                pass
        size_mb = os.path.getsize(a.target) / 1048576.0 if a.target.lower().endswith(".appimage") else None
        section = build_section(a, results, data, size_mb, version)
        print("\n" + section)
        if a.out != "-":
            os.makedirs(os.path.dirname(a.out), exist_ok=True)
            fresh = not os.path.exists(a.out) or os.path.getsize(a.out) == 0
            with open(a.out, "a") as f:
                f.write((INTRO if fresh else "\n") + section)
            log(f"appended to {a.out}")
    finally:
        stop_everything()
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
