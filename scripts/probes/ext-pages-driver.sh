#!/bin/bash
# D-72 driver for ext-pages.js (PROBE_DRIVER of ext-probe.sh): screenshots, changes made outside
# Cairn, and the files on disk printed when the probe asks. Env: ROOT (the knowledge folder), TRASH
# (CAIRN_DEV_TRASH_DIR), SHOTS (folder for PNGs).
source "$(dirname "$0")/ext-driver-lib.sh"
show() { echo "----- $1"; cat "$ROOT/$1"; echo "----- end"; }
wait_for shot-tree && shot "$SHOTS/B1-tree.png"
wait_for shot-image && shot "$SHOTS/B2-image.png"
wait_for dump-b3 && { show Operations/Overview.md; show "Learning/Rust ownership.md"; }
wait_for outside-seal-1 && { printf 'OUTSIDE-1 written by another program.\n' >> "$ROOT/Operations/Pumps/Seals/Seal replacement.md"; signal_done "appended OUTSIDE-1"; }
wait_for shot-conflict && shot "$SHOTS/B7-conflict.png"
wait_for dump-seal-1 && show "Operations/Pumps/Seals/Seal replacement.md"
wait_for outside-seal-2 && { printf 'OUTSIDE-2 written by another program.\n' >> "$ROOT/Operations/Pumps/Seals/Seal replacement.md"; signal_done "appended OUTSIDE-2"; }
wait_for dump-seal-2 && show "Operations/Pumps/Seals/Seal replacement.md"
wait_for shot-savedas && shot "$SHOTS/B10-saved-as.png"
wait_for dump-rename && {
  ls -la "$ROOT/Operations/Pumps"
  for f in Welcome.md Operations/Overview.md "Operations/Pumps/Seals/Seal replacement.md" "Operations/Hoses/Hose guide.markdown"; do
    echo "links in $f:"; grep -n "Pump" "$ROOT/$f"
  done
  show "$(cd "$ROOT" && ls Operations/Pumps/*.md | head -1)"
}
wait_for dump-trash && { echo "trash:"; ls -laR "$TRASH"; echo "Hoses folder:"; ls -la "$ROOT/Operations/Hoses"; }
