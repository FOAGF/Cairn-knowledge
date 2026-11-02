# Sourced by the release scripts (release.sh, release-windows.sh, build-all.sh, publish.sh, audit.sh):
# the repository, the version being built, the tools on the PATH and a yes/no question. The script
# that sources it defines `log` and goes to "$REPO" itself.

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$REPO/src-tauri/tauri.conf.json")"

# cargo and node are normally on the PATH of a login shell; this makes `make` work from any shell.
load_path() {
    if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
    if ! command -v npm >/dev/null 2>&1 && [ -s "$HOME/.nvm/nvm.sh" ]; then . "$HOME/.nvm/nvm.sh"; fi
    return 0
}

# cargo, npm and the Tauri command line tool, or a message and exit 1.
need_tools() {
    load_path
    for t in cargo npm; do
        command -v "$t" >/dev/null 2>&1 || { log "$t is not installed or not on the PATH"; exit 1; }
    done
    cargo tauri --version >/dev/null 2>&1 || { log "the Tauri command line tool is missing (cargo install tauri-cli, online)"; exit 1; }
}

# The UI build as a check before packaging (Tauri runs it again as its beforeBuildCommand).
step_ui() {
    need_tools
    [ -d app/node_modules ] || { log "app/node_modules is missing: run 'npm ci' in app/ first (online)"; exit 1; }
    log "UI build check (npm run build)"
    (cd app && nice -n 10 npm run build)
}

ask() {   # ask "question" -> 0 for yes; anything but y/yes is no
    local answer=""
    read -r -p "$1 [y/N] " answer || true
    case "$answer" in y|Y|yes|YES|Yes) return 0 ;; *) return 1 ;; esac
}
