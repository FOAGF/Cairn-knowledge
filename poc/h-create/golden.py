#!/usr/bin/env python
"""Writes crates/cairn-core/prompts/testdata/golden.json: every template rendered by prompts.py with fixed values,
so the Rust renderer's test (`prompts::tests::golden_matches_the_python_renderer`) checks both render alike.
Run it after changing a template:

    sidecar/.venv/bin/python poc/h-create/golden.py
"""
import glob, json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import prompts as P  # noqa: E402

out = {}
for path in sorted(glob.glob(os.path.join(P.PROMPTS, "[!_]*.md"))):
    tid = os.path.basename(path)[:-3]
    if tid == "README":
        continue
    names = P.load(tid)["meta"]["variables"]
    out[tid] = P.render(tid, **{v: f"<{v}: {{{{x}}}} value>" for v in names})
dest = os.path.join(P.PROMPTS, "testdata", "golden.json")
with open(dest, "w", encoding="utf-8") as f:
    json.dump(out, f, ensure_ascii=False, indent=1)
    f.write("\n")
print(f"{len(out)} templates -> {dest}")
