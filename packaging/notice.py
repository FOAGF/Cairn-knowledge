"""Writes Cairn's NOTICE file (D-46, NFR-INS-1). Standard library only; run it as

    python3 -I packaging/notice.py [--windows] [--lite] [--out FILE] [--python-root DIR] [--no-rust] [--no-npm]

It works offline. The file holds, in this order:
  1. the Gemma line and the Prohibited Use Policy URL, then the full Gemma Terms of Use
     (packaging/licenses/GEMMA_TERMS_OF_USE.txt); not in a lite NOTICE;
  2. CPython's licence (from the Python in the bundle);
  3. the Python packages of the sidecar bundle (the packages named in sidecar/requirements.lock,
     with the licence files of their dist-info folders);
  4. the npm packages of the production UI: `dependencies` of app/package.json and what they need
     (not the dev dependencies), read from app/node_modules, minus the tools in NPM_BUILD_ONLY;
  5. the Rust crates of the build: `cargo metadata --offline`, normal dependencies of the app for
     Linux, with the licence files from ~/.cargo/registry/src.
Licence texts that are word for word the same are printed once, with the list of packages that
use them.

--python-root: the folder that holds `lib/python3.12` (default: build/stage/sidecar/python).
--windows (M8): the Windows bundle. Python root build/stage/windows/sidecar/python (CPython's
licence is python/LICENSE.txt, the packages are in Lib/site-packages), output
build/stage/windows/NOTICE, and the Rust crates for x86_64-pc-windows-msvc.
--lite: the NOTICE of a lite package, which has no search model (NFR-INS-1b, D-82): no Gemma section,
output NOTICE-lite in the same folder.
For a test against a venv, give --python-site and --cpython-license separately.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
GEMMA_TERMS = REPO / "packaging" / "licenses" / "GEMMA_TERMS_OF_USE.txt"
GEMMA_LINE = ("Gemma is provided under and subject to the Gemma Terms of Use found at "
              "ai.google.dev/gemma/terms")
GEMMA_POLICY = ("The Gemma Prohibited Use Policy is at "
                "https://ai.google.dev/gemma/prohibited_use_policy")

# Tools that are listed under `dependencies` in app/package.json but are not part of the built UI
# (the shadcn command line tool and the Tailwind build plugin).
NPM_BUILD_ONLY = {"shadcn", "@tailwindcss/vite"}

LICENSE_FILE = re.compile(r"^(licen[sc]e|copying|unlicen[sc]e|notice|copyright)([-_.].*)?$", re.I)
RULE = "=" * 78


def read_text(path):
    return Path(path).read_text(encoding="utf-8", errors="replace").strip("\n")


def licence_files(folder, depth=0):
    """Licence-like files directly in `folder` (and in a `licenses` subfolder)."""
    found = []
    if not folder.is_dir():
        return found
    for entry in sorted(folder.iterdir()):
        if entry.is_file() and LICENSE_FILE.match(entry.name) and entry.stat().st_size < 200_000:
            found.append(entry)
        elif entry.is_dir() and entry.name.lower() in ("licenses", "license", "licences") and depth == 0:
            found += licence_files(entry, depth + 1)
    return found


class Group:
    """Packages that share one licence text."""

    def __init__(self):
        self.texts = {}   # sha -> (text, [package labels])
        self.missing = []  # (label, licence expression, where to look)

    def add(self, label, expression, files, where=""):
        texts = [read_text(f) for f in files]
        texts = [t for t in texts if t]
        if not texts:
            self.missing.append((label, expression, where))
            return
        for t in texts:
            key = hashlib.sha256(t.encode()).hexdigest()
            self.texts.setdefault(key, (t, []))[1].append(f"{label} [{expression or 'licence not stated'}]")

    def render(self, title):
        out = [RULE, title, RULE, ""]
        for text, labels in sorted(self.texts.values(), key=lambda v: v[1][0].lower()):
            labels = list(dict.fromkeys(labels))
            out.append("The following packages come with this licence text:")
            out += [f"  - {label}" for label in labels]
            out += ["", text, "", "-" * 78, ""]
        if self.missing:
            out.append("These packages state their licence, but their folder holds no licence file:")
            for label, expression, where in sorted(self.missing, key=lambda m: m[0].lower()):
                out.append(f"  - {label}: {expression or 'not stated'}" + (f" ({where})" if where else ""))
            out.append("")
        return "\n".join(out)


# ---------------------------------------------------------------- Python
def norm(name):
    return re.sub(r"[-_.]+", "-", name).lower()


def metadata_field(text, key):
    m = re.search(rf"^{key}: *(.*)$", text, re.M | re.I)
    return m.group(1).strip() if m else ""


def python_section(site, locked):
    group = Group()
    count = 0
    for dist in sorted(site.glob("*.dist-info")):
        name, version = dist.name[:-len(".dist-info")].rsplit("-", 1)
        if locked is not None and norm(name) not in locked:
            continue
        meta = (dist / "METADATA").read_text(encoding="utf-8", errors="replace") if (dist / "METADATA").exists() else ""
        expression = metadata_field(meta, "License-Expression") or metadata_field(meta, "License")
        if len(expression) > 80 or "\n" in expression:
            expression = ""   # a pasted licence text, not a name
        if not expression:
            classifiers = re.findall(r"^Classifier: License :: (.*)$", meta, re.M)
            expression = "; ".join(c.split(" :: ")[-1] for c in classifiers)
        files = licence_files(dist)
        # a few wheels keep their licence beside the code
        for pkg in (site / name, site / name.replace("-", "_"), site / name.lower().replace("-", "_")):
            if not files and pkg.is_dir():
                files = licence_files(pkg)
        group.add(f"{name} {version}", expression, files, where=str(dist.name))
        count += 1
    return group, count


# ---------------------------------------------------------------- npm
def npm_closure(app):
    pkg = json.loads((app / "package.json").read_text())
    todo = [(d, app) for d in pkg.get("dependencies", {}) if d not in NPM_BUILD_ONLY]
    seen = {}
    missing = []
    while todo:
        name, parent = todo.pop()
        # Node looks in the requiring package's own node_modules, then in each folder above it.
        folder = None
        probe = parent
        while folder is None:
            if (probe / "node_modules" / name / "package.json").exists():
                folder = (probe / "node_modules" / name).resolve()
            elif probe == app or probe.parent == probe:
                break
            else:
                probe = probe.parent
        if folder is None:
            missing.append(name)
            continue
        if folder in seen:
            continue
        info = json.loads((folder / "package.json").read_text())
        seen[folder] = info
        for dep in info.get("dependencies", {}):
            todo.append((dep, folder))
    return seen, sorted(set(missing))


def npm_section(app):
    seen, missing = npm_closure(app)
    group = Group()
    for folder, info in seen.items():
        lic = info.get("license") or info.get("licenses") or ""
        if isinstance(lic, dict):
            lic = lic.get("type", "")
        if isinstance(lic, list):
            lic = " OR ".join(x.get("type", "") if isinstance(x, dict) else str(x) for x in lic)
        group.add(f"{info.get('name', folder.name)} {info.get('version', '')}".strip(), str(lic), licence_files(folder),
                  where=info.get("homepage") or "")
    return group, len(seen), missing


# ---------------------------------------------------------------- Rust
def rust_section(target="x86_64-unknown-linux-gnu"):
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    raw = subprocess.run([cargo, "metadata", "--offline", "--format-version", "1",
                          "--filter-platform", target],
                         cwd=REPO, check=True, capture_output=True, text=True).stdout
    meta = json.loads(raw)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    # normal dependencies (not build scripts, not tests) of the app, for the target
    todo = list(members)
    keep = set()
    while todo:
        node = nodes[todo.pop()]
        for dep in node["deps"]:
            if not any(k.get("kind") is None for k in dep["dep_kinds"]):
                continue
            if dep["pkg"] not in keep and dep["pkg"] not in members:
                keep.add(dep["pkg"])
                todo.append(dep["pkg"])
    group = Group()
    for pid in sorted(keep, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        p = packages[pid]
        folder = Path(p["manifest_path"]).parent
        files = licence_files(folder)
        if p.get("license_file"):
            lf = folder / p["license_file"]
            if lf.is_file() and lf not in files:
                files.append(lf)
        group.add(f"{p['name']} {p['version']}", p.get("license") or "", files, where=p.get("repository") or "")
    return group, len(keep)


# ---------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--windows", action="store_true", help="the Windows bundle (see the top of this file)")
    ap.add_argument("--lite", action="store_true", help="a lite package: no search model, no Gemma section")
    ap.add_argument("--out")
    ap.add_argument("--python-root")
    ap.add_argument("--python-site", help="site-packages folder (default: inside --python-root)")
    ap.add_argument("--cpython-license", help="CPython LICENSE.txt (default: inside --python-root)")
    ap.add_argument("--lock", default=str(REPO / "sidecar" / "requirements.lock"))
    ap.add_argument("--no-rust", action="store_true", help="skip the Rust section (test hook)")
    ap.add_argument("--no-npm", action="store_true", help="skip the npm section (test hook)")
    args = ap.parse_args()

    build = REPO / "build" / "stage" / ("windows" if args.windows else "")
    root = Path(args.python_root or build / "sidecar" / "python")
    out_default = build / ("NOTICE-lite" if args.lite else "NOTICE")
    if args.windows:
        site_default = root / "Lib" / "site-packages"
        licence_default = root / "LICENSE.txt"
        rust_target = "x86_64-pc-windows-msvc"
        python_title = "PYTHON (CPython, in the python-build-standalone build for Windows)"
    else:
        site_default = root / "lib" / "python3.12" / "site-packages"
        licence_default = root / "lib" / "python3.12" / "LICENSE.txt"
        rust_target = "x86_64-unknown-linux-gnu"
        python_title = "PYTHON (CPython, in the python-build-standalone build)"
    site = Path(args.python_site) if args.python_site else site_default
    cpython_license = Path(args.cpython_license) if args.cpython_license else licence_default
    locked = set()
    for line in Path(args.lock).read_text().splitlines():
        line = line.split("#")[0].strip()
        if "==" in line:
            locked.add(norm(line.split("==")[0]))

    if args.lite:
        parts = [
            "This file lists the notices for the components Cairn ships with. They are under their own licences.",
            "This lite package leaves out the search model (EmbeddingGemma) and its terms. Cairn shows the Gemma",
            "Terms of Use before it downloads the model.",
            "",
        ]
    else:
        parts = [
            GEMMA_LINE,
            GEMMA_POLICY,
            "",
            "This file lists the notices for the components Cairn ships with.",
            "Cairn uses the EmbeddingGemma model (embeddinggemma-300m) for search by meaning. Its terms follow.",
            "The packages after them are under their own licences.",
            "",
            RULE,
            "GEMMA TERMS OF USE",
            RULE,
            "",
            read_text(GEMMA_TERMS),
            "",
        ]

    if not cpython_license.is_file():
        sys.exit(f"CPython licence not found: {cpython_license} (build the sidecar bundle first)")
    parts += [RULE, python_title, RULE, "",
              "Cairn's AI worker runs on CPython from the python-build-standalone project",
              "(https://github.com/astral-sh/python-build-standalone). The interpreter also contains",
              "libraries that the build links in (for example OpenSSL, SQLite, zlib, libffi, expat);",
              "their licence texts are listed by the python-build-standalone release the bundle came from.",
              "", read_text(cpython_license), ""]

    py_group, py_count = python_section(site, locked)
    parts.append(py_group.render(f"PYTHON PACKAGES OF THE AI WORKER ({py_count})"))
    summary = [f"python packages: {py_count}"]

    if not args.no_npm:
        npm_group, npm_count, npm_missing = npm_section(REPO / "app")
        parts.append(npm_group.render(f"UI PACKAGES (npm, {npm_count})"))
        summary.append(f"npm packages: {npm_count}")
        if npm_missing:
            print("npm packages not found in node_modules: " + ", ".join(npm_missing), file=sys.stderr)
    if not args.no_rust:
        rust_group, rust_count = rust_section(rust_target)
        parts.append(rust_group.render(f"RUST CRATES ({rust_count})"))
        summary.append(f"rust crates: {rust_count}")

    out = Path(args.out) if args.out else out_default
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(parts).rstrip() + "\n", encoding="utf-8")
    print(f"[notice] wrote {out} ({out.stat().st_size // 1024} KiB; " + ", ".join(summary) + ")")


if __name__ == "__main__":
    main()
