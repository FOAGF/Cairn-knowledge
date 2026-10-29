#!/bin/bash
# D-72 driver for ext-more.js. Before the app's window is up (Cairn has just been started, its
# start-up scan comes 3 s after the window opens) it changes the folder as if while Cairn was closed:
# an edit to Welcome.md, a new Learning/Written while closed.md, Learning/Old notes.md removed. Then
# prints files when asked. Env: ROOT, SHOTS.
source "$(dirname "$0")/ext-driver-lib.sh"
printf '\nCLOSED-EDIT made while Cairn was closed.\n' >> "$ROOT/Welcome.md"
printf '# Written while closed\n\nHello.\n' > "$ROOT/Learning/Written while closed.md"
rm -f "$ROOT/Learning/Old notes.md"
signal_done "changed the folder before the scan"
wait_for shot-title && shot "$SHOTS/M1-title-line.png"
wait_for dump-archive && { echo "----- Learning/中文笔记.md"; cat "$ROOT/Learning/中文笔记.md"; echo "----- end"; }
wait_for dump-images-1 && { ls -la "$ROOT/Operations/attachments" "$ROOT/Operations/diagrams"; tail -3 "$ROOT/Operations/Overview.md"; }
wait_for dump-images-2 && { ls -la "$ROOT/Operations/attachments" "$ROOT/Operations/diagrams"; tail -3 "$ROOT/Operations/Overview.md"; ls -la "$ROOT/General" "$ROOT/Operations/Pumps"; }
