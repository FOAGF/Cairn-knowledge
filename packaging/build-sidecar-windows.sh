#!/usr/bin/env bash
# packaging/build-sidecar-windows.sh (M8, NFR-INS-1, D-55, D-57): builds the sidecar bundle for Windows x64
# ON LINUX. Output: packaging/build/windows/sidecar/python/ (python.exe, DLLs, Lib\ with the packages).
# The app runs `<exe folder>\sidecar\python\python.exe -I -m cairn_sidecar`.
#
# Steps (same shape as build-sidecar.sh for Linux):
#   1. python-build-standalone for Windows (same tag and Python version as the Linux one, sha256 pinned).
#      From packaging/cache/, else DOWNLOADED (online step, about 21 MB from github.com).
#   2. The Windows wheels for sidecar/requirements.lock: `pip download --platform win_amd64` into
#      packaging/cache/wheels-win (online step, about 120 MB, only when that folder is not complete for
#      this lock), LiteLLM's wheel checked against its sha256 (pip-pins.sh), then an offline
#      `pip install --target` into Lib\site-packages. The Linux dev Python's pip does both; nothing of the
#      Windows side is run. The sidecar's own package is copied in.
#      A locked package without a Windows wheel stops the build (pip says which).
#   3. Pruning (the Linux list where it applies, plus Windows' tcl/tk, test modules and import libraries),
#      then LiteLLM's route table read from catalog.py (check-litellm-routes.py --source, D-82).
#   4. Compiled files only (D-57): every .py becomes a .pyc beside it (compileall -b, unchecked-hash,
#      compiled by the Linux Python 3.12, whose bytecode is the same) and the .py is deleted. Fewer files
#      for Defender to scan, and no .py to carry a Mark of the Web problem. A .py that cannot be compiled
#      or that a package reads as data stays (KEEP_PY below).
#   5. Checks that run on Linux: no symlinks, no .py left (except KEEP_PY), every .pyc has the 3.12 magic
#      number, is unchecked-hash and unmarshals, python.exe exists, LiteLLM's Rust extension is gone, file
#      count and size.
#      The import smoke test cannot run here: the self-test of the exe covers it on Windows.
#
# CAIRN_HOST_PYTHON: the Linux Python 3.12 used for pip and compiling (default: sidecar/.venv, else a
# copy of the Linux python-build-standalone in packaging/build/host-python, unpacked from the tarball
# that build-sidecar.sh keeps in packaging/cache/ and downloaded like there if it is missing; the copy
# in packaging/build/sidecar has no pip, it is pruned).
# CAIRN_WIN_PBS_TARBALL=<file>: a local copy of the Windows Python tarball (checked against the sha256).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"

# ---- pins: bump all of them together (and the Linux ones in build-sidecar.sh); check the sha256 by hand
TAG=20261009
PY=3.12.15
FILE="cpython-${PY}+${TAG}-x86_64-pc-windows-msvc-install_only_stripped.tar.gz"
SHA256=11ed9f8a6ac64fb1b5a55e40e305a6744a3f77a70863c8f8197a6e4349abc64a   # from the release's SHA256SUMS
URL="https://github.com/astral-sh/python-build-standalone/releases/download/${TAG}/${FILE//+/%2B}"

CACHE="$HERE/cache"
OUT="$HERE/build/windows/sidecar"
LOCK="$REPO/sidecar/requirements.lock"
WHEELS="$CACHE/wheels-win"
PYROOT="$OUT/python"
LIB="$PYROOT/Lib"
SP="$LIB/site-packages"

# .py files that stay (path relative to Lib/site-packages). Empty: nothing was found that reads a .py as data.
KEEP_PY=()

log() { echo "[sidecar-win] $*"; }
sha_ok() { [ -f "$1" ] && [ "$(sha256sum "$1" | cut -d' ' -f1)" = "$SHA256" ]; }
# shellcheck source=packaging/pip-pins.sh
. "$HERE/pip-pins.sh"

# ---- the Linux Python 3.12 that runs pip and compileall
usable_python() {
    [ -n "$1" ] && [ -x "$1" ] && "$1" -I -c 'import sys; sys.exit(0 if sys.version_info[:2]==(3,12) else 1)' 2>/dev/null \
        && "$1" -I -m pip --version >/dev/null 2>&1
}
# The Linux python-build-standalone of build-sidecar.sh (same tag and version, its sha256 pin read from
# there), unpacked once into packaging/build/host-python. Logs go to stderr: stdout is the path.
host_pbs() {
    local file sha url tarball dir="$HERE/build/host-python"
    if usable_python "$dir/python/bin/python3"; then echo "$dir/python/bin/python3"; return; fi
    file="cpython-${PY}+${TAG}-x86_64-unknown-linux-gnu-install_only_stripped.tar.gz"
    sha="$(sed -n 's/^SHA256=//p' "$HERE/build-sidecar.sh")"
    url="https://github.com/astral-sh/python-build-standalone/releases/download/${TAG}/${file//+/%2B}"
    tarball="$CACHE/$file"
    mkdir -p "$CACHE"
    if [ ! -f "$tarball" ] || [ "$(sha256sum "$tarball" | cut -d' ' -f1)" != "$sha" ]; then
        log "ONLINE STEP: downloading the Linux python-build-standalone ($file, about 34 MB) from github.com" >&2
        curl -fL --retry 3 -o "$tarball.part" "$url" >&2
        mv "$tarball.part" "$tarball"
    fi
    [ "$(sha256sum "$tarball" | cut -d' ' -f1)" = "$sha" ] || { log "$tarball does not match the sha256 pinned in build-sidecar.sh" >&2; rm -f "$tarball"; return 1; }
    rm -rf "$dir"
    mkdir -p "$dir"
    tar -xzf "$tarball" -C "$dir"
    echo "$dir/python/bin/python3"
}
pick_host_python() {
    local c
    for c in "${CAIRN_HOST_PYTHON:-}" "$REPO/sidecar/.venv/bin/python"; do
        if usable_python "$c"; then echo "$c"; return; fi
    done
    c="$(host_pbs)" && usable_python "$c" && { echo "$c"; return; }
    log "no Linux Python 3.12 with pip found; set CAIRN_HOST_PYTHON (the dev venv is made by scripts/setup-sidecar-dev.sh)" >&2
    exit 1
}
HOSTPY="$(pick_host_python)"
log "host Python: $HOSTPY ($("$HOSTPY" -I -c 'import sys; print(sys.version.split()[0])'))"

# ---------------------------------------------------------------- 1. Python
mkdir -p "$CACHE"
TARBALL="$CACHE/$FILE"
if ! sha_ok "$TARBALL"; then
    rm -f "$TARBALL"
    if [ -n "${CAIRN_WIN_PBS_TARBALL:-}" ] && sha_ok "$CAIRN_WIN_PBS_TARBALL"; then
        log "using the local copy $CAIRN_WIN_PBS_TARBALL"
        cp "$CAIRN_WIN_PBS_TARBALL" "$TARBALL"
    fi
fi
if ! sha_ok "$TARBALL"; then
    log "ONLINE STEP: downloading python-build-standalone for Windows ($FILE, about 21 MB) from github.com"
    curl -fL --retry 3 -o "$TARBALL.part" "$URL"
    mv "$TARBALL.part" "$TARBALL"
    sha_ok "$TARBALL" || { log "the download does not match the pinned sha256 ($SHA256); removing it"; rm -f "$TARBALL"; exit 1; }
fi
log "Python tarball OK (sha256 $SHA256)"

rm -rf "$OUT"
mkdir -p "$OUT"
tar -xzf "$TARBALL" -C "$OUT"   # creates $OUT/python/
[ -f "$PYROOT/python.exe" ] || { log "the tarball has no python/python.exe"; exit 1; }

# ---------------------------------------------------------------- 2. packages
stamp="$WHEELS/.complete-$(sha256sum "$LOCK" | cut -c1-16)"
if [ ! -f "$stamp" ]; then
    log "ONLINE STEP: downloading the Windows wheels listed in sidecar/requirements.lock (about 120 MB,"
    log "from pypi.org and files.pythonhosted.org) into $WHEELS"
    mkdir -p "$WHEELS"
    "$HOSTPY" -I -m pip download -q --disable-pip-version-check --platform win_amd64 --python-version 3.12 \
        --implementation cp --abi cp312 --only-binary=:all: --no-deps -r "$LOCK" -d "$WHEELS" || {
        log "pip could not get a Windows wheel for a locked package (see its message above). Nothing was installed."
        exit 1
    }
    touch "$stamp"
fi
litellm_wheel_ok "$LOCK" "$WHEELS" "$LITELLM_WHEEL_WIN" "$LITELLM_SHA256_WIN" || exit 1
log "LiteLLM wheel OK (sha256 $LITELLM_SHA256_WIN)"
log "installing the wheels offline into Lib\\site-packages"
mkdir -p "$SP"
"$HOSTPY" -I -m pip install --disable-pip-version-check --no-index --no-cache-dir --no-compile --no-deps \
    --platform win_amd64 --python-version 3.12 --implementation cp --abi cp312 --only-binary=:all: \
    --find-links "$WHEELS" --target "$SP" -r "$LOCK" --quiet
# console scripts that pip puts in <target>/bin (the sidecar runs by `-m`)
rm -rf "$SP/bin"
# The sidecar's own package: plain Python files (compiled below).
mkdir -p "$SP/cairn_sidecar"
cp "$REPO"/sidecar/cairn_sidecar/*.py "$SP/cairn_sidecar/"
log "installed: $(du -sh "$SP" | cut -f1) in site-packages"

# Dependencies that apply on Windows but are not in the lock (it was made on Linux), e.g. colorama.
"$HOSTPY" -I - "$LOCK" "$WHEELS" <<'PY'
import email, glob, re, sys, zipfile
from pip._vendor.packaging.requirements import Requirement
norm = lambda n: re.sub(r"[-_.]+", "-", n).lower()
lock = {norm(l.split("==")[0]) for l in open(sys.argv[1]) if "==" in l and not l.startswith("#")}
env = {"sys_platform": "win32", "platform_system": "Windows", "os_name": "nt", "platform_machine": "AMD64",
       "python_version": "3.12", "python_full_version": "3.12.15", "implementation_name": "cpython",
       "platform_python_implementation": "CPython", "extra": ""}
# Known and harmless: tqdm and click only use colorama when it is importable.
optional = {"colorama"}
bad = []
for w in sorted(glob.glob(sys.argv[2] + "/*.whl")):
    z = zipfile.ZipFile(w)
    meta = [n for n in z.namelist() if n.endswith(".dist-info/METADATA")][0]
    for r in email.message_from_bytes(z.read(meta)).get_all("Requires-Dist") or []:
        q = Requirement(r)
        if (q.marker is None or q.marker.evaluate(env)) and norm(q.name) not in lock:
            if norm(q.name) in optional:
                print(f"[sidecar-win] note: {w.split('/')[-1]} wants {r} (optional, not installed)")
            else:
                bad.append(f"{w.split('/')[-1]} wants {r}")
if bad:
    print("[sidecar-win] Windows-only dependencies missing from sidecar/requirements.lock:\n  " + "\n  ".join(bad))
    sys.exit(1)
PY

# ---------------------------------------------------------------- 3. PRUNE
# Only what the sidecar provably never imports. The Linux list (build-sidecar.sh), where it applies.
rm -rf "$SP"/pip "$SP"/pip-*.dist-info "$SP"/setuptools* "$SP"/pkg_resources "$SP"/wheel* "$SP"/_distutils_hack
find "$SP" -type d \( -name tests -o -name test \) -prune -exec rm -rf {} +
rm -rf "$SP/litellm/proxy/_experimental/out"                              # the proxy's web UI (static files)
rm -rf "$SP"/onnxruntime/{quantization,tools,transformers,datasets}       # model tools, not inference
rm -rf "$SP"/hf_xet "$SP"/hf_xet-*.dist-info                              # faster downloads, we never download
rm -rf "$SP"/boto3 "$SP"/boto3-*.dist-info "$SP"/botocore "$SP"/botocore-*.dist-info \
       "$SP"/s3transfer "$SP"/s3transfer-*.dist-info "$SP"/jmespath "$SP"/jmespath-*.dist-info  # AWS, not offered
# LiteLLM's Rust extension (_native.pyd, 52 MB): none of Cairn's routes use it, the app runs the sidecar
# with LITELLM_RUST=false, and the check below reads LiteLLM's route table (FR-LLM-LIB-2, D-82)
rm -f "$SP"/litellm/rust_bridge/_native*
# Windows extras: type stubs, and numpy's C headers and import libraries (only for building against numpy)
find "$SP" -type f -name '*.pyi' -delete
rm -rf "$SP"/numpy/_core/include "$SP"/numpy/_core/lib "$SP"/numpy/random/lib
# Python itself
rm -rf "$PYROOT"/include "$PYROOT"/libs "$PYROOT"/Scripts "$PYROOT"/tcl \
       "$LIB"/test "$LIB"/idlelib "$LIB"/tkinter "$LIB"/turtledemo "$LIB"/turtle.py \
       "$LIB"/ensurepip "$LIB"/lib2to3 "$LIB"/pydoc_data
# tcl/tk, the test extension modules and the console-less launcher
rm -f "$PYROOT"/DLLs/_tkinter.pyd "$PYROOT"/DLLs/tcl86t.dll "$PYROOT"/DLLs/tk86t.dll \
      "$PYROOT"/DLLs/_test*.pyd "$PYROOT"/DLLs/_ctypes_test.pyd "$PYROOT"/pythonw.exe
find "$OUT" -type d -name __pycache__ -prune -exec rm -rf {} +
# The routes Cairn uses must run without the Rust extension. The Windows Python can't run here, so the
# check reads catalog.py (while it is still a .py) instead of importing it (D-82).
"$HOSTPY" -I "$HERE/check-litellm-routes.py" --source "$SP/litellm/rust_bridge/catalog.py"

# ---------------------------------------------------------------- 4. compiled files only (D-57)
log "compiling to .pyc beside the sources (unchecked-hash), then deleting the .py files"
# -b: legacy layout (x.pyc beside x.py), the only one Python loads without a source file.
# -s: strip the build folder from the paths stored in the code, so tracebacks say Lib/os.py.
"$HOSTPY" -I -m compileall -q -f -b -j 4 --invalidation-mode unchecked-hash -s "$PYROOT/" "$PYROOT" >/dev/null || true
cd "$PYROOT"
kept=0
while IFS= read -r -d '' py; do
    rel="${py#"$SP"/}"
    [ -f "${py}c" ] || { log "no .pyc for $py (it did not compile): kept"; kept=$((kept+1)); continue; }
    keep=0
    for k in "${KEEP_PY[@]+"${KEEP_PY[@]}"}"; do [ "$rel" = "$k" ] && keep=1; done
    [ "$keep" = 1 ] || rm -f "$py"
done < <(find "$PYROOT" -type f -name '*.py' -print0)
cd "$REPO"

# ---------------------------------------------------------------- 5. checks
links="$(find "$OUT" -type l)"
if [ -n "$links" ]; then
    log "symlinks left in the bundle:"; echo "$links"; exit 1
fi
left="$(find "$PYROOT" -type f -name '*.py' | sed "s#^$PYROOT/##" | sort)"
if [ -n "$left" ]; then
    nleft="$(echo "$left" | wc -l)"
    log "$nleft .py files are left:"; echo "$left" | head -20
    # allowed only if listed in KEEP_PY
    for f in $left; do
        ok=0
        for k in "${KEEP_PY[@]+"${KEEP_PY[@]}"}"; do [ "$f" = "Lib/site-packages/$k" ] && ok=1; done
        [ "$ok" = 1 ] || { log "not in KEEP_PY: $f"; exit 1; }
    done
fi
[ -f "$PYROOT/python.exe" ] || { log "python.exe is missing"; exit 1; }
"$HOSTPY" -I - "$PYROOT" <<'PY'
import importlib.util, marshal, os, sys
magic = importlib.util.MAGIC_NUMBER           # the host's Python is 3.12: 3531 + \r\n
assert int.from_bytes(magic[:2], "little") == 3531, magic
bad = total = 0
for root, _, files in os.walk(sys.argv[1]):
    for f in files:
        if f.endswith(".pyc"):
            total += 1
            with open(os.path.join(root, f), "rb") as fh:
                data = fh.read()
            ok = data[:4] == magic and int.from_bytes(data[4:8], "little") == 1   # 1 = hash-based, not checked
            if ok:
                try:
                    marshal.loads(data[16:])
                except Exception:  # noqa: BLE001
                    ok = False
            if not ok:
                bad += 1
                print("bad:", os.path.join(root, f))
print(f"[sidecar-win] {total} .pyc files, {bad} bad (magic, unchecked-hash, unmarshal)")
sys.exit(1 if bad or not total else 0)
PY
# the stdlib the interpreter needs at start-up is there as .pyc (landmark for finding its home)
for f in Lib/os.pyc Lib/encodings/utf_8.pyc Lib/site.pyc Lib/site-packages/cairn_sidecar/__main__.pyc; do
    [ -f "$PYROOT/$f" ] || { log "missing $f"; exit 1; }
done
# and LiteLLM's Rust extension is gone (D-82)
native="$(find "$SP/litellm/rust_bridge" -maxdepth 1 -name '_native*')"
[ -z "$native" ] || { log "LiteLLM's Rust extension is still in the bundle: $native"; exit 1; }
log "done: $(du -sh "$OUT" | cut -f1), $(find "$OUT" -type f | wc -l) files, in $OUT"
