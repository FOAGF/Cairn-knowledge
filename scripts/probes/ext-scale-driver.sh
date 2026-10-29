#!/bin/bash
# D-72 driver for ext-scale.js: notes when the app started and samples its CPU and memory for the
# whole run (CPU: the file for `sample_cpu` lines).
source "$(dirname "$0")/ext-driver-lib.sh"
signal_done "app started (pid $APP_PID)"
sample_cpu 400 "$CPU"
