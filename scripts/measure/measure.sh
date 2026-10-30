#!/usr/bin/env bash
# scripts/measure/measure.sh (M7, NFR-PERF-6, D-51): measures the AppImage and appends a dated
# section to docs/measurements.md. Run it only when the machine is free: it needs a screen, runs
# the app four times on full-scale data and loads the CPU for several minutes.
#
#   scripts/measure/measure.sh [APPIMAGE] [--pages N] [--tasks N] [--mock-port P] [--out FILE|-]
#                              [--cpu-secs S] [--rounds R] [--keep]
#
# APPIMAGE defaults to the newest full one in build/ (a plain
# binary also works, for testing). Everything else goes to measure.py (`measure.py --help`).
# MEASURE_DATA_BIN=<path> uses an already built measure_data example instead of building the
# release one.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"

target=""
if [ $# -gt 0 ] && [[ "$1" != --* ]]; then
  target=$1
  shift
fi
if [ -z "$target" ]; then
  # The full AppImage: a lite one (NFR-INS-1b) has no search model to measure.
  target=$(ls -t build/*.AppImage 2>/dev/null | grep -v '/Cairn-lite_' | head -n 1 || true)
fi
if [ -z "$target" ] || [ ! -f "$target" ]; then
  echo "measure: no AppImage found. Build one with 'make release', or pass the file as the first argument." >&2
  exit 1
fi

if [ -n "${MEASURE_DATA_BIN:-}" ]; then
  data_bin=$MEASURE_DATA_BIN
else
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  echo "measure: building the data generator (release)..."
  CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" nice -n 10 cargo build --release -p cairn-core --example measure_data
  data_bin=$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -I -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/release/examples/measure_data
fi

echo "measure: $target"
exec nice -n 5 python3 -I scripts/measure/measure.py --target "$target" --data-bin "$data_bin" "$@"
