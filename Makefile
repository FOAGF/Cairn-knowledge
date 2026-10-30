# Cairn: building, installing and measuring the release (NFR-INS-1, NFR-PERF-6, NFR-SEC-6).
# docs/building.md explains each command. Development itself runs with `cargo tauri dev` (docs/development.md).
#
# The release scripts live in packaging/ so they can move to a separate release repository later. Everything
# they make goes to build/ (D-85): the release files at its top, the staged pieces in build/stage, the
# downloads in build/cache. Cargo still compiles in target/, which development shares.

.PHONY: all help release release-deb release-windows release-lite release-deb-lite release-windows-lite release-for-publish install install-lite audit measure clean publish

BUILD := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))build

# `make` alone: from a fresh copy of the source to the AppImage and the Windows zip (installs the
# missing Cargo tools, the UI packages and the model first; online on a first run).
all:
	packaging/build-all.sh

help:
	@echo "Everything goes to build/: the release files at its top, staged pieces in build/stage, downloads in build/cache."
	@echo
	@echo "make           build everything from a fresh copy: tools, UI packages, model, then the AppImage and the Windows zip"
	@echo "make release   build the AppImage (UI, sidecar bundle, model, NOTICE), then check it once"
	@echo "make release-deb   build the Debian package the same way (uses the system's GTK and WebKit), then check it once"
	@echo "make release-windows   build the Windows zip (cross-compiled on this machine; the owner runs check-windows.ps1 on Windows)"
	@echo "make release-lite   build the lite AppImage, without the search model (Cairn-lite_<version>_amd64.AppImage), then check it once"
	@echo "make release-deb-lite   build the lite Debian package (Cairn-lite_<version>_amd64.deb, still the package cairn), then check it once"
	@echo "make release-windows-lite   build the lite Windows zip (Cairn-<version>-windows-x64-lite.zip)"
	@echo "make release-for-publish   build the four files make publish uploads: both .deb packages and both Windows zips"
	@echo "make install   copy the full AppImage to ~/Applications and add Cairn to the app menu"
	@echo "make install-lite   the same for the lite AppImage (Cairn-lite-<version>.AppImage); the one installed last starts"
	@echo "make clean     empty build/ but keep the downloads in build/cache"
	@echo "make audit     check the Rust, npm and Python packages for known problems (goes online)"
	@echo "make publish   upload the .deb packages and Windows zips in build/ as a draft GitHub release (asks first)"
	@echo "make measure   measure memory, start-up and search on the AppImage (loads the CPU for minutes)"

release:
	packaging/release.sh

release-deb:
	packaging/release.sh deb-all

release-windows:
	packaging/release-windows.sh

# Lite packages: the same, without the search model, which Settings > Search can add later (NFR-INS-1b, D-82).
release-lite:
	packaging/release.sh lite

release-deb-lite:
	packaging/release.sh deb-lite

release-windows-lite:
	packaging/release-windows.sh lite

# The four files `make publish` uploads (D-87), after the same first-run setup as `make`.
release-for-publish:
	packaging/build-all.sh check tools npm model deb deb-lite windows windows-lite

install:
	packaging/install.sh

install-lite:
	packaging/install.sh --lite

# Keeps build/cache: the next build downloads nothing, and the development setup links the model from there.
clean:
	@[ ! -d "$(BUILD)" ] || find "$(BUILD)" -mindepth 1 -maxdepth 1 ! -name cache -print -exec rm -rf {} +

audit:
	packaging/audit.sh

# The maintainer's release (D-80, D-87): a draft on GitHub from the packages in build/; publishing is done there.
publish:
	packaging/publish.sh

measure:
	scripts/measure/measure.sh
