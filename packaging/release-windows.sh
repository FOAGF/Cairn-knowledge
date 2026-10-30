#!/usr/bin/env bash
# packaging/release-windows.sh (`make release-windows`, `make release-windows-lite`, M8, NFR-INS-1, NFR-INS-1b,
# D-55 to D-59, D-82): builds the Windows zip ON LINUX.
#
#   packaging/release-windows.sh           every step below, in this order
#   packaging/release-windows.sh lite      the lite zip, without the search model (sidecar, notice-lite, exe, zip-lite)
#   packaging/release-windows.sh sidecar   Python and the packages for Windows -> build/stage/windows/sidecar
#                                          (online on a first run: the Python tarball and the Windows wheels)
#   packaging/release-windows.sh model     the embedding model -> build/stage/windows/models (offline)
#   packaging/release-windows.sh notice    the NOTICE file     -> build/stage/windows/NOTICE (offline)
#   packaging/release-windows.sh ui        only the UI build check (npm run build); `exe` builds the UI itself
#   packaging/release-windows.sh exe       cairn.exe: cargo-xwin, target x86_64-pc-windows-msvc, no bundle
#   packaging/release-windows.sh zip       the folder Cairn-<version>/ and its zip -> build/
#   packaging/release-windows.sh notice-lite  the NOTICE without the Gemma section -> build/stage/windows/NOTICE-lite
#   packaging/release-windows.sh zip-lite  the folder Cairn-<version>-lite/ (no models\, NOTICE-lite as NOTICE) and its zip
#
# Result: build/Cairn-<version>-windows-x64.zip, or Cairn-<version>-windows-x64-lite.zip from the
# same cairn.exe and sidecar. It holds no data, config or keys (D-58).
# Needs cargo-xwin, clang and lld (the Windows SDK is in ~/.cache/cargo-xwin; see poc/j-windows/README.md).
# Settings (environment): CAIRN_JOBS (cargo jobs, default 5), CAIRN_EMBED_MODEL_SRC (see build-model.sh),
# CAIRN_HOST_PYTHON and CAIRN_WIN_PBS_TARBALL (see build-sidecar-windows.sh). Heavy steps run at nice 10.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
cd "$REPO"

STAGE="$REPO/build/stage/windows"
TARGET=x86_64-pc-windows-msvc
VERSION="$(python3 -I -c 'import json;print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
EXE="$REPO/target/$TARGET/release/cairn.exe"
NAME="Cairn-${VERSION}"
ZIP="$REPO/build/${NAME}-windows-x64.zip"
ZIP_LITE="$REPO/build/${NAME}-windows-x64-lite.zip"
log() { echo "[release-windows] $*"; }

need_tools() {
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    if ! command -v npm >/dev/null 2>&1 && [ -s "$HOME/.nvm/nvm.sh" ]; then . "$HOME/.nvm/nvm.sh"; fi
    for t in cargo npm; do
        command -v "$t" >/dev/null 2>&1 || { log "$t is not installed or not on the PATH"; exit 1; }
    done
    cargo tauri --version >/dev/null 2>&1 || { log "the Tauri command line tool is missing (cargo install tauri-cli, online)"; exit 1; }
}
need_xwin() {
    command -v cargo-xwin >/dev/null 2>&1 || { log "cargo-xwin is missing (cargo install cargo-xwin, online); see poc/j-windows/README.md"; exit 1; }
    rustup target list --installed 2>/dev/null | grep -q "^$TARGET\$" || { log "the Rust target $TARGET is missing (rustup target add $TARGET, online)"; exit 1; }
}

step_sidecar() { nice -n 10 "$HERE/build-sidecar-windows.sh"; }

step_model() {
    nice -n 10 "$HERE/build-model.sh"     # the output is the same on every platform: build/stage/models
    rm -rf "$STAGE/models"
    mkdir -p "$STAGE"
    cp -a "$REPO/build/stage/models" "$STAGE/models"
    log "model copied to $STAGE/models"
}

step_notice() { python3 -I "$HERE/notice.py" --windows; }
step_notice_lite() { python3 -I "$HERE/notice.py" --windows --lite; }

step_ui() {
    need_tools
    [ -d app/node_modules ] || { log "app/node_modules is missing: run 'npm ci' in app/ first (online)"; exit 1; }
    log "UI build check (npm run build)"
    (cd app && nice -n 10 npm run build)
}

step_exe() {
    need_tools
    need_xwin
    log "building cairn.exe for Windows (UI build, release compile with cargo-xwin); this takes several minutes"
    # --no-bundle: no installer (D-56), and Tauri copies no resources then; the zip step lays them out.
    # The overlay sets the release identifier. The UI build is Tauri's beforeBuildCommand.
    CARGO_BUILD_JOBS="${CAIRN_JOBS:-5}" nice -n 10 cargo tauri build --runner cargo-xwin --target "$TARGET" \
        --no-bundle --config packaging/tauri.release.windows.conf.json
    [ -f "$EXE" ] || { log "expected $EXE, but it is not there"; exit 1; }
    log "built $EXE ($(du -h "$EXE" | cut -f1))"
}

# step_zip (full) or step_zip lite: the lite zip has no models\ and takes NOTICE-lite as its NOTICE (D-82).
step_zip() {
    local mode="${1:-full}" name="$NAME" zip="$ZIP" notice="$STAGE/NOTICE" model="$STAGE/models/embeddinggemma-300m/tokenizer.json"
    if [ "$mode" = lite ]; then
        name="$NAME-lite"; zip="$ZIP_LITE"; notice="$STAGE/NOTICE-lite"; model=""
    fi
    for f in "$EXE" "$STAGE/sidecar/python/python.exe" ${model:+"$model"} "$notice" \
             "$HERE/windows/README.txt" "$HERE/windows/check-windows.ps1" "$HERE/windows/ui-check.js"; do
        [ -e "$f" ] || { log "missing $f: run the 'sidecar', $([ "$mode" = lite ] && echo "'notice-lite'" || echo "'model', 'notice'") and 'exe' steps first"; exit 1; }
    done
    log "writing $name/ into the zip"
    python3 -I - "$STAGE" "$EXE" "$HERE/windows" "$name" "$zip" "$mode" <<'PY'
import os, sys, zipfile
stage, exe, winsrc, name, zippath, mode = sys.argv[1:7]
lite = mode == "lite"
TEXT = {".txt", ".ps1"}   # top-level text files: plain ASCII (PowerShell 5.1 reads a .ps1 without BOM as ANSI), CRLF
files = [(exe, "cairn.exe"), (os.path.join(stage, "NOTICE-lite" if lite else "NOTICE"), "NOTICE")]
for f in ("README.txt", "check-windows.ps1", "ui-check.js"):
    files.append((os.path.join(winsrc, f), f))
trees = [(os.path.join(stage, "sidecar", "python"), "sidecar/python")]
if not lite:
    trees.append((os.path.join(stage, "models", "embeddinggemma-300m"), "models/embeddinggemma-300m"))
for top, prefix in trees:
    for root, dirs, names in os.walk(top):
        dirs.sort()
        for n in names:
            src = os.path.join(root, n)
            assert not os.path.islink(src), src
            files.append((src, prefix + "/" + os.path.relpath(src, top).replace(os.sep, "/")))
files.sort(key=lambda p: p[1])
assert len({a for _, a in files}) == len(files)
for _, a in files:
    assert a.split("/")[0].lower() not in ("data", "config.json", "settings.json"), a   # D-58: no data in the zip
stamp = (2026, 1, 1, 0, 0, 0)   # fixed, so the same files give the same zip
tmp = zippath + ".part"
with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED, compresslevel=6) as z:
    for src, arc in files:
        info = zipfile.ZipInfo(f"{name}/{arc}", stamp)
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o644 << 16
        if "/" not in arc and os.path.splitext(arc)[1] in TEXT:
            data = open(src, "rb").read().replace(b"\r\n", b"\n")
            data.decode("ascii")
            z.writestr(info, data.replace(b"\n", b"\r\n"), compresslevel=6)
        else:
            with open(src, "rb") as fh, z.open(info, "w", force_zip64=os.path.getsize(src) > (1 << 30)) as out:
                while chunk := fh.read(1 << 20):
                    out.write(chunk)
os.replace(tmp, zippath)
with zipfile.ZipFile(zippath) as z:
    bad = z.testzip()
    assert bad is None, bad
    names = z.namelist()
need = ["cairn.exe", "NOTICE", "README.txt", "check-windows.ps1", "ui-check.js", "sidecar/python/python.exe"]
if not lite:
    need += ["models/embeddinggemma-300m/onnx/model_q4.onnx", "models/embeddinggemma-300m/onnx/model_q4.onnx_data",
             "models/embeddinggemma-300m/tokenizer.json"]
missing = [n for n in (f"{name}/{n}" for n in need) if n not in names]
assert not missing, missing
if lite:
    assert not any(n.startswith(f"{name}/models/") for n in names), "the lite zip holds models/"
print(f"[release-windows] {zippath}")
print(f"[release-windows] {os.path.getsize(zippath) / 1048576:.0f} MB, {len(names)} files")
PY
}

step_all() {
    step_sidecar
    step_model
    step_notice
    step_exe
    step_zip
    log "done: $ZIP ($(du -h "$ZIP" | cut -f1)). Unzip it on Windows, outside OneDrive, and read README.txt"
}

step_lite() {
    step_sidecar
    step_notice_lite
    step_exe
    step_zip lite
    log "done: $ZIP_LITE ($(du -h "$ZIP_LITE" | cut -f1)). Unzip it on Windows, outside OneDrive, and read README.txt"
}

case "${1:-all}" in
    all)     step_all ;;
    sidecar) step_sidecar ;;
    model)   step_model ;;
    notice)  step_notice ;;
    ui)      step_ui ;;
    exe)     step_exe ;;
    zip)     step_zip ;;
    lite)    step_lite ;;
    notice-lite) step_notice_lite ;;
    zip-lite)    step_zip lite ;;
    -h|--help|help) sed -n '2,21p' "${BASH_SOURCE[0]}" ;;
    *) echo "unknown step '$1'; try: all lite sidecar model notice notice-lite ui exe zip zip-lite" >&2; exit 2 ;;
esac
