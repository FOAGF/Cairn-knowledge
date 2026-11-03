#!/usr/bin/env bash
# packaging/version.sh (D-90): prints the version a build carries. The release scripts name their files with
# it and build it into the app (Settings › About, the manifest of exports and backups).
#
#   on a commit tagged v<x.y.z>                 x.y.z
#   on a later commit                           x.y.z-<commits since that tag>-g<short commit>  (git describe)
#   tracked files changed but not committed     the same with -dirty at the end
#   without git or a v tag (a source download)  the version in src-tauri/tauri.conf.json
#
# src-tauri/build.rs works out the same for builds that don't come from these scripts.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if described="$(git -C "$REPO" describe --tags --match 'v[0-9]*' --dirty 2>/dev/null)"; then
    echo "${described#v}"
else
    python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$REPO/src-tauri/tauri.conf.json"
fi
