#!/usr/bin/env bash
# packaging/fetch-model.sh (FR-RAG-EM-1): downloads the embedding model, EmbeddingGemma-300m as q4 ONNX
# (3 files, about 200 MB), from Hugging Face and checks every file against its pinned sha256
# (packaging/model-pins.sh, which also pins the revision). The repository is public: no account or
# token. Files that are already there and match are kept, so a second run downloads nothing.
# The model is under the Gemma Terms of Use (packaging/licenses/GEMMA_TERMS_OF_USE.txt).
#
#   packaging/fetch-model.sh [FOLDER]    default: build/cache/embeddinggemma-300m
#
# build-model.sh runs it when no other copy of the model is found.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=packaging/model-pins.sh
. "$HERE/model-pins.sh"
DST="${1:-$MODEL_CACHE}"
log() { echo "[model] $*"; }
sha_of() { sha256sum "$1" | cut -d' ' -f1; }

mkdir -p "$DST/onnx"
while read -r sha f; do
    if [ -f "$DST/$f" ] && [ "$(sha_of "$DST/$f")" = "$sha" ]; then
        continue
    fi
    log "ONLINE STEP: downloading $f from huggingface.co ($MODEL_REPO, revision ${MODEL_REV:0:12})"
    curl -fL --retry 3 -o "$DST/$f.part" "https://huggingface.co/$MODEL_REPO/resolve/$MODEL_REV/$f"
    if [ "$(sha_of "$DST/$f.part")" != "$sha" ]; then
        log "$f does not match its pinned sha256 ($sha); removing it"
        rm -f "$DST/$f.part"
        exit 1
    fi
    mv "$DST/$f.part" "$DST/$f"
done <<< "$MODEL_PINS"
log "model files OK in $DST ($(du -sh "$DST" | cut -f1))"
