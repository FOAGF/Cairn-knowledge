#!/usr/bin/env bash
# packaging/build-sidecar.sh (NFR-INS-1, D-46): builds the sidecar bundle, i.e. Python with the
# sidecar's packages, trimmed as M0-d found. Output: packaging/build/sidecar/python/
# (the app runs `<resources>/sidecar/python/bin/python3 -I -m cairn_sidecar`).
#
# Steps:
#   1. python-build-standalone (pinned tag, sha256 checked). Taken from packaging/cache/, else
#      copied from the M0 download, else DOWNLOADED (the first online step).
#   2. The wheels from sidecar/requirements.lock: `pip download` into packaging/cache/wheels
#      (the second online step, only when that folder is not complete), LiteLLM's wheel checked
#      against its sha256 (pip-pins.sh), then an offline `pip install --no-index --no-deps`.
#      The sidecar's own package is copied in.
#   3. Pruning (see the PRUNE section) and unchecked-hash .pyc files.
#   4. Checks: no symlinks left, every .pyc unchecked-hash, an import smoke test, LiteLLM's route table
#      (check-litellm-routes.py: the routes Cairn uses run without the Rust extension, D-82) and a `ping`.
#
# Test hook (no network): CAIRN_SITE_PACKAGES_FROM=<site-packages of a venv that has the lock
# installed> copies exactly the lock's packages from there instead of running pip.
# CAIRN_PBS_TARBALL=<file> names a local copy of the Python tarball (checked against the sha256).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"

# ---- pins: bump all of them together, and check the sha256 of the new file by hand
TAG=20261009
PY=3.12.15
FILE="cpython-${PY}+${TAG}-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
SHA256=ffa8f85f1b56e88b687f08d743d817015524b86a9365fd921243132b80f1a36d
URL="https://github.com/astral-sh/python-build-standalone/releases/download/${TAG}/${FILE//+/%2B}"

CACHE="$HERE/cache"
OUT="$HERE/build/sidecar"
LOCK="$REPO/sidecar/requirements.lock"
WHEELS="$CACHE/wheels"
PYLIB="$OUT/python/lib/python3.12"
SP="$PYLIB/site-packages"
PYBIN="$OUT/python/bin/python3"

log() { echo "[sidecar] $*"; }
sha_ok() { [ -f "$1" ] && [ "$(sha256sum "$1" | cut -d' ' -f1)" = "$SHA256" ]; }
# shellcheck source=packaging/pip-pins.sh
. "$HERE/pip-pins.sh"

# ---------------------------------------------------------------- 1. Python
mkdir -p "$CACHE"
TARBALL="$CACHE/$FILE"
if ! sha_ok "$TARBALL"; then
    rm -f "$TARBALL"
    for local_copy in "${CAIRN_PBS_TARBALL:-}"; do
        if [ -n "$local_copy" ] && sha_ok "$local_copy"; then
            log "using the local copy $local_copy"
            cp "$local_copy" "$TARBALL"
            break
        fi
    done
fi
if ! sha_ok "$TARBALL"; then
    log "ONLINE STEP: downloading python-build-standalone ($FILE, about 34 MB) from github.com"
    curl -fL --retry 3 -o "$TARBALL.part" "$URL"
    mv "$TARBALL.part" "$TARBALL"
    sha_ok "$TARBALL" || { log "the download does not match the pinned sha256 ($SHA256); removing it"; rm -f "$TARBALL"; exit 1; }
fi
log "Python tarball OK (sha256 $SHA256)"

rm -rf "$OUT"
mkdir -p "$OUT"
tar -xzf "$TARBALL" -C "$OUT"   # creates $OUT/python/
export PYTHONDONTWRITEBYTECODE=1

# ---------------------------------------------------------------- 2. packages
if [ -n "${CAIRN_SITE_PACKAGES_FROM:-}" ]; then
    log "TEST HOOK: copying the lock's packages from $CAIRN_SITE_PACKAGES_FROM (no pip)"
    python3 -I - "$LOCK" "$CAIRN_SITE_PACKAGES_FROM" "$SP" <<'PY'
import os, re, shutil, sys
lock, src, dst = sys.argv[1:4]
norm = lambda n: re.sub(r"[-_.]+", "-", n).lower()
wanted = {}
for line in open(lock):
    line = line.split("#")[0].strip()
    if "==" in line:
        name, ver = line.split("==")
        wanted[norm(name)] = ver.strip()
found = set()
for entry in sorted(os.listdir(src)):
    if not entry.endswith(".dist-info"):
        continue
    name, ver = entry[:-len(".dist-info")].rsplit("-", 1)
    if wanted.get(norm(name)) != ver:
        continue
    found.add(norm(name))
    for row in open(os.path.join(src, entry, "RECORD")):
        rel = row.rsplit(",", 2)[0].strip('"')
        if rel.startswith("..") or "__pycache__" in rel or rel.endswith(".pyc") or not rel:
            continue
        s = os.path.join(src, rel)
        if not os.path.isfile(s):
            continue
        d = os.path.join(dst, rel)
        os.makedirs(os.path.dirname(d), exist_ok=True)
        shutil.copy2(s, d)
missing = sorted(set(wanted) - found)
if missing:
    sys.exit("not installed in the source environment: " + ", ".join(missing))
print(f"copied {len(found)} packages")
PY
else
    stamp="$WHEELS/.complete-$(sha256sum "$LOCK" | cut -c1-16)"
    if [ ! -f "$stamp" ]; then
        log "ONLINE STEP: downloading the wheels listed in sidecar/requirements.lock (a few hundred MB,"
        log "from pypi.org and files.pythonhosted.org) into $WHEELS"
        mkdir -p "$WHEELS"
        "$PYBIN" -I -m pip download --disable-pip-version-check --only-binary=:all: --no-deps \
            -r "$LOCK" -d "$WHEELS"
        touch "$stamp"
    fi
    litellm_wheel_ok "$LOCK" "$WHEELS" "$LITELLM_WHEEL_LINUX" "$LITELLM_SHA256_LINUX" || exit 1
    log "LiteLLM wheel OK (sha256 $LITELLM_SHA256_LINUX)"
    log "installing the wheels offline"
    "$PYBIN" -I -m pip install --disable-pip-version-check --no-index --no-cache-dir --no-compile \
        --no-deps --find-links "$WHEELS" -r "$LOCK"
fi
# The sidecar's own package: plain Python files (pip would need a build backend from the network).
mkdir -p "$SP/cairn_sidecar"
cp "$REPO"/sidecar/cairn_sidecar/*.py "$SP/cairn_sidecar/"
log "installed: $(du -sh "$SP" | cut -f1) in site-packages"

# ---------------------------------------------------------------- 3. PRUNE
# Only what the sidecar provably never imports (the smoke test below and the self-test of the
# AppImage catch a mistake). Sizes are from the M0 / M7 build.
# pip and friends are removed last-but-one, after the install above.
rm -rf "$SP"/pip "$SP"/pip-*.dist-info "$SP"/setuptools* "$SP"/pkg_resources "$SP"/wheel* "$SP"/_distutils_hack
# Tests of the packages (numpy, litellm, ... ship their test folders)
find "$SP" -type d \( -name tests -o -name test \) -prune -exec rm -rf {} +
# Unused parts of big packages
rm -rf "$SP/litellm/proxy/_experimental/out"                              # the proxy's web UI (static files), 23 MB
rm -rf "$SP"/onnxruntime/{quantization,tools,transformers,datasets}       # model tools, not inference
rm -rf "$SP"/hf_xet "$SP"/hf_xet-*.dist-info                              # faster downloads, we never download
rm -rf "$SP"/boto3 "$SP"/boto3-*.dist-info "$SP"/botocore "$SP"/botocore-*.dist-info \
       "$SP"/s3transfer "$SP"/s3transfer-*.dist-info "$SP"/jmespath "$SP"/jmespath-*.dist-info  # AWS (Bedrock), not offered
# LiteLLM's Rust extension, 41 MB: none of Cairn's routes use it, the app runs the sidecar with
# LITELLM_RUST=false, and step 4 checks LiteLLM's route table (FR-LLM-LIB-2, D-82)
rm -f "$SP"/litellm/rust_bridge/_native*
# Python itself
cd "$OUT/python"
rm -rf share include lib/pkgconfig \
       lib/tcl* lib/tk* lib/itcl* lib/thread* lib/libtcl* \
       "$PYLIB"/test "$PYLIB"/idlelib "$PYLIB"/tkinter "$PYLIB"/turtledemo "$PYLIB"/turtle.py \
       "$PYLIB"/ensurepip "$PYLIB"/lib2to3 "$PYLIB"/pydoc_data "$PYLIB"/config-3.12-x86_64-linux-gnu \
       "$PYLIB"/lib-dynload/_tkinter*
# The interpreter is statically linked: libpython is only for programs that embed Python.
rm -f lib/libpython3.12.so lib/libpython3.12.so.1.0 lib/libpython3.so
# One interpreter file only (Tauri copies symlinks as files, so every link would be a 31 MB copy).
rm -f bin/python bin/python3 bin/idle3* bin/pydoc3* bin/2to3* bin/pip* bin/python3*-config
mv bin/python3.12 bin/python3
cd "$REPO"
# Caches built by the install or the tests are rebuilt below
find "$OUT" -type d -name __pycache__ -prune -exec rm -rf {} +

# .pyc as unchecked-hash: the app's resource copy changes file times, and a time-checked .pyc would
# then be rejected and recompiled in memory at every start (M0-d: +2 s). The AppImage is read-only.
"$PYBIN" -I -m compileall -q -f -j 4 --invalidation-mode unchecked-hash "$OUT/python/lib/python3.12" >/dev/null

# ---------------------------------------------------------------- 4. checks
links="$(find "$OUT" -type l)"
if [ -n "$links" ]; then
    log "symlinks left in the bundle (Tauri would copy each as a file):"; echo "$links"; exit 1
fi
python3 -I - "$OUT/python/lib/python3.12" <<'PY'
import os, sys
bad = total = 0
for root, _, files in os.walk(sys.argv[1]):
    for f in files:
        if f.endswith(".pyc"):
            total += 1
            with open(os.path.join(root, f), "rb") as fh:
                head = fh.read(8)
            if int.from_bytes(head[4:8], "little") != 1:   # 1 = hash-based, not checked
                bad += 1
print(f"[sidecar] {total} .pyc files, {bad} not unchecked-hash")
sys.exit(1 if bad or not total else 0)
PY

log "smoke test: imports"
# LITELLM_RUST=false as the app sets it (FR-LLM-LIB-2, D-82)
env -i HOME=/nonexistent PATH=/usr/bin:/bin PYTHONDONTWRITEBYTECODE=1 LITELLM_LOCAL_MODEL_COST_MAP=True LITELLM_RUST=false \
    "$PYBIN" -I - <<'PY'
import sys
mods = ["numpy", "onnxruntime", "tokenizers", "litellm", "azure.identity", "pypdfium2", "docx", "PIL",
        "httpx", "ssl", "sqlite3", "cairn_sidecar.ai", "cairn_sidecar.embed", "cairn_sidecar.extract",
        "cairn_sidecar.web", "cairn_sidecar.rpc"]
failed = []
for m in mods:
    try:
        __import__(m)
    except Exception as e:  # noqa: BLE001
        failed.append(f"{m}: {type(e).__name__}: {e}")
import litellm
for name in ("openai/gpt-4o", "azure/gpt-4o", "anthropic/claude-sonnet-4-5", "azure_ai/x"):
    try:
        litellm.get_llm_provider(name)
    except Exception as e:  # noqa: BLE001
        failed.append(f"litellm route {name}: {type(e).__name__}: {e}")
# the Rust extension is gone (D-82)
from litellm import rust_bridge
if rust_bridge.native_bridge_available():
    failed.append("litellm.rust_bridge._native is still importable")
if failed:
    print("\n".join(failed))
    sys.exit(1)
print(f"[sidecar] {len(mods)} modules import, Python {sys.version.split()[0]}")
PY
log "smoke test: LiteLLM's route table (the routes Cairn uses must run in Python)"
env -i HOME=/nonexistent PATH=/usr/bin:/bin PYTHONDONTWRITEBYTECODE=1 LITELLM_LOCAL_MODEL_COST_MAP=True LITELLM_RUST=false \
    "$PYBIN" -I "$HERE/check-litellm-routes.py" --import
log "smoke test: ping"
reply="$(echo '{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}' |
    env -i HOME=/nonexistent PATH=/usr/bin:/bin PYTHONDONTWRITEBYTECODE=1 LITELLM_RUST=false timeout 30 "$PYBIN" -I -m cairn_sidecar --role ai)"
echo "$reply" | grep -q '"pong"' || { log "no pong: $reply"; exit 1; }

log "done: $(du -sh "$OUT" | cut -f1), $(find "$OUT" -type f | wc -l) files, in $OUT"
