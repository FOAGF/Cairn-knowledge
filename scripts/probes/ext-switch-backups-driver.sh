#!/bin/bash
# Driver for ext-switch-backups.js: copies the folder's switch backup into the backups folder under a
# manual backup's name, so the Backup card lists it with *Restore…*; then captures the import dialog.
. "$(dirname "$0")/ext-driver-lib.sh"
wait_for copy || exit 1
src=$(ls -t "$CAIRN_DATA_DIR"/switch-backups/*external*.zip | head -1)
mkdir -p "$CAIRN_DATA_DIR/backups"
cp "$src" "$CAIRN_DATA_DIR/backups/cairn-manual-$(hostname)-2026-10-20-2359.zip"
signal_done "copied $(basename "$src")"
wait_for shot || exit 1
shot "${SHOT_OUT:-/tmp/ext-switch-backups.png}"
signal_done shot
