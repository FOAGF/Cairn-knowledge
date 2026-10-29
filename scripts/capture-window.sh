#!/bin/bash
# Screenshot of a test run of Cairn, without touching any other window (the owner may have their own
# Cairn open). Starts the debug build, finds the window by the process ID, captures it, waits for exit.
#
#   CAIRN_DATA_DIR=/tmp/x scripts/capture-window.sh out.png [VAR=value ...]
#
# A fresh data folder starts in internal knowledge mode unless the caller sets
# CAIRN_DEV_KNOWLEDGE_MODE (e.g. external=/tmp/notes), so the mode chooser never blocks a capture.
#
# The page shown is the devUrl baked into the last build. Bake one first, e.g. with your own Vite on 5190:
#   CAIRN_DATA_DIR=/tmp/x CAIRN_EXIT_AFTER_SECS=2 cargo tauri dev --no-watch \
#     --config '{"build":{"devUrl":"http://localhost:5190/knowledge"}}'
set -u
out=$1; shift
here=$(cd "$(dirname "$0")" && pwd)
cd "$here/.."
env CAIRN_EXIT_AFTER_SECS="${CAIRN_EXIT_AFTER_SECS:-14}" \
  CAIRN_DEV_KNOWLEDGE_MODE="${CAIRN_DEV_KNOWLEDGE_MODE:-internal}" "$@" \
  nice -n 10 ./target/debug/cairn >"${out%.png}.log" 2>&1 &
pid=$!
sleep "${CAPTURE_AFTER_SECS:-10}"
win=""
for id in $(xwininfo -root -tree | awk '/\("cairn" "Cairn"\)/ {print $1}'); do
  p=$(xprop -id "$id" _NET_WM_PID 2>/dev/null | awk '{print $3}')
  w=$(xwininfo -id "$id" | awk '/Width:/ {print $2}')
  [ "$p" = "$pid" ] && [ "${w:-0}" -gt 100 ] && win=$id
done
if [ -n "$win" ]; then
  xwd -silent -id "$win" -out "${out%.png}.xwd" && python3 -I "$here/xwd2png.py" "${out%.png}.xwd" >/dev/null && rm -f "${out%.png}.xwd"
fi
echo "pid=$pid window=${win:-none} -> $out"
wait "$pid"
