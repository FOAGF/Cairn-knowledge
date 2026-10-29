#!/bin/bash
# D-72 driver for ext-switch.js (PROBE_DRIVER of ext-probe.sh): screenshots of the progress dialog,
# the switch-backups folder when asked, and a copy of the backups for the import check
# (ext-import.js). Env: SHOTS, CAIRN_DATA_DIR, KEEP (folder for the copies).
source "$(dirname "$0")/ext-driver-lib.sh"
wait_for shot-progress && shot "$SHOTS/C-progress.png"
wait_for shot-progress-done && shot "$SHOTS/C-progress-done.png"
wait_for list-backups && ls -la "$CAIRN_DATA_DIR/switch-backups"
wait_for copy-backups && { mkdir -p "$KEEP" && cp "$CAIRN_DATA_DIR"/switch-backups/*.zip "$KEEP"/ && ls -la "$KEEP"; }
while [ "$(grep -c " @@list-backups$" "$PROBE_OUT")" -lt 2 ]; do sleep 0.1; done
ls -la "$CAIRN_DATA_DIR/switch-backups"
