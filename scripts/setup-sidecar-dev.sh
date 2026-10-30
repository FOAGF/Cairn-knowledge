#!/usr/bin/env bash
# Sets up the sidecar development environment (idempotent). Run it as:
#   nice -n 10 scripts/setup-sidecar-dev.sh
# Creates sidecar/.venv, installs the pinned requirements and the package (editable),
# and links the embedding model into sidecar/models/embeddinggemma-300m/.
# CAIRN_EMBED_MODEL_SRC: folder holding onnx/model_q4.onnx{,_data} and tokenizer.json
#   (default: build/cache/embeddinggemma-300m, which packaging/fetch-model.sh fills).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SIDECAR="$REPO/sidecar"
VENV="$SIDECAR/.venv"
PY="${PYTHON:-python3}"

if [ ! -x "$VENV/bin/python" ]; then
    echo "Creating $VENV with $($PY --version)"
    "$PY" -m venv "$VENV"
fi

# LiteLLM first, from its sha256-checked wheel (packaging/pip-pins.sh), so the lock's line below is
# already satisfied. The wheel is the release build's copy (build/cache/wheels), downloaded if missing.
# shellcheck source=packaging/pip-pins.sh
. "$REPO/packaging/pip-pins.sh"
WHEELS="$REPO/build/cache/wheels"
if [ ! -f "$WHEELS/$LITELLM_WHEEL_LINUX" ]; then
    "$VENV/bin/pip" download --quiet --disable-pip-version-check --only-binary=:all: --no-deps \
        "litellm==$LITELLM_VERSION" -d "$WHEELS"
fi
litellm_wheel_ok "$SIDECAR/requirements.lock" "$WHEELS" "$LITELLM_WHEEL_LINUX" "$LITELLM_SHA256_LINUX" || exit 1
"$VENV/bin/pip" install --quiet --disable-pip-version-check --no-deps "$WHEELS/$LITELLM_WHEEL_LINUX"

# --no-deps: the lock is complete, and the resolver backtracks for a long time otherwise (M0-d)
"$VENV/bin/pip" install --quiet --disable-pip-version-check --no-deps -r "$SIDECAR/requirements.lock"
"$VENV/bin/pip" install --quiet --disable-pip-version-check --no-deps -r "$SIDECAR/requirements-dev.txt"
"$VENV/bin/pip" install --quiet --disable-pip-version-check --no-deps -e "$SIDECAR"
echo "Python packages: OK"

# Embedding model: symlinks, nothing is copied
# shellcheck source=packaging/model-pins.sh
. "$REPO/packaging/model-pins.sh"
if [ -n "${CAIRN_EMBED_MODEL_SRC:-}" ]; then
    SRC="$CAIRN_EMBED_MODEL_SRC"
else
    SRC="$MODEL_CACHE"
fi
DST="$SIDECAR/models/embeddinggemma-300m"
FILES=(onnx/model_q4.onnx onnx/model_q4.onnx_data tokenizer.json)
missing=0
for f in "${FILES[@]}"; do
    [ -f "$SRC/$f" ] || { echo "MISSING embedding model file: $SRC/$f"; missing=1; }
done
# The folder is made only with all files: the Rust tests take an existing folder as a set-up model.
if [ "$missing" = 0 ]; then
    mkdir -p "$DST/onnx"
    for f in "${FILES[@]}"; do ln -sfn "$SRC/$f" "$DST/$f"; done
fi
if [ "$missing" = 1 ]; then
    echo "Embedding model: NOT linked. Download it with packaging/fetch-model.sh (about 200 MB) and run"
    echo "this again, or set CAIRN_EMBED_MODEL_SRC to a folder that holds the files. The embed tests will be skipped."
else
    echo "Embedding model: linked into $DST"
fi
echo "Done. Run the tests with: nice -n 10 $VENV/bin/python -m pytest $SIDECAR/tests -q"
