#!/usr/bin/env bash
# packaging/release.sh (`make release`, `make release-deb`, `make release-lite`, `make release-deb-lite`,
# NFR-INS-1, NFR-INS-1b, D-46, D-74, D-82): builds the Cairn AppImage or Debian package and checks it once.
#
#   packaging/release.sh            every AppImage step below, in this order (sidecar, model, notice, appimage, selftest)
#   packaging/release.sh deb-all    the same for the Debian package (sidecar, model, notice, deb, selftest-deb)
#   packaging/release.sh lite       the lite AppImage, without the search model (sidecar, notice-lite, appimage-lite,
#                                   selftest-lite) -> Cairn-lite_<version>_amd64.AppImage
#   packaging/release.sh deb-lite   the lite Debian package (sidecar, notice-lite, deb-lite-only, selftest-deb-lite)
#                                   -> Cairn-lite_<version>_amd64.deb (the package is still called cairn)
#   packaging/release.sh sidecar    Python and the packages -> build/stage/sidecar (online on a first run)
#   packaging/release.sh model      the embedding model     -> build/stage/models
#   packaging/release.sh notice     the NOTICE file         -> build/stage/NOTICE (offline)
#   packaging/release.sh ui         only the UI build check (npm run build); `appimage` builds the UI itself
#   packaging/release.sh appimage   the AppImage (Tauri runs the UI build first)
#   packaging/release.sh selftest [FILE]  starts the AppImage once: the sidecar must answer and embedding must work
#   packaging/release.sh deb        the Debian package (Tauri runs the UI build first)
#   packaging/release.sh selftest-deb [FILE]  unpacks the package into a scratch folder and runs the same check
#   packaging/release.sh notice-lite | appimage-lite | selftest-lite [FILE] | deb-lite-only | selftest-deb-lite [FILE]
#                                   the same steps for the lite packages: no models/, NOTICE without the Gemma section,
#                                   and the self-test expects no model
#
# The finished packages go to build/ (D-85), the lite and full ones side by side: building one keeps the other's file.
# Settings (environment): CAIRN_JOBS (cargo jobs, default 5), CAIRN_EMBED_MODEL_SRC (see build-model.sh),
# CAIRN_APPIMAGE_FLAGS (extra arguments when the AppImage is started, e.g. none needed with libfuse2).
# The heavy steps run at low priority (nice 10), so the machine stays usable.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
cd "$REPO"

VERSION="$(python3 -I -c 'import json;print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
BUILD="$REPO/build"
STAGE="$BUILD/stage"
APPIMAGE="$BUILD/Cairn_${VERSION}_amd64.AppImage"
DEB="$BUILD/Cairn_${VERSION}_amd64.deb"
APPIMAGE_LITE="$BUILD/Cairn-lite_${VERSION}_amd64.AppImage"
DEB_LITE="$BUILD/Cairn-lite_${VERSION}_amd64.deb"
# Where Tauri writes them (full or lite: the name is the same), before they move to build/.
TAURI_APPIMAGE="$REPO/target/release/bundle/appimage/Cairn_${VERSION}_amd64.AppImage"
TAURI_DEB="$REPO/target/release/bundle/deb/Cairn_${VERSION}_amd64.deb"
# The lite overlay goes on top of the release overlay: it drops models/ and maps NOTICE-lite to NOTICE (D-82).
FULL_CONFIG=(--config packaging/tauri.release.conf.json)
LITE_CONFIG=(--config packaging/tauri.release.conf.json --config packaging/tauri.release.lite.conf.json)
log() { echo "[release] $*"; }

need_tools() {
    # cargo and node are normally on the PATH of a login shell; this makes `make release` work from any shell.
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    if ! command -v npm >/dev/null 2>&1 && [ -s "$HOME/.nvm/nvm.sh" ]; then . "$HOME/.nvm/nvm.sh"; fi
    for t in cargo npm; do
        command -v "$t" >/dev/null 2>&1 || { log "$t is not installed or not on the PATH"; exit 1; }
    done
    cargo tauri --version >/dev/null 2>&1 || { log "the Tauri command line tool is missing (cargo install tauri-cli, online)"; exit 1; }
}

step_sidecar() { nice -n 10 "$HERE/build-sidecar.sh"; }
step_model()   { nice -n 10 "$HERE/build-model.sh"; }
step_notice()  { python3 -I "$HERE/notice.py"; }
step_notice_lite() { python3 -I "$HERE/notice.py" --lite; }

step_ui() {
    need_tools
    [ -d app/node_modules ] || { log "app/node_modules is missing: run 'npm ci' in app/ first (online)"; exit 1; }
    log "UI build check (npm run build)"
    (cd app && nice -n 10 npm run build)
}

need_built() {
    local f
    for f in "$STAGE/sidecar/python/bin/python3" "$STAGE/models/embeddinggemma-300m/tokenizer.json" "$STAGE/NOTICE"; do
        [ -e "$f" ] || { log "missing $f: run the 'sidecar', 'model' and 'notice' steps first"; exit 1; }
    done
}
need_built_lite() {
    local f
    for f in "$STAGE/sidecar/python/bin/python3" "$STAGE/NOTICE-lite"; do
        [ -e "$f" ] || { log "missing $f: run the 'sidecar' and 'notice-lite' steps first"; exit 1; }
    done
}

# tauri_bundle KIND OUT FINAL CONFIG...: `cargo tauri build --bundles KIND`, then Tauri's OUT moved to FINAL in
# build/ (D-85). Tauri writes full and lite under the same name and empties bundle/appimage before an AppImage
# build; build/ is never touched by Tauri, so the packages of earlier builds stay there.
tauri_bundle() {
    local kind="$1" out="$2" final="$3"
    shift 3
    # The AppImage is laid out from a scratch Debian tree that Tauri doesn't empty first: one left by a failed
    # build could carry models/ into a lite AppImage (D-82).
    if [ "$kind" = appimage ]; then rm -rf target/release/bundle/appimage_deb; fi
    rm -f "$out"
    CARGO_BUILD_JOBS="${CAIRN_JOBS:-5}" nice -n 10 cargo tauri build --bundles "$kind" "$@" \
        || { log "the Tauri build failed"; exit 1; }
    [ -f "$out" ] || { log "expected $out, but it is not there"; ls -l "$(dirname "$out")" || true; exit 1; }
    mkdir -p "$BUILD"
    mv -f "$out" "$final"
}

step_appimage() {
    need_tools
    need_built
    log "building the AppImage (UI build, release compile, bundling); this takes a few minutes"
    # --config merges the release overlay: identifier app.cairn.desktop and the bundled sidecar, model and NOTICE.
    # The UI build is Tauri's beforeBuildCommand. No `npm ci` here.
    tauri_bundle appimage "$TAURI_APPIMAGE" "$APPIMAGE" "${FULL_CONFIG[@]}"
}

step_deb() {
    need_tools
    need_built
    log "building the Debian package (UI build, release compile, packaging); this takes a few minutes"
    # The same overlay as the AppImage, so both share one compile. The package uses the system's GTK and
    # WebKit (Tauri adds them to Depends) and installs the program as /usr/bin/cairn, its files in /usr/lib/Cairn.
    tauri_bundle deb "$TAURI_DEB" "$DEB" "${FULL_CONFIG[@]}"
}

# The lite packages (NFR-INS-1b, D-82): no models/, NOTICE-lite as NOTICE. Tauri embeds its configuration in
# the program, so after a full build only the app's own crate compiles again (and the other way round).
step_appimage_lite() {
    need_tools
    need_built_lite
    log "building the lite AppImage (no search model); this takes a few minutes"
    tauri_bundle appimage "$TAURI_APPIMAGE" "$APPIMAGE_LITE" "${LITE_CONFIG[@]}"
}

step_deb_lite() {
    need_tools
    need_built_lite
    log "building the lite Debian package (no search model); this takes a few minutes"
    tauri_bundle deb "$TAURI_DEB" "$DEB_LITE" "${LITE_CONFIG[@]}"
}

# check_selftest full|lite JSON [FOLDER]: the self-test's last line (JSON) must say ok, from Cairn's own Python.
# full: 256 dimensions from Cairn's own model. lite: no model bundled, so model_dir, dims and embed_ms are null.
# FOLDER, when given: the folder the Python (and the model) must be in.
check_selftest() {
    python3 -I - "$1" "$2" "${3:-}" <<'PY' || { log "self-test FAILED"; exit 1; }
import json, sys
mode, r, inside = sys.argv[1], json.loads(sys.argv[2]), sys.argv[3]
assert r.get("ok") is True, r
# the build must have used its own Python and model, not a development folder
assert r["python_path"].endswith("/sidecar/python/bin/python3"), r["python_path"]
assert "/.venv/" not in r["python_path"], r["python_path"]
if inside:
    assert r["python_path"].startswith(inside + "/"), (r["python_path"], inside)
if mode == "lite":
    for key in ("model_dir", "dims", "embed_ms"):
        assert key in r and r[key] is None, (key, r)
else:
    assert r["dims"] == 256, r
    assert r["model_dir"].endswith("/models/embeddinggemma-300m"), r["model_dir"]
    if inside:
        assert r["model_dir"].startswith(inside + "/"), (r["model_dir"], inside)
PY
    log "self-test OK ($1)"
}

# step_selftest [FILE] (full), step_selftest_as lite [FILE]
step_selftest() { step_selftest_as full "$@"; }
step_selftest_as() {
    local mode="$1" image
    image="${2:-$([ "$mode" = lite ] && echo "$APPIMAGE_LITE" || echo "$APPIMAGE")}"
    [ -x "$image" ] || { log "$image is missing or not executable: run the 'appimage' (or 'appimage-lite') step first"; exit 1; }
    local data out rc=0
    data="$(mktemp -d "${TMPDIR:-/tmp}/cairn-selftest.XXXXXX")"
    log "self-test: starting $image once (throw-away data folder $data)"
    # The dev overrides must not leak in: the check is about the files inside the AppImage.
    # shellcheck disable=SC2086
    out="$(env -u CAIRN_SIDECAR_PYTHON -u CAIRN_EMBED_MODEL_DIR CAIRN_DATA_DIR="$data" \
        timeout 300 "$image" --self-test ${CAIRN_APPIMAGE_FLAGS:-} | tail -n 1)" || rc=$?
    rm -rf "$data"
    echo "$out"
    [ "$rc" = 0 ] || { log "self-test FAILED (exit code $rc)"; exit 1; }
    check_selftest "$mode" "$out"
}

# step_selftest_deb [FILE] (full), step_selftest_deb_as lite [FILE]
step_selftest_deb() { step_selftest_deb_as full "$@"; }
step_selftest_deb_as() {
    local mode="$1" deb
    deb="${2:-$([ "$mode" = lite ] && echo "$DEB_LITE" || echo "$DEB")}"
    [ -f "$deb" ] || { log "$deb is missing: run the 'deb' (or 'deb-lite-only') step first"; exit 1; }
    command -v dpkg-deb >/dev/null 2>&1 || { log "dpkg-deb is missing (it comes with dpkg)"; exit 1; }
    dpkg-deb -f "$deb" Package Version Depends | sed 's/^/[release]   /'
    [ "$(dpkg-deb -f "$deb" Version)" = "$VERSION" ] || { log "the package's version is not $VERSION"; exit 1; }
    # lite and full are the same package, so one replaces the other (NFR-INS-1b)
    [ "$(dpkg-deb -f "$deb" Package)" = cairn ] || { log "the package's name is not cairn"; exit 1; }
    # Unpacked on disk next to the package (not in /tmp, which may be memory): the program looks for its
    # files in ../lib/Cairn, as when installed.
    local root data out rc=0
    root="$(mktemp -d "$(dirname "$deb")/selftest.XXXXXX")"
    root="$(cd "$root" && pwd -P)"
    data="$(mktemp -d "${TMPDIR:-/tmp}/cairn-selftest.XXXXXX")"
    log "self-test: unpacking $(basename "$deb") into $root and starting it once (throw-away data folder $data)"
    dpkg-deb -x "$deb" "$root"
    if [ "$mode" = lite ] && [ -e "$root/usr/lib/Cairn/models" ]; then
        rm -rf "${root:?}"; log "the lite package holds usr/lib/Cairn/models"; exit 1
    fi
    out="$(env -u CAIRN_SIDECAR_PYTHON -u CAIRN_EMBED_MODEL_DIR CAIRN_DATA_DIR="$data" \
        timeout 300 "$root/usr/bin/cairn" --self-test | tail -n 1)" || rc=$?
    rm -rf "${data:?}" "${root:?}"
    echo "$out"
    [ "$rc" = 0 ] || { log "self-test FAILED (exit code $rc)"; exit 1; }
    check_selftest "$mode" "$out" "$root/usr/lib/Cairn"
}

step_all() {
    step_sidecar
    step_model
    step_notice
    step_appimage
    step_selftest
    local size
    size="$(du -h "$APPIMAGE" | cut -f1)"
    log "done: $APPIMAGE ($size). Install it with: make install"
}

step_deb_all() {
    step_sidecar
    step_model
    step_notice
    step_deb
    step_selftest_deb
    local size
    size="$(du -h "$DEB" | cut -f1)"
    log "done: $DEB ($size). Install it with: sudo apt install $DEB"
}

step_lite() {
    step_sidecar
    step_notice_lite
    step_appimage_lite
    step_selftest_as lite
    log "done: $APPIMAGE_LITE ($(du -h "$APPIMAGE_LITE" | cut -f1)). The search model can be added in Settings > Search"
}

step_deb_lite_all() {
    step_sidecar
    step_notice_lite
    step_deb_lite
    step_selftest_deb_as lite
    log "done: $DEB_LITE ($(du -h "$DEB_LITE" | cut -f1)). Install it with: sudo apt install $DEB_LITE"
}

case "${1:-all}" in
    all)      step_all ;;
    deb-all)  step_deb_all ;;
    sidecar)  step_sidecar ;;
    model)    step_model ;;
    notice)   step_notice ;;
    ui)       step_ui ;;
    appimage) step_appimage ;;
    selftest) shift; step_selftest "$@" ;;
    deb)      step_deb ;;
    selftest-deb) shift; step_selftest_deb "$@" ;;
    lite)          step_lite ;;
    deb-lite)      step_deb_lite_all ;;
    notice-lite)   step_notice_lite ;;
    appimage-lite) step_appimage_lite ;;
    selftest-lite) shift; step_selftest_as lite "$@" ;;
    deb-lite-only) step_deb_lite ;;
    selftest-deb-lite) shift; step_selftest_deb_as lite "$@" ;;
    -h|--help|help) sed -n '2,26p' "${BASH_SOURCE[0]}" ;;
    *) echo "unknown step '$1'; try: all deb-all lite deb-lite sidecar model notice notice-lite ui appimage appimage-lite selftest selftest-lite deb deb-lite-only selftest-deb selftest-deb-lite" >&2; exit 2 ;;
esac
