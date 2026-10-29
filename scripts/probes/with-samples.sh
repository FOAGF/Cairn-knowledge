#!/bin/bash
# Writes a copy of a probe with the M5 sample files (poc/h-create/samples) prepended as
# `window.M5_SAMPLES = {name: base64}`, and prints its path:
#   scripts/ui-probe.sh "$(scripts/probes/with-samples.sh scripts/probes/m5-flows.js)"
set -eu
here=$(cd "$(dirname "$0")/../.." && pwd)
out=$(mktemp --suffix=.js)
python3 -I - "$here/poc/h-create/samples" "$1" "$out" <<'PY'
import base64, json, os, sys
samples, probe, out = sys.argv[1:]
files = {n: base64.b64encode(open(os.path.join(samples, n), "rb").read()).decode()
         for n in sorted(os.listdir(samples)) if not n.endswith((".json", ".truth.txt"))}
with open(out, "w") as f:
    f.write("window.M5_SAMPLES = " + json.dumps(files) + ";\n")
    f.write(open(probe).read())
PY
echo "$out"
