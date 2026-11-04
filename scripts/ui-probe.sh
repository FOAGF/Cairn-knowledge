#!/bin/bash
# Runs a JavaScript probe inside the debug build's window and prints what it reports, without
# typing or clicking in the window (the owner's keyboard and focus are never touched).
#
#   CAIRN_DATA_DIR=/tmp/x scripts/ui-probe.sh scripts/probes/dialog-flash.js [VAR=value ...]
#
# The page shown is the devUrl baked into the last build (see capture-window.sh). The probe gets a
# `report(text)` function; each call is printed here. Env:
# - PROBE_AFTER (s before the probe runs, default 10), PROBE_EXIT (s before the app closes,
#   default 20), PROBE_PORT (default 5199).
# - CAIRN_DEV_KNOWLEDGE_MODE: a fresh data folder starts in internal knowledge mode unless the
#   caller sets it (e.g. external=/tmp/notes), so the mode chooser never blocks a probe; `none`
#   leaves it unset, so a fresh data folder shows the full-screen mode chooser (FR-KN-EXT-7).
# - CAIRN_BIN: the binary (default ./target/debug/cairn). Copy a baked build somewhere of your own
#   so a later `cargo build` (which bakes port 5173) can't change it under you.
# - PROBE_OUT: file the reported lines are also written to (default: a temporary file). A driver
#   reads it to follow the probe.
# - PROBE_DRIVER: a bash script run next to the app with APP_PID, PROBE_OUT and the caller's
#   environment; the probe asks it for things by reporting `@@<word>` lines, and the driver waits
#   for them with `wait_for <word>` (see probes/ext-outside-driver.sh). Its output goes to stdout too.
# - PROBE_APPLOG: where the app's stderr goes (default: thrown away with the temporary folder).
set -u
probe=$1; shift
here=$(cd "$(dirname "$0")" && pwd)
port=${PROBE_PORT:-5199}
bin=${CAIRN_BIN:-./target/debug/cairn}
tmp=$(mktemp -d)
listener=""
driver=""
cleanup() {
  [ -n "$driver" ] && kill "$driver" 2>/dev/null
  [ -n "$listener" ] && kill "$listener" 2>/dev/null
  rm -rf "${tmp:?}"
}
trap cleanup EXIT
out=${PROBE_OUT:-$tmp/out.txt}
: > "$out"
{
  echo "const report = (t) => fetch('http://127.0.0.1:$port/' + encodeURIComponent(t), { mode: 'no-cors' });"
  cat "$probe"
} > "$tmp/probe.js"
# A tiny listener that prints each reported text.
python3 -I -c "
import http.server, urllib.parse, sys, time
out = open(sys.argv[1], 'a')
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        line = urllib.parse.unquote(self.path[1:])
        print(line, flush=True)
        out.write('%.3f %s\n' % (time.time(), line)); out.flush()
        self.send_response(204); self.end_headers()
    def log_message(self, *a): pass
http.server.HTTPServer(('127.0.0.1', $port), H).serve_forever()
" "$out" &
listener=$!
# A port already taken (say, by a listener left from an interrupted run) would get the reports.
sleep 0.5
if ! kill -0 "$listener" 2>/dev/null; then
  echo "ui-probe: port $port is in use; stop what holds it or set PROBE_PORT" >&2
  listener=""
  exit 1
fi
cd "$here/.."
mode=${CAIRN_DEV_KNOWLEDGE_MODE:-internal}
envmode=(CAIRN_DEV_KNOWLEDGE_MODE="$mode")
[ "$mode" = none ] && envmode=(-u CAIRN_DEV_KNOWLEDGE_MODE)
env "${envmode[@]}" "$@" \
  CAIRN_DEV_SCRIPT_FILE="$tmp/probe.js" CAIRN_DEV_SCRIPT_AFTER="${PROBE_AFTER:-10}" \
  CAIRN_EXIT_AFTER_SECS="${PROBE_EXIT:-20}" nice -n 10 "$bin" >"${PROBE_APPLOG:-$tmp/app.log}" 2>&1 &
app=$!
if [ -n "${PROBE_DRIVER:-}" ]; then
  APP_PID=$app PROBE_OUT=$out bash "$PROBE_DRIVER" &
  driver=$!
fi
wait "$app"
if [ -n "$driver" ]; then
  sleep 1
  kill "$driver" 2>/dev/null
  wait "$driver" 2>/dev/null
  driver=""
fi
sleep 0.5
