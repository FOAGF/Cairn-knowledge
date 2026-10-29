#!/bin/bash
# D-72 driver for ext-recheck.js (PROBE_DRIVER of ext-probe.sh): outside edits, files printed when
# asked, and screenshots. Env: ROOT, SHOTS.
source "$(dirname "$0")/ext-driver-lib.sh"
wait_for shot-card && shot "$SHOTS/R7-storage-card.png"
wait_for outside-welcome && { printf '\nRESCAN-EDIT made by another program.\n' >> "$ROOT/Welcome.md"; signal_done "edited Welcome.md"; }
wait_for outside-pump && { printf '\nOUTSIDE written by another program.\n' >> "$ROOT/Operations/Pumps/Pump check.md"; signal_done "edited Pump check.md"; }
wait_for shot-conflict && shot "$SHOTS/R3-conflict.png"
wait_for dump-pump && { ls "$ROOT/Operations/Pumps"; echo "----- Pump- check-.md"; cat "$ROOT/Operations/Pumps/Pump- check-.md"; echo "----- end"; }
wait_for shot-title && shot "$SHOTS/R1-title.png"
wait_for dump-doc && { ls "$ROOT/General"; echo "----- General/Doc- one-.md"; cat "$ROOT/General/Doc- one-.md"; echo; echo "----- end"; }
