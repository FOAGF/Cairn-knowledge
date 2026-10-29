#!/bin/bash
# D-72 driver for ext-switch-warning.js: screenshots of the start-up pop-up and the list. Env: SHOTS.
source "$(dirname "$0")/ext-driver-lib.sh"
wait_for shot-popup && shot "$SHOTS/C-popup.png"
wait_for shot-list && shot "$SHOTS/C-backups-list.png"
