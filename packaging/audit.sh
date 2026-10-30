#!/usr/bin/env bash
# packaging/audit.sh (`make audit`, NFR-SEC-6, D-52): checks the libraries for known security problems.
#
#   cargo audit   the Rust crates         (Cargo.lock)
#   npm audit     the UI packages         (app/package-lock.json, runtime dependencies only)
#   pip-audit     the sidecar's packages  (sidecar/requirements.lock)
#
# It goes online (to fetch the lists of known problems), so it asks first, every time. There is no
# "yes" option: it needs a terminal and your answer. It never runs by itself or from another command.
# A missing tool is not installed without asking either (installing also goes online).
# Exit code: 0 = all three checks ran and found nothing; 1 = a check found problems or could not run.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
cd "$REPO"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v npm >/dev/null 2>&1 && [ -s "$HOME/.nvm/nvm.sh" ]; then . "$HOME/.nvm/nvm.sh"; fi

AUDIT_VENV="$REPO/build/cache/audit-venv"      # pip-audit lives here, apart from the sidecar's packages
log() { echo "[audit] $*"; }

ask() {   # ask "question" -> 0 for yes; anything but y/yes is no
    local answer=""
    read -r -p "$1 [y/N] " answer || true
    case "$answer" in y|Y|yes|YES|Yes) return 0 ;; *) return 1 ;; esac
}

if [ ! -t 0 ]; then
    log "this needs a terminal: it asks before going online, and nothing can answer for you."
    exit 1
fi

pip_audit_bin() {
    if command -v pip-audit >/dev/null 2>&1; then command -v pip-audit
    elif [ -x "$AUDIT_VENV/bin/pip-audit" ]; then echo "$AUDIT_VENV/bin/pip-audit"
    fi
}

have_cargo_audit=0; cargo audit --version >/dev/null 2>&1 && have_cargo_audit=1
have_npm=0;         command -v npm >/dev/null 2>&1 && have_npm=1
have_pip_audit=0;   [ -n "$(pip_audit_bin || true)" ] && have_pip_audit=1

log "tools: cargo-audit $([ $have_cargo_audit = 1 ] && echo found || echo MISSING), npm $([ $have_npm = 1 ] && echo found || echo MISSING), pip-audit $([ $have_pip_audit = 1 ] && echo found || echo MISSING)"
[ $have_cargo_audit = 1 ] || log "  to get cargo-audit (online, compiles for a few minutes): cargo install cargo-audit --locked"
[ $have_npm = 1 ] || log "  to get npm: install Node.js, for example with nvm"
[ $have_pip_audit = 1 ] || log "  to get pip-audit (online), in its own folder: python3 -m venv $AUDIT_VENV && $AUDIT_VENV/bin/pip install pip-audit"
log "  (this script offers to install the missing ones after you agree to go online)"
echo
echo "This downloads the lists of known problems:"
echo "  - the RustSec advisory database (from github.com), for the Rust crates"
echo "  - the npm advisories (from registry.npmjs.org), for the UI packages"
echo "  - the PyPI advisories (from pypi.org), for the sidecar's Python packages"
if ! ask "Continue?"; then
    log "stopped. Nothing was downloaded."
    exit 0
fi

failed=0
skipped=()

# ---- Rust
if [ $have_cargo_audit = 0 ]; then
    log "cargo-audit is missing. Installing it compiles it from crates.io (online, a few minutes):"
    log "    cargo install cargo-audit --locked"
    if command -v cargo >/dev/null 2>&1 && ask "Install cargo-audit now?"; then
        cargo install cargo-audit --locked && have_cargo_audit=1
    fi
fi
if [ $have_cargo_audit = 1 ]; then
    log "cargo audit"
    cargo audit || failed=1
else
    skipped+=("cargo audit")
fi

# ---- npm
if [ $have_npm = 1 ]; then
    log "npm audit (runtime dependencies)"
    (cd app && npm audit --omit=dev) || failed=1
else
    log "npm is missing: install Node.js (for example with nvm), then run this again."
    skipped+=("npm audit")
fi

# ---- Python
if [ $have_pip_audit = 0 ]; then
    log "pip-audit is missing. It can live in its own folder, $AUDIT_VENV (online):"
    log "    python3 -m venv $AUDIT_VENV && $AUDIT_VENV/bin/pip install pip-audit"
    if ask "Install pip-audit there now?"; then
        python3 -m venv "$AUDIT_VENV" && "$AUDIT_VENV/bin/pip" install --quiet pip-audit && have_pip_audit=1
    fi
fi
if [ $have_pip_audit = 1 ]; then
    log "pip-audit"
    # --no-deps and --disable-pip: the lock is complete, so nothing has to be resolved or installed.
    "$(pip_audit_bin)" -r sidecar/requirements.lock --no-deps --disable-pip || failed=1
else
    skipped+=("pip-audit")
fi

echo
if [ ${#skipped[@]} -gt 0 ]; then
    log "not run: ${skipped[*]}"
    failed=1
fi
if [ $failed = 0 ]; then
    log "all three checks ran and found no known problems."
else
    log "see above: a check reported problems or could not run."
fi
exit $failed
