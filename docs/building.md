# Building Cairn from source

How to build the release files yourself, and what each command downloads. How to work on the code is in [development.md](development.md).

## What you need to build

- **Ubuntu 24.04 or newer** (the AppImage is built against glibc 2.39).
- **libfuse2**, which every AppImage needs. On Ubuntu 24.04 the package is called `libfuse2t64`: `sudo apt install libfuse2t64`. (`make install` checks for it and prints this command if it is missing.) The Debian package (`make release-deb`) doesn't need it.
- To build it yourself: Rust 1.90 or newer (through rustup), Node.js 22 or newer (through nvm), Python 3 and these system packages: `sudo apt install build-essential pkg-config curl file xdg-utils libwebkit2gtk-4.1-dev libdbus-1-dev librsvg2-dev libfuse2t64 clang lld llvm` (clang, lld and llvm are for the Windows build). `make` installs the rest itself. The first build needs the network, see below.

## The commands

All commands run from the top of the repository.

**From a fresh copy of the source, one command builds both release files:** `make`.

| Command | What it does |
|---|---|
| `make` | Builds everything from a fresh copy of the source (`packaging/build-all.sh`). It checks the tools and system packages above and prints what to install if one is missing. Then it installs, for your user and only when missing, the Tauri command line tool, `cargo-xwin` and the Rust target for Windows. It runs `npm ci` when `app/node_modules` is missing, and downloads the search model when no copy is found. Last it runs `make release` and `make release-windows`, and prints where the AppImage and the Windows zip are. The first run downloads several GB (most of it the Rust crates and the Windows SDK) and compiles everything twice, so it takes a long time; later runs download nothing. Linux only: `packaging/build-all.sh check tools npm model linux`. |
| `make release` | Builds the AppImage: the UI, the Python sidecar bundle, the search model and the NOTICE file, then the AppImage itself. At the end it starts the AppImage once to check that the sidecar answers and that embedding works, and prints where the file is and how big it is. It loads the machine for several minutes (it runs at low priority). |
| `make release-deb` | Builds a Debian package the same way, from the same compile: `target/release/bundle/deb/Cairn_<version>_amd64.deb`. Unlike the AppImage it carries no copy of GTK and WebKit: it uses the system's, which Ubuntu keeps updated, so it needs no libfuse2 and is smaller. At the end it unpacks the package into a scratch folder next to it, starts it once with the same check as the AppImage, and removes the folder. Install it with `sudo apt install ./target/release/bundle/deb/Cairn_<version>_amd64.deb` (the program goes to `/usr/bin/cairn`, its files to `/usr/lib/Cairn`, and the menu entry is added); remove it with `sudo apt remove cairn`. |
| `make install` | Copies the AppImage to `~/Applications/`, adds Cairn to the app menu with its icon, and checks that libfuse2 is installed. |
| `make install-lite` | The same for the lite AppImage, as `~/Applications/Cairn-lite-<version>.AppImage`. Full and lite share the `Cairn.AppImage` link, the menu entry and your data, so the one installed last is the one that starts; each keeps its own previous version. |
| `make audit` | Checks the Rust, UI and Python libraries for known security problems. It downloads the lists of known problems, so it asks first, every time. |
| `make measure` | Measures memory, start-up time and search speed on the AppImage (it loads the CPU for minutes). The method and the results are in [docs/measurements.md](measurements.md). |
| `make release-windows` | Builds the Windows version on this Linux machine: `packaging/build/windows/Cairn-<version>-windows-x64.zip` (a portable folder: `cairn.exe`, the Windows Python bundle, the model, NOTICE, `README.txt`, `check-windows.ps1`). It cross-compiles with `cargo-xwin` and cannot run the result: you unzip it on Windows and runs `check-windows.ps1` (see [docs/windows-check.md](windows-check.md)). The zip holds no data. Prints the zip's path, size and file count at the end. |
| `make release-lite`, `make release-deb-lite`, `make release-windows-lite` | The same three packages without the search model (NFR-INS-1b): `target/release/bundle/appimage/Cairn-lite_<version>_amd64.AppImage`, `target/release/bundle/deb/Cairn-lite_<version>_amd64.deb` and `packaging/build/windows/Cairn-<version>-windows-x64-lite.zip`, about 150 MB smaller. Search by words works as usual; search by meaning needs the model, which Settings › Search can download or add from a file. Their NOTICE leaves out the Gemma terms, and their self-test expects no model. They sit next to the full files (building one keeps the other), and the lite Debian package is the same package `cairn`, so one replaces the other. `make install` stays on the full AppImage; `make install-lite` installs the lite one. |

`make help` lists them. Every release step can also run alone, for example `packaging/release.sh notice` (`sidecar`, `model`, `notice`, `ui`, `appimage`, `selftest`, `deb`, `selftest-deb`, and for lite `notice-lite`, `appimage-lite`, `selftest-lite`, `deb-lite-only`, `selftest-deb-lite`) or `packaging/release-windows.sh zip` (`sidecar`, `model`, `notice`, `ui`, `exe`, `zip`, and for lite `notice-lite`, `zip-lite`).

### Building: what goes online

- **Python** (the first `make release` only): the python-build-standalone tarball (about 34 MB) from github.com. If a copy is in `packaging/cache/`, nothing is downloaded. The tarball is checked against a pinned sha256.
- **Python packages** (the first `make release`, and after `sidecar/requirements.lock` changes): the wheels listed in the lock file, from pypi.org, into `packaging/cache/wheels/`. The install into the bundle is offline.
- **The search model** (about 200 MB): copied from the folder in `CAIRN_EMBED_MODEL_SRC`, else from `packaging/cache/embeddinggemma-300m`. When none of them has it, `packaging/fetch-model.sh` downloads the three files from Hugging Face (`onnx-community/embeddinggemma-300m-ONNX`, a pinned revision, no account needed) into that cache folder. Every file is checked against its pinned sha256 (`packaging/model-pins.sh`). The model is under the Gemma Terms of Use, which go into NOTICE.
- **Rust crates and npm packages:** `make release` does not install anything, so they must already be on the machine (`cargo build` and `npm ci` have been run before). `make` runs `npm ci` in a fresh copy, and cargo downloads the crates in its first build.
- **The AppImage tools** (linuxdeploy and its plugins): Tauri downloads them into `~/.cache/tauri` on the first AppImage build.
- **`make` only:** the Tauri command line tool and `cargo-xwin` (`cargo install`, from crates.io) and the Rust target for Windows (`rustup`), when they are missing.
- The files it builds are in `packaging/build/` and `packaging/cache/`, which git ignores. The AppImage ends up in `target/release/bundle/appimage/`, the Debian package in `target/release/bundle/deb/`.

### Windows: what goes online

`make release-windows` needs `cargo-xwin`, `clang`, `lld` and the Rust target `x86_64-pc-windows-msvc` (see [`poc/j-windows/README.md`](../poc/j-windows/README.md)); the Windows SDK it uses is in `~/.cache/cargo-xwin`. The first run downloads:

- **Python for Windows:** the python-build-standalone `install_only_stripped` tarball for `x86_64-pc-windows-msvc` (same tag and Python version as the Linux one; about 21 MB) from github.com, into `packaging/cache/`, checked against a pinned sha256.
- **The Windows wheels** listed in `sidecar/requirements.lock` (`pip download --platform win_amd64`, about 120 MB) from pypi.org, into `packaging/cache/wheels-win/`. The install into the bundle is offline.
- **The Microsoft C runtime and the Windows SDK:** `cargo-xwin` downloads them into `~/.cache/cargo-xwin` (about 1.1 GB) on its first build, under Microsoft's licence terms for them.
- **The Linux Python again, when there is no `sidecar/.venv`:** pip runs on a Linux Python 3.12. Without the development environment, the python-build-standalone tarball of the Linux build is unpacked into `packaging/build/host-python` for that (downloaded only if `packaging/cache/` doesn't have it).
- Nothing else: the model, the Rust crates and the npm packages are the same as for `make release`. Later runs download nothing.

### Updating

Build the new version and run `make install` again. It keeps the previous version next to the new one as a fallback (`~/Applications/Cairn-<version>.AppImage`, with `Cairn.AppImage` pointing at the newest). Your data is not touched. There is no automatic updater.

With the Debian package, `sudo apt install ./…deb` of the new version replaces the old one; only one version is installed at a time, so keep the previous `.deb` to go back (`sudo apt install --allow-downgrades ./Cairn_<old version>_amd64.deb`). The AppImage and the package use the same data folder and keychain entry, so you can move between them. Installing both gives two *Cairn* entries in the app menu; to keep only the package's, delete `~/.local/share/applications/cairn.desktop`.
