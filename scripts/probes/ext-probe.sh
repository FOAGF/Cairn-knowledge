#!/bin/bash
# D-72 checks: runs a JavaScript probe like scripts/ui-probe.sh, with what the external-mode probes
# need on top. Usage:
#
#   CAIRN_DATA_DIR=/tmp/x scripts/probes/ext-probe.sh probe.js [VAR=value ...]
#
# - CAIRN_BIN: the binary (default ./target/debug/cairn). Copy a baked build somewhere of your own
#   so a later `cargo build` (which bakes port 5173) can't change it under you.
# - CAIRN_DEV_KNOWLEDGE_MODE: as in ui-probe.sh (default internal); `none` leaves it unset, so a
#   fresh data folder shows the full-screen mode chooser (FR-KN-EXT-7).
# - PROBE_OUT: file the reported lines are also written to (default: a temporary file). A driver
#   reads it to follow the probe.
# - PROBE_DRIVER: a bash script run next to the app with APP_PID, PROBE_OUT and the caller's
#   environment; the probe asks it for things by reporting `@@<word>` lines, and the driver waits
#   for them with `wait_for <word>` (see ext-outside-driver.sh). Its output goes to stdout too.
# - PROBE_APPLOG: where the app's stderr goes (default: thrown away with the temporary folder).
# - PROBE_AFTER, PROBE_EXIT, PROBE_PORT: as in ui-probe.sh.
set -u
probe=$1; shift
here=$(cd "$(dirname "$0")/.." && pwd)
port=${PROBE_PORT:-5199}
bin=${CAIRN_BIN:-./target/debug/cairn}
tmp=$(mktemp -d)
out=${PROBE_OUT:-$tmp/out.txt}
: > "$out"
{
  echo "const report = (t) => fetch('http://127.0.0.1:$port/' + encodeURIComponent(t), { mode: 'no-cors' });"
  cat "$probe"
} > "$tmp/probe.js"
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
cd "$here/.."
mode=${CAIRN_DEV_KNOWLEDGE_MODE:-internal}
envmode=(CAIRN_DEV_KNOWLEDGE_MODE="$mode")
[ "$mode" = none ] && envmode=(-u CAIRN_DEV_KNOWLEDGE_MODE)
env "${envmode[@]}" "$@" \
  CAIRN_DEV_SCRIPT_FILE="$tmp/probe.js" CAIRN_DEV_SCRIPT_AFTER="${PROBE_AFTER:-10}" \
  CAIRN_EXIT_AFTER_SECS="${PROBE_EXIT:-20}" nice -n 10 "$bin" >"${PROBE_APPLOG:-$tmp/app.log}" 2>&1 &
app=$!
driver=""
if [ -n "${PROBE_DRIVER:-}" ]; then
  APP_PID=$app PROBE_OUT=$out bash "$PROBE_DRIVER" &
  driver=$!
fi
wait "$app"
[ -n "$driver" ] && { sleep 1; kill "$driver" 2>/dev/null; wait "$driver" 2>/dev/null; }
sleep 0.5
kill "$listener"
rm -rf "${tmp:?}"
