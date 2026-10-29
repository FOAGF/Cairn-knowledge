#!/usr/bin/env bash
# packaging/build-all.sh (`make`): from a fresh copy of the source to both release files in one command,
# the Linux AppImage (packaging/release.sh) and the Windows zip (packaging/release-windows.sh).
#
#   packaging/build-all.sh [STEP...]    default: every step below, in this order
#     check    the tools and system packages that need a manual or administrator install; stops with
#              the commands to run when one is missing (nothing is installed with sudo)
#     tools    for this user, when missing (ONLINE STEP): the Tauri command line tool, and for Windows
#              cargo-xwin and the Rust target x86_64-pc-windows-msvc
#     npm      `npm ci` in app/ when app/node_modules is missing (ONLINE STEP)
#     model    packaging/fetch-model.sh when no copy of the model is found (ONLINE STEP, about 200 MB)
#     linux    packaging/release.sh: sidecar, model, NOTICE, AppImage, self-test
#     windows  packaging/release-windows.sh: sidecar, model, NOTICE, cairn.exe, zip
#   Linux only: packaging/build-all.sh check tools npm model linux
#
# The first run downloads a lot (README "Building: what goes online"); later runs download nothing.
# Settings: CAIRN_JOBS (cargo jobs, default 5) and those of the release scripts. The heavy steps run
# at low priority (nice 10), so the machine stays usable.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
cd "$REPO"

WIN_TARGET=x86_64-pc-windows-msvc
TAURI_CLI_VERSION=2.12.2    # the versions the release was built with
XWIN_VERSION=0.23.1
log() { echo "[build-all] $*"; }

STEPS=("$@")
[ ${#STEPS[@]} -gt 0 ] || STEPS=(check tools npm model linux windows)
wants() { local s; for s in "${STEPS[@]}"; do [ "$s" = "$1" ] && return 0; done; return 1; }
# The Windows tools are checked and installed only when the Windows build is one of the steps.
windows_too() { wants windows; }

load_path() {
    # cargo and node are normally on the PATH of a login shell; this makes `make` work from any shell.
    [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
    if ! command -v npm >/dev/null 2>&1 && [ -s "$HOME/.nvm/nvm.sh" ]; then . "$HOME/.nvm/nvm.sh"; fi
    return 0
}

# at_least HAVE NEED: version HAVE (e.g. 1.92.1) is NEED or newer.
at_least() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n 1)" = "$2" ]; }

step_check() {
    local problems=() missing_pkgs=() p rust_need have
    for p in cargo rustup rustc; do
        command -v "$p" >/dev/null 2>&1 || { problems+=("Rust is missing: install it with rustup (https://rustup.rs), then open a new shell"); break; }
    done
    if command -v rustc >/dev/null 2>&1; then
        rust_need="$(sed -n 's/^rust-version *= *"\(.*\)"/\1/p' Cargo.toml)"
        have="$(rustc --version | cut -d' ' -f2)"
        at_least "$have" "$rust_need" || problems+=("Rust $have is too old, $rust_need or newer is needed: rustup update")
    fi
    if command -v node >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
        have="$(node --version | tr -d v)"
        # Vite's own requirement (app/node_modules/vite/package.json "engines").
        if ! { at_least "$have" 22.12.0 || { at_least "$have" 20.19.0 && [ "${have%%.*}" = 20 ]; }; }; then
            problems+=("Node.js $have is too old: 20.19 or newer (or 22.12 or newer) is needed, e.g. through nvm")
        fi
    else
        problems+=("Node.js and npm are missing: install Node.js 22 or newer, e.g. through nvm (https://github.com/nvm-sh/nvm)")
    fi
    for p in python3 curl sha256sum tar; do
        command -v "$p" >/dev/null 2>&1 || problems+=("$p is missing")
    done

    # System packages: Tauri's libraries, the AppImage tools and, for Windows, clang and lld.
    if command -v dpkg-query >/dev/null 2>&1; then
        local pkgs=(build-essential pkg-config curl file xdg-utils libwebkit2gtk-4.1-dev libdbus-1-dev librsvg2-dev)
        windows_too && pkgs+=(clang lld llvm)
        for p in "${pkgs[@]}"; do
            dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q "ok installed" || missing_pkgs+=("$p")
        done
        # FUSE 2 runs the AppImage tools and the self-test; Ubuntu 24.04 calls the package libfuse2t64.
        if ! dpkg-query -W -f='${Status}' libfuse2t64 2>/dev/null | grep -q "ok installed" \
           && ! dpkg-query -W -f='${Status}' libfuse2 2>/dev/null | grep -q "ok installed"; then
            missing_pkgs+=(libfuse2t64)
        fi
        [ ${#missing_pkgs[@]} -eq 0 ] || problems+=("system packages are missing: sudo apt install ${missing_pkgs[*]}")
    else
        log "not a Debian or Ubuntu system: check by hand that the packages in README \"What you need\" are installed"
    fi

    if [ ${#problems[@]} -gt 0 ]; then
        log "this machine is not ready to build Cairn:"
        for p in "${problems[@]}"; do log "  - $p"; done
        log "fix these and run make again"
        exit 1
    fi
    log "check OK: Rust $(rustc --version | cut -d' ' -f2), Node.js $(node --version)"
}

step_tools() {
    local jobs="${CAIRN_JOBS:-5}"
    if ! cargo tauri --version >/dev/null 2>&1; then
        log "ONLINE STEP: installing the Tauri command line tool $TAURI_CLI_VERSION (cargo install, a few minutes)"
        nice -n 10 cargo install tauri-cli --version "$TAURI_CLI_VERSION" --locked --jobs "$jobs"
    fi
    if windows_too; then
        if ! command -v cargo-xwin >/dev/null 2>&1; then
            log "ONLINE STEP: installing cargo-xwin $XWIN_VERSION (cargo install, a few minutes)"
            nice -n 10 cargo install cargo-xwin --version "$XWIN_VERSION" --locked --jobs "$jobs"
        fi
        if ! rustup target list --installed 2>/dev/null | grep -q "^$WIN_TARGET\$"; then
            log "ONLINE STEP: adding the Rust target $WIN_TARGET"
            rustup target add "$WIN_TARGET"
        fi
    fi
    log "tools OK: $(cargo tauri --version)$(windows_too && echo ", $(cargo-xwin --version)")"
}

step_npm() {
    # Only into a fresh copy: an existing app/node_modules is left as it is (a running Vite uses it).
    if [ ! -d app/node_modules ]; then
        log "ONLINE STEP: installing the UI packages (npm ci in app/)"
        (cd app && nice -n 10 npm ci --no-audit --no-fund)
    else
        log "UI packages: app/node_modules is there (after a package-lock.json change, run npm ci in app/)"
    fi
}

step_model() {
    # shellcheck source=packaging/model-pins.sh
    . "$HERE/model-pins.sh"
    if [ -n "${CAIRN_EMBED_MODEL_SRC:-}" ]; then
        log "model: using CAIRN_EMBED_MODEL_SRC ($CAIRN_EMBED_MODEL_SRC)"
    else
        "$HERE/fetch-model.sh"
    fi
}

step_linux()   { "$HERE/release.sh"; }
step_windows() { "$HERE/release-windows.sh"; }

load_path
began=$(date +%s)
for s in "${STEPS[@]}"; do
    case "$s" in
        check|tools|npm|model|linux|windows) log "== $s"; "step_$s" ;;
        -h|--help|help) sed -n '2,19p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown step '$s'; try: check tools npm model linux windows" >&2; exit 2 ;;
    esac
done

VERSION="$(python3 -I -c 'import json;print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
log "done in $(( ($(date +%s) - began) / 60 )) min"
if wants linux; then
    f="target/release/bundle/appimage/Cairn_${VERSION}_amd64.AppImage"
    log "Linux:   $REPO/$f ($(du -h "$f" | cut -f1)); install it with: make install"
fi
if wants windows; then
    f="packaging/build/windows/Cairn-${VERSION}-windows-x64.zip"
    log "Windows: $REPO/$f ($(du -h "$f" | cut -f1)); unzip it on Windows outside OneDrive and read README.txt"
fi
