#!/usr/bin/env bash
# packaging/publish.sh (`make publish`, NFR-INS-1, D-80, D-84, D-87): makes a draft GitHub release from the
# packages in build/. The maintainer looks it over on GitHub and publishes it there; nothing here publishes.
#
#   packaging/publish.sh               the checks, then (after asking) the draft: made, or updated in place
#   packaging/publish.sh check         only the checks that need no network
#   packaging/publish.sh notes [LAST]  print the release notes; LAST is the last published version, or
#                                      `none` for the first release (default: asked from GitHub)
#
# Uploads build/Cairn_<v>_amd64.deb, Cairn-lite_<v>_amd64.deb, Cairn-<v>-windows-x64.zip,
# Cairn-<v>-windows-x64-lite.zip and SHA256SUMS (written here). Never the AppImage (D-80).
# Refuses when: the version numbers disagree, or the version table has no row for this version; a file is
# missing or older than the last commit; the working tree has changes; the commit is not on the `origin`
# remote; a published release already has this version's tag.
# The draft: tag v<version> on the current commit, in the GitHub repository of `origin` (CAIRN_PUBLISH_REPO=
# owner/name overrides it, e.g. for an SSH host alias), a pre-release while the version is 0.x. A draft of
# the same version (a rehearsal) is updated in place: notes rewritten, files replaced.
# Notes: the first release gets README's introduction; a later one the version table's rows
# (docs/requirements.md §9.4) since the last published release, newest first. Then README's download
# section for this version and how to check a file against SHA256SUMS.
# Needs the GitHub command line tool, signed in: sudo apt install gh && gh auth login.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
cd "$REPO"

BUILD="$REPO/build"
VERSION="$(python3 -I -c 'import json;print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
TAG="v$VERSION"
FILES=("Cairn_${VERSION}_amd64.deb" "Cairn-lite_${VERSION}_amd64.deb"
       "Cairn-${VERSION}-windows-x64.zip" "Cairn-${VERSION}-windows-x64-lite.zip")
MAKES=("make release-deb" "make release-deb-lite" "make release-windows" "make release-windows-lite")
NOTES="$BUILD/release-notes-$VERSION.md"
log() { echo "[publish] $*"; }

ask() {   # ask "question" -> 0 for yes; anything but y/yes is no
    local answer=""
    read -r -p "$1 [y/N] " answer || true
    case "$answer" in y|Y|yes|YES|Yes) return 0 ;; *) return 1 ;; esac
}

step_check() {
    local problems=() i f cargo app since rebuild=false
    cargo="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n 1)"
    app="$(python3 -I -c 'import json;print(json.load(open("app/package.json"))["version"])')"
    if [ "$cargo" != "$VERSION" ] || [ "$app" != "$VERSION" ]; then
        problems+=("the versions disagree: src-tauri/tauri.conf.json $VERSION, Cargo.toml $cargo, app/package.json $app")
    fi
    grep -q "^| $VERSION |" docs/requirements.md \
        || problems+=("docs/requirements.md §9.4 has no row for $VERSION (every release adds one; the notes come from it)")
    [ -z "$(git status --porcelain)" ] || problems+=("the working tree has changes: commit them, then build the packages again")
    # A package built before the last commit may hold other code than the tag it is published under.
    since="$(git log -1 --format=%ct)"
    for i in "${!FILES[@]}"; do
        f="$BUILD/${FILES[$i]}"
        if [ ! -f "$f" ]; then
            problems+=("${FILES[$i]} is missing: ${MAKES[$i]}"); rebuild=true
        elif [ "$(stat -c %Y "$f")" -lt "$since" ]; then
            problems+=("${FILES[$i]} is older than the last commit: ${MAKES[$i]}"); rebuild=true
        fi
    done
    if [ ${#problems[@]} -gt 0 ]; then
        log "not ready to publish $VERSION:"
        for f in "${problems[@]}"; do log "  - $f"; done
        [ "$rebuild" = false ] || log "all four packages in one command: make release-for-publish"
        exit 1
    fi
    log "check OK: $VERSION, commit $(git rev-parse --short HEAD), ${#FILES[@]} packages in build/"
}

# GH_REPO: the GitHub repository (owner/name) of `origin`, after checking that the commit is there.
step_remote() {
    local url
    url="$(git remote get-url origin 2>/dev/null)" \
        || { log "this clone has no origin remote: add the public repository (git remote add origin …) and push"; exit 1; }
    GH_REPO="${CAIRN_PUBLISH_REPO:-$(printf '%s\n' "$url" \
        | sed -E -n 's#^(https://github\.com/|git@github\.com:|ssh://git@github\.com/)##p' | sed -E 's#(\.git)?/?$##')}"
    [[ "$GH_REPO" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] \
        || { log "origin ($url) is not a GitHub repository; set CAIRN_PUBLISH_REPO=owner/name"; exit 1; }
    command -v gh >/dev/null 2>&1 || { log "the GitHub command line tool is missing: sudo apt install gh, then gh auth login"; exit 1; }
    gh auth status >/dev/null 2>&1 || { log "the GitHub command line tool is not signed in: gh auth login"; exit 1; }
    git fetch --quiet origin
    git branch -r --contains HEAD --format='%(refname:short)' | grep -q '^origin/' \
        || { log "commit $(git rev-parse --short HEAD) is not on origin: push it first"; exit 1; }
}

# The newest published release (drafts don't count) older than this version, or `none`.
last_published() {
    gh release list --repo "$GH_REPO" --exclude-drafts --limit 1000 --json tagName --jq '.[].tagName' \
        | python3 -I -c '
import re, sys
current = tuple(map(int, sys.argv[1].split(".")))
older = [tuple(map(int, m.group(1).split("."))) for t in sys.stdin.read().split()
         if (m := re.fullmatch(r"v(\d+\.\d+\.\d+)", t)) and tuple(map(int, m.group(1).split("."))) < current]
print(".".join(map(str, max(older))) if older else "none")' "$VERSION"
}

# step_notes LAST OUT: the notes for this version (D-87), LAST being the last published version or `none`.
step_notes() {
    python3 -I - "$REPO" "$VERSION" "$1" "$2" <<'PY'
import re, sys
from pathlib import Path
repo, version, last, out = Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4]
readme = (repo / "README.md").read_text(encoding="utf-8")
spec = (repo / "docs" / "requirements.md").read_text(encoding="utf-8")
num = lambda v: tuple(map(int, v.split(".")))

def section(text, start, end):
    """The text from the line `start` up to (not including) the next line `end`."""
    i = text.index(start)
    j = text.find(end, i + len(start))
    return text[i:j if j >= 0 else len(text)].strip()

# Requirement and decision IDs in parentheses mean nothing outside the spec: "(D-83)", "(FR-KN-ED-8)",
# "(NFR-INS-1b, D-82)", "(D-62 to D-64)".
ID = r"[A-Z]{1,4}(?:-[A-Z0-9]+)*-\d+[a-z]?"
ids = re.compile(rf"\s*\(\s*{ID}(?:\s*(?:,|to|and)\s*{ID})*\s*\)")

parts = []
if last == "none":
    # The first release: what Cairn is (README up to its download section).
    parts.append(section(readme, "Cairn is ", "\n## Download"))
else:
    rows = []
    table = section(spec, "### 9.4 Versions", "\n## ")
    for line in table.splitlines():
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", line)]
        if len(cells) >= 5 and re.fullmatch(r"\d+\.\d+\.\d+", cells[1]):
            if num(last) < num(cells[1]) <= num(version):
                rows.append((num(cells[1]), cells[1], ids.sub("", cells[2]), ids.sub("", cells[3])))
    if not rows:
        sys.exit(f"no row in the version table between {last} and {version}")
    for _, v, title, content in sorted(rows, reverse=True):
        parts.append(f"## {v}: {title}\n\n{content}")

# Every release: the files and how to install them (README's download section, for this version).
download = section(readme, "## Download", "\n## First start")
download = "\n".join(l for l in download.splitlines() if "Releases page" not in l)
parts.append(download.replace("<version>", version).strip())
parts.append("To check a download, put it next to `SHA256SUMS` and run `sha256sum -c --ignore-missing SHA256SUMS`.")
Path(out).write_text("\n\n".join(re.sub(r"\n{3,}", "\n\n", p) for p in parts) + "\n", encoding="utf-8")
PY
}

step_publish() {
    step_check
    if [ ! -t 0 ]; then
        log "this needs a terminal: it asks before it uploads anything, and nothing can answer for you."
        exit 1
    fi
    step_remote
    local state err last mode paths=() f pre=() prerelease=false head
    head="$(git rev-parse HEAD)"
    err="$(mktemp)"
    if state="$(gh release view "$TAG" --repo "$GH_REPO" --json isDraft --jq .isDraft 2>"$err")"; then
        [ "$state" = true ] || { rm -f "$err"; log "$TAG is already published on $GH_REPO; a published release is never changed here"; exit 1; }
        mode=update
    elif grep -qi 'not found' "$err"; then
        mode=create
    else
        cat "$err" >&2; rm -f "$err"; log "could not ask GitHub about $TAG"; exit 1
    fi
    rm -f "$err"
    last="$(last_published)"
    step_notes "$last" "$NOTES"
    (cd "$BUILD" && sha256sum "${FILES[@]}" > SHA256SUMS)
    for f in "${FILES[@]}" SHA256SUMS; do paths+=("$BUILD/$f"); done
    if [[ "$VERSION" == 0.* ]]; then pre=(--prerelease); prerelease=true; fi

    echo
    cat "$NOTES"
    echo
    log "repository: $GH_REPO"
    log "release:    $TAG on commit ${head:0:12}, $([ "$mode" = create ] && echo "a new draft" || echo "updating the existing draft")$([ "$prerelease" = true ] && echo ", pre-release")"
    log "notes:      above ($NOTES); $([ "$last" = none ] && echo "the first release" || echo "versions since $last")"
    for f in "${paths[@]}"; do log "  $(du -h "$f" | cut -f1)  $(basename "$f")"; done
    ask "Upload these to GitHub as a draft release?" || { log "nothing uploaded"; exit 1; }

    if [ "$mode" = create ]; then
        gh release create "$TAG" --repo "$GH_REPO" --draft "${pre[@]}" --target "$head" \
            --title "Cairn $VERSION" --notes-file "$NOTES" "${paths[@]}"
    else
        gh release edit "$TAG" --repo "$GH_REPO" --draft --prerelease="$prerelease" --target "$head" \
            --title "Cairn $VERSION" --notes-file "$NOTES"
        gh release upload "$TAG" --repo "$GH_REPO" --clobber "${paths[@]}"
    fi
    log "draft ready: $(gh release view "$TAG" --repo "$GH_REPO" --json url --jq .url)"
    log "look it over on GitHub and publish it there; the tag $TAG is made when it is published"
}

case "${1:-all}" in
    all)   step_publish ;;
    check) step_check ;;
    notes)
        last="${2:-}"
        if [ -z "$last" ]; then step_remote; last="$(last_published)"; fi
        out="$(mktemp)"; step_notes "$last" "$out"; cat "$out"; rm -f "$out" ;;
    -h|--help|help) sed -n '2,21p' "${BASH_SOURCE[0]}" ;;
    *) echo "unknown step '$1'; try: all check notes" >&2; exit 2 ;;
esac
