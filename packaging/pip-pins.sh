# packaging/pip-pins.sh: the sha256 of LiteLLM's wheels, sourced by build-sidecar.sh,
# build-sidecar-windows.sh and scripts/setup-sidecar-dev.sh. LiteLLM's releases on PyPI were hijacked
# twice in 2026 (1.82.7/1.82.8, 1.107.0), so its wheel is checked before it is installed. The other packages
# in sidecar/requirements.lock are pinned by version only: one --hash in the lock would make pip demand
# one on every line. The values are PyPI's own (https://pypi.org/pypi/litellm/1.105.0/json, uploaded
# 2026-10-11) and match the wheels downloaded on 2026-10-16/17. Bump them together with the lock's
# litellm line, and check the new sha256 by hand.
# shellcheck shell=bash

LITELLM_VERSION=1.105.0
LITELLM_WHEEL_LINUX=litellm-1.105.0-cp310-abi3-manylinux_2_28_x86_64.whl
LITELLM_SHA256_LINUX=52b13819212d4beb0fcfaec9cfbd8bd616fade930a3a399acdfb7d959ba4df2b
LITELLM_WHEEL_WIN=litellm-1.105.0-cp310-abi3-win_amd64.whl
LITELLM_SHA256_WIN=6ff237f1fe64a086d91b1453116347e0c1b2aed0fe8c8b9014d5a019c6a6b5d6

# litellm_wheel_ok LOCK DIR WHEEL SHA256: true when LOCK pins LITELLM_VERSION, DIR holds WHEEL with that
# sha256, and DIR has no other wheel of that version (one that pip might prefer). Says what is wrong otherwise.
litellm_wheel_ok() {
    local lock="$1" dir="$2" wheel="$3" sha="$4" locked others
    locked="$(sed -n 's/^litellm==\([^ ;#]*\).*/\1/p' "$lock")"
    if [ "$locked" != "$LITELLM_VERSION" ]; then
        echo "litellm: the lock pins '$locked' but packaging/pip-pins.sh pins $LITELLM_VERSION; update the pins" >&2
        return 1
    fi
    if [ ! -f "$dir/$wheel" ] || [ "$(sha256sum "$dir/$wheel" | cut -d' ' -f1)" != "$sha" ]; then
        echo "litellm: $dir/$wheel is missing or does not match the pinned sha256 ($sha)" >&2
        return 1
    fi
    others="$(find "$dir" -maxdepth 1 -name "litellm-$LITELLM_VERSION-*.whl" ! -name "$wheel" -printf '%f ')"
    if [ -n "$others" ]; then
        echo "litellm: other wheels of $LITELLM_VERSION next to the pinned one in $dir: $others" >&2
        return 1
    fi
}
