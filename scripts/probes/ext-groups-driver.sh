#!/bin/bash
# D-72 driver for ext-groups.js (PROBE_DRIVER of ext-probe.sh): prints the folder when the probe
# asks, puts an image into Operations/Pumps, and takes the clash screenshot. Env: ROOT, SHOTS.
source "$(dirname "$0")/ext-driver-lib.sh"
tree_of() { echo "----- files under $1:"; (cd "$ROOT" && find "$1" | sort); }
wait_for shot-clash && shot "$SHOTS/B8-clash.png"
wait_for dump-b8 && { tree_of Learning; tree_of Operations/Pumps; }
wait_for dump-fittings && tree_of Operations/Hoses
wait_for dump-move && { tree_of Operations; echo "links to Seal replacement:"; grep -rn "Seal" "$ROOT/Operations" "$ROOT/Welcome.md" | sed "s|$ROOT/||"; }
wait_for dump-delete-hoses && { tree_of Operations; echo "links to Seal replacement:"; grep -rn "Seal%20\|Seal replacement" "$ROOT" | sed "s|$ROOT/||"; }
wait_for put-image && { cp "$ROOT/Operations/attachments/pump.png" "$ROOT/Operations/Pumps/photo.png"; signal_done "image in Pumps"; }
wait_for dump-delete-pumps && tree_of Operations
