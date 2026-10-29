# packaging/model-pins.sh: the embedding model's pins (FR-RAG-EM-1), sourced by build-model.sh,
# fetch-model.sh and scripts/setup-sidecar-dev.sh. EmbeddingGemma-300m, q4 ONNX: the files at the
# pinned Hugging Face revision. Bump all of them together.
# shellcheck shell=bash

MODEL_REPO=onnx-community/embeddinggemma-300m-ONNX
MODEL_REV=5090578d9565bb06545b4552f76e6bc2c93e4a66
MODEL_PINS="ad1dfee81a70f7944b9b9d1cc6e48075b832881cf33fab2f2b248be78f3f0043  onnx/model_q4.onnx
599962c3143b040de2dd05e5975be3e9091dd067cacc6a8f7186e3203bab9e02  onnx/model_q4.onnx_data
4dda02faaf32bc91031dc8c88457ac272b00c1016cc679757d1c441b248b9c47  tokenizer.json"
# Each file's size in bytes. The app reads this file too (`cairn-core/src/search_model.rs`, D-82): it
# checks a model added later against these sizes and the sha256 above, and downloads it from
# MODEL_REPO at MODEL_REV.
MODEL_SIZES="519322  onnx/model_q4.onnx
196725760  onnx/model_q4.onnx_data
20323312  tokenizer.json"

# Where packaging/fetch-model.sh puts the files (git ignores packaging/cache).
MODEL_CACHE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/cache/embeddinggemma-300m"

# True when FOLDER holds every pinned file (their sha256 is checked where they are used).
model_files_in() {
    local f
    while read -r _ f; do
        [ -f "$1/$f" ] || return 1
    done <<< "$MODEL_PINS"
}
