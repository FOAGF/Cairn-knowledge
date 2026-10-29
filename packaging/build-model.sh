#!/usr/bin/env bash
# packaging/build-model.sh (NFR-INS-1): copies the embedding model into the release bundle and
# checks every file against its pinned sha256. Output: packaging/build/models/embeddinggemma-300m/
# (the app finds it at `<resources>/models/embeddinggemma-300m`).
#
# Where the files come from, first match: CAIRN_EMBED_MODEL_SRC (a folder that holds
# onnx/model_q4.onnx{,_data} and tokenizer.json); else packaging/cache/embeddinggemma-300m, which packaging/fetch-model.sh fills (ONLINE STEP,
# about 200 MB, only when the files are not there yet). The pins are in packaging/model-pins.sh.
# Links in the source folder are followed, so the bundle holds real files.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
DST="$HERE/build/models/embeddinggemma-300m"
# shellcheck source=packaging/model-pins.sh
. "$HERE/model-pins.sh"
PINS="$MODEL_PINS"

if [ -n "${CAIRN_EMBED_MODEL_SRC:-}" ]; then
    SRC="$CAIRN_EMBED_MODEL_SRC"
else
    SRC="$MODEL_CACHE"
    model_files_in "$SRC" || "$HERE/fetch-model.sh" "$SRC"
fi
echo "[model] source: $SRC"

missing=0
while read -r _ f; do
    [ -f "$SRC/$f" ] || { echo "[model] MISSING: $SRC/$f" >&2; missing=1; }
done <<< "$PINS"
if [ "$missing" = 1 ]; then
    echo "[model] Set CAIRN_EMBED_MODEL_SRC to the folder that holds the model files, or unset it to download them." >&2
    exit 1
fi

rm -rf "$DST"
mkdir -p "$DST/onnx"
while read -r _ f; do
    cp -L "$SRC/$f" "$DST/$f"
done <<< "$PINS"

echo "[model] checking sha256 of the copied files"
(cd "$DST" && echo "$PINS" | sha256sum --check --quiet) || {
    echo "[model] a file does not match its pinned sha256: wrong model download?" >&2
    rm -rf "$DST"
    exit 1
}
echo "[model] done: $(du -sh "$DST" | cut -f1) in $DST"
