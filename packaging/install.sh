#!/usr/bin/env bash
# packaging/install.sh (`make install`, `make install-lite`, NFR-INS-1, NFR-INS-1b, D-46, D-82): installs the
# AppImage for the current user.
#
#   packaging/install.sh [AppImage file]          (default: the newest one `make release` built)
#   packaging/install.sh --lite [AppImage file]   (default: the newest one `make release-lite` built)
#
# A lite AppImage (no search model) is installed as ~/Applications/Cairn-lite-<version>.AppImage; a file named
# Cairn-lite_… is taken as lite without --lite. Full and lite share the Cairn.AppImage link, the menu entry and the
# data folder, so the one installed last is the one that starts. Each kind keeps its own newest two.
#
# What it does (everything is in your home folder; it never uses sudo):
#   - copies the AppImage to ~/Applications/Cairn-<version>.AppImage and points
#     ~/Applications/Cairn.AppImage at it. The menu entry uses the fixed name, so a new version
#     replaces the old one without touching the menu entry. The previous version stays as a
#     fallback; older ones are removed.
#   - adds the menu entry (~/.local/share/applications/cairn.desktop) and the icon
#     (~/.local/share/icons/hicolor/*/apps/cairn.png).
#   - checks that libfuse2 is installed (AppImages need it) and prints the apt command if not.
# Your data and settings are not touched: updating means installing the new AppImage over the old one.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/.." && pwd)"
log() { echo "[install] $*"; }

lite=0
if [ "${1:-}" = --lite ]; then
    lite=1
    shift
fi
image="${1:-}"
if [ -z "$image" ]; then
    # newest by version number
    pattern='Cairn_*_amd64.AppImage'
    [ "$lite" = 1 ] && pattern='Cairn-lite_*_amd64.AppImage'
    # shellcheck disable=SC2086
    image="$(ls "$REPO"/build/$pattern 2>/dev/null | sort -V | tail -n 1 || true)"
fi
if [ -z "$image" ] || [ ! -f "$image" ]; then
    build='make release'
    [ "$lite" = 1 ] && build='make release-lite'
    log "no AppImage found. Build one with '$build', or pass the file: packaging/install.sh $([ "$lite" = 1 ] && echo '--lite ')FILE" >&2
    exit 1
fi

base="$(basename "$image")"
if [[ "$base" =~ ^Cairn-lite_(.+)_amd64\.AppImage$ ]]; then
    lite=1
    version="${BASH_REMATCH[1]}"
elif [[ "$base" =~ ^Cairn_(.+)_amd64\.AppImage$ ]]; then
    version="${BASH_REMATCH[1]}"
else
    version="$(date -r "$image" +%Y%m%d%H%M%S)"   # a file with another name: keep it apart by its time
fi

apps="$HOME/Applications"
share="${XDG_DATA_HOME:-$HOME/.local/share}"
kind="Cairn"
[ "$lite" = 1 ] && kind="Cairn-lite"
target="$apps/$kind-$version.AppImage"
link="$apps/Cairn.AppImage"

mkdir -p "$apps" "$share/applications"
log "copying $base to $target"
install -m 755 "$image" "$target.part"
mv -f "$target.part" "$target"
ln -sfn "$(basename "$target")" "$link"

# Keep this version and the newest other one of the same kind (full or lite); remove the rest. The file just
# installed is never removed, even when it is older than the others.
if [ "$lite" = 1 ]; then
    mapfile -t others < <(ls "$apps"/Cairn-lite-*.AppImage 2>/dev/null | grep -vxF "$target" | sort -V)
else
    mapfile -t others < <(ls "$apps"/Cairn-[0-9]*.AppImage 2>/dev/null | grep -vxF "$target" | sort -V)
fi
if [ "${#others[@]}" -gt 1 ]; then
    for old in "${others[@]:0:${#others[@]}-1}"; do
        log "removing the old version $(basename "$old")"
        rm -f "$old"
    done
fi

# Icon: two sizes of the app icon
for size in 128 256; do
    src="$REPO/src-tauri/icons/128x128.png"
    [ "$size" = 256 ] && src="$REPO/src-tauri/icons/128x128@2x.png"
    if [ -f "$src" ]; then
        mkdir -p "$share/icons/hicolor/${size}x${size}/apps"
        install -m 644 "$src" "$share/icons/hicolor/${size}x${size}/apps/cairn.png"
    fi
done

# Menu entry. Exec needs the path quoted (a home folder may contain spaces).
desktop="$share/applications/cairn.desktop"
cat > "$desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Cairn
Comment=Notes and tasks, with an assistant
Exec="$link"
Icon=cairn
Terminal=false
Categories=Office;
EOF
chmod 644 "$desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$share/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -f -t "$share/icons/hicolor" >/dev/null 2>&1 || true

if [ "$lite" = 1 ]; then
    log "installed Cairn $version (lite, no search model; add it in Settings > Search): $link"
else
    log "installed Cairn $version: $link"
fi
log "menu entry: $desktop"

# libfuse2: an AppImage cannot start without it. (Ubuntu 24.04 names the package libfuse2t64.)
if ldconfig -p 2>/dev/null | grep -q 'libfuse\.so\.2' || dpkg -s libfuse2t64 >/dev/null 2>&1 || dpkg -s libfuse2 >/dev/null 2>&1; then
    log "libfuse2: installed"
else
    log "libfuse2 is NOT installed, so Cairn will not start yet. Install it with:"
    log "    sudo apt install libfuse2t64     (Ubuntu 22.04 and older: sudo apt install libfuse2)"
fi
