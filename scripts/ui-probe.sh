#!/bin/bash
# Runs a JavaScript probe inside the debug build's window and prints what it reports, without
# typing or clicking in the window (the owner's keyboard and focus are never touched).
#
#   CAIRN_DATA_DIR=/tmp/x scripts/ui-probe.sh scripts/probes/dialog-flash.js [VAR=value ...]
#
# The page shown is the devUrl baked into the last build (see capture-window.sh). The probe gets a
# `report(text)` function; each call is printed here. Env: PROBE_AFTER (s before the probe runs,
# default 10), PROBE_EXIT (s before the app closes, default 20), PROBE_PORT (default 5199).
# A fresh data folder starts in internal knowledge mode (CAIRN_DEV_KNOWLEDGE_MODE=internal) unless
# the caller sets CAIRN_DEV_KNOWLEDGE_MODE (e.g. external=/tmp/notes), so the mode chooser never
# blocks a probe.
set -u
probe=$1; shift
here=$(cd "$(dirname "$0")" && pwd)
port=${PROBE_PORT:-5199}
tmp=$(mktemp -d)
{
  echo "const report = (t) => fetch('http://127.0.0.1:$port/' + encodeURIComponent(t), { mode: 'no-cors' });"
  cat "$probe"
} > "$tmp/probe.js"
# A tiny listener that prints each reported text.
python3 -I -c "
import http.server, urllib.parse
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        print(urllib.parse.unquote(self.path[1:]), flush=True)
        self.send_response(204); self.end_headers()
    def log_message(self, *a): pass
http.server.HTTPServer(('127.0.0.1', $port), H).serve_forever()
" &
listener=$!
cd "$here/.."
env CAIRN_DEV_KNOWLEDGE_MODE="${CAIRN_DEV_KNOWLEDGE_MODE:-internal}" "$@" \
  CAIRN_DEV_SCRIPT_FILE="$tmp/probe.js" CAIRN_DEV_SCRIPT_AFTER="${PROBE_AFTER:-10}" \
  CAIRN_EXIT_AFTER_SECS="${PROBE_EXIT:-20}" nice -n 10 ./target/debug/cairn >"$tmp/app.log" 2>&1
sleep 0.5
kill "$listener"
rm -rf "$tmp"
