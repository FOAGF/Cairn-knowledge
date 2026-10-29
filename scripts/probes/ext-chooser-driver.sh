#!/bin/bash
# D-72 driver for ext-chooser.js (run by ext-probe.sh as PROBE_DRIVER): screenshots of the mode
# chooser when the probe asks. SHOTS: the folder for the PNGs.
source "$(dirname "$0")/ext-driver-lib.sh"
wait_for shot-chooser 60 && shot "$SHOTS/A-chooser.png"
wait_for shot-chooser-ready 60 && shot "$SHOTS/A-chooser-ready.png"
