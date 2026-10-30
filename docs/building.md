# Building Cairn from source

How to build the release files yourself. **Basic** is all you need for a normal build. **Advanced** has every command, the single steps, the settings and what each command downloads. How to work on the code is in [development.md](development.md).

## Basic

### What you need

- **Ubuntu 24.04 or newer** (the build uses glibc 2.39).
- **Rust 1.90 or newer**, through [rustup](https://rustup.rs).
- **Node.js 22 or newer**, e.g. through [nvm](https://github.com/nvm-sh/nvm), and **Python 3**.
- These system packages:

  ```bash
  sudo apt install build-essential pkg-config curl file xdg-utils libwebkit2gtk-4.1-dev \
      libdbus-1-dev librsvg2-dev libfuse2t64 clang lld llvm
  ```

You don't have to check this list by hand: `make` checks it first and prints what to install if something is missing. It installs everything else itself.

### Build

From the top of the repository:

```bash
make
```

This builds the Linux AppImage and the Windows zip, and checks the AppImage by starting it once. The first run downloads several GB and compiles everything twice, so it takes a long time. Later runs download nothing. The heavy steps run at low priority, so the machine stays usable.

### Where the files go

Everything a build makes goes to `build/` at the top of the repository:

```
build/
  Cairn_<version>_amd64.AppImage    Linux: one file
  Cairn-<version>-windows-x64.zip   Windows: a portable folder in a zip
  stage/                            the pieces the packages are made from
  cache/                            downloads, kept between builds
```

The other packages (the Debian package and the lite packages, see Advanced) go next to them. Git ignores `build/`.

`make clean` empties `build/` but keeps `build/cache`, so the next build doesn't download anything again.

### Install and update

- **Linux:** `make install` copies the AppImage to `~/Applications/` and adds Cairn to the app menu.
- **Windows:** unzip the zip anywhere outside OneDrive and read the `README.txt` inside.

To update, build the new version and install it the same way. Your data is not touched. There is no automatic updater.

## Advanced

### Every command

All commands run from the top of the repository. `make help` lists them.

| Command | What you get in `build/` | What it does |
|---|---|---|
| `make` | `Cairn_<version>_amd64.AppImage` and `Cairn-<version>-windows-x64.zip` | Everything from a fresh copy of the source, in the steps under [What `make` does](#what-make-does). |
| `make release` | `Cairn_<version>_amd64.AppImage` | Builds the UI, the Python sidecar bundle, the search model and the NOTICE file, then the AppImage. Then it starts the AppImage once to check that the sidecar answers and embedding works. Takes several minutes. |
| `make release-deb` | `Cairn_<version>_amd64.deb` | The same as a Debian package, with the same check. See [The Debian package](#the-debian-package). |
| `make release-windows` | `Cairn-<version>-windows-x64.zip` | Builds the Windows version on this Linux machine. See [The Windows zip](#the-windows-zip). |
| `make release-lite` | `Cairn-lite_<version>_amd64.AppImage` | The AppImage without the search model. See [Full and lite packages](#full-and-lite-packages). |
| `make release-deb-lite` | `Cairn-lite_<version>_amd64.deb` | The Debian package without the search model. |
| `make release-windows-lite` | `Cairn-<version>-windows-x64-lite.zip` | The Windows zip without the search model. |
| `make release-for-publish` | Both `.deb` packages and both Windows zips | The four files `make publish` uploads, after the same first steps as `make`. |
| `make install` | | Installs the full AppImage for your user. See [Installing the AppImage](#installing-the-appimage). |
| `make install-lite` | | The same for the lite AppImage. |
| `make clean` | | Empties `build/` except `build/cache`. Cargo's compile in `target/` stays, so the next build is quick. |
| `make audit` | | Checks the Rust, UI and Python libraries for known security problems. It downloads the lists of known problems, so it asks first, every time. |
| `make publish` | | The maintainer's release: uploads the four packages as a draft on GitHub. See [Publishing a release](#publishing-a-release). |
| `make measure` | | Measures memory, start-up time and search speed on the newest full AppImage in `build/`. It loads the CPU for minutes. The method and results are in [measurements.md](measurements.md). |

### What `make` does

`make` runs `packaging/build-all.sh`, which takes these steps in order:

1. **check:** the tools and system packages under [What you need](#what-you-need). When one is missing, it stops and prints the command to install it (it never uses sudo itself).
2. **tools:** installs, for your user, the Tauri command line tool and `cargo-xwin` when they are missing or older than the versions pinned in `packaging/build-all.sh`, and the Rust target for Windows when it is missing.
3. **npm:** runs `npm ci` in `app/` when `app/node_modules` is missing.
4. **model:** downloads the search model when no copy is found.
5. **linux:** `make release`.
6. **windows:** `make release-windows`.

At the end it prints where the AppImage and the zip are. To run only some of the steps, name them, e.g. for Linux only: `packaging/build-all.sh check tools npm model linux`.

`make` doesn't build the Debian package or the lite packages; run their own commands for those. `make release-for-publish` takes the same first four steps, then builds the four files a release publishes: `packaging/build-all.sh check tools npm model deb deb-lite windows windows-lite`.

### Inside `build/`

```
build/
  Cairn_<version>_amd64.AppImage     the release files (full and lite, Linux and Windows)
  …
  stage/
    sidecar/                         Python with the sidecar's packages, for Linux
    models/                          the search model, checked against its pinned sha256
    NOTICE, NOTICE-lite
    windows/                         the same pieces for Windows: sidecar/, models/, NOTICE, NOTICE-lite
    host-python/                     a Linux Python for the Windows build, only when there is no sidecar/.venv
  cache/
    cpython-….tar.gz                 the Python downloads (Linux and Windows)
    wheels/, wheels-win/             the Python packages from sidecar/requirements.lock
    embeddinggemma-300m/             the search model (the development setup links it into sidecar/models)
    audit-venv/                      pip-audit, for make audit
```

Two things stay outside `build/`:

- **Cargo's compile** stays in `target/`, which development builds and tests share. Tauri writes each Linux package to `target/release/bundle/` first, and the release script then moves it into `build/`. `cairn.exe` is compiled in `target/x86_64-pc-windows-msvc/release/`.
- **Tool caches:** the AppImage tools are in `~/.cache/tauri`, the Windows SDK in `~/.cache/cargo-xwin`.

### Full and lite packages

Each package also comes in a **lite** version without the search model, about 160 MB smaller (NFR-INS-1b). Search by words works as usual. Search by meaning needs the model, which Settings › Search can download or add from a file.

- A lite package's NOTICE leaves out the Gemma terms, and its self-test expects no model.
- Full and lite files sit side by side in `build/`: building one keeps the other.
- The lite Debian package is still the package `cairn`, so installing one replaces the other.

### Installing the AppImage

`make install` copies the AppImage to `~/Applications/Cairn-<version>.AppImage` and points `~/Applications/Cairn.AppImage` at it. It adds Cairn to the app menu with its icon, and checks that libfuse2 is installed (it prints the `apt` command if not). It never uses sudo.

- **Updating:** build the new version and run `make install` again. The previous version stays next to it as a fallback; older ones are removed. The menu entry uses `Cairn.AppImage`, so it never changes.
- **Lite:** `make install-lite` installs the lite AppImage as `~/Applications/Cairn-lite-<version>.AppImage`. Full and lite share the `Cairn.AppImage` link, the menu entry and your data, so the one installed last is the one that starts. Each keeps its own previous version.

### The Debian package

`make release-deb` builds from the same compile as the AppImage. Unlike the AppImage it carries no copy of GTK and WebKit: it uses the system's, which Ubuntu keeps updated. So it needs no libfuse2 and is smaller. Its check unpacks the package into a scratch folder in `build/`, starts it once and removes the folder.

- **Install:** `sudo apt install ./build/Cairn_<version>_amd64.deb`. The program goes to `/usr/bin/cairn`, its files to `/usr/lib/Cairn`, and the menu entry is added.
- **Remove:** `sudo apt remove cairn`.
- **Update:** `sudo apt install` the new `.deb`. Only one version is installed at a time, so keep the previous `.deb` to go back: `sudo apt install --allow-downgrades ./Cairn_<old version>_amd64.deb`.
- **AppImage and package together:** both use the same data folder and keychain entry, so you can move between them. Installing both gives two *Cairn* entries in the app menu; to keep only the package's, delete `~/.local/share/applications/cairn.desktop`.

### The Windows zip

`make release-windows` cross-compiles with `cargo-xwin` and needs `clang`, `lld` and the Rust target `x86_64-pc-windows-msvc` (see [`poc/j-windows/README.md`](../poc/j-windows/README.md)). It cannot run the result here.

The zip is a portable folder: `cairn.exe`, the Windows Python bundle, the model, NOTICE, `README.txt` and `check-windows.ps1`. It holds no data, settings or keys. At the end the build prints the zip's path, size and file count. To check it, unzip it on Windows and run `check-windows.ps1`, then go through [windows-check.md](windows-check.md).

### Publishing a release

For the maintainer. `make publish` (`packaging/publish.sh`) makes a **draft** GitHub release; you publish it on GitHub after looking it over.

1. Bump the version (`Cargo.toml`, `src-tauri/tauri.conf.json`, `app/package.json`) and add its row to the version table in `docs/requirements.md` §9.4. Commit and push.
2. Build the four packages from that commit: `make release-for-publish` (or one at a time: `make release-deb`, `make release-deb-lite`, `make release-windows`, `make release-windows-lite`). Check the Windows zip on Windows.
3. Run `make publish`. It checks that every package is there and newer than the last commit, that nothing is left uncommitted and that the commit is on GitHub. Then it writes `build/SHA256SUMS` and the notes (`build/release-notes-<version>.md`), shows them with the files, and asks before uploading.
4. Open the draft it prints, look it over and press *Publish release*. The tag `v<version>` is made then.

- **The draft:** tag `v<version>`, in the GitHub repository of the `origin` remote (`CAIRN_PUBLISH_REPO=owner/name` overrides it), marked as a pre-release while the version is 0.x. From 1.0 on, a pre-release made from `develop` (CONTRIBUTING.md, "Branches") must be marked by hand: tick *Set as a pre-release* on the draft before publishing it. Running it again for the same version updates the draft; a published release is never changed.
- **The notes:** the first release gets the README's introduction; later ones the version table's rows since the last published release. Then the README's download section for this version. Preview them with `packaging/publish.sh notes [last published version]`.
- **Needs** the GitHub command line tool, signed in: `sudo apt install gh`, then `gh auth login`.

### Running single steps

Every release step also runs alone:

| Script | Steps |
|---|---|
| `packaging/release.sh <step>` | `sidecar`, `model`, `notice`, `ui`, `appimage`, `selftest`, `deb`, `selftest-deb`; for lite `notice-lite`, `appimage-lite`, `selftest-lite`, `deb-lite-only`, `selftest-deb-lite` |
| `packaging/release-windows.sh <step>` | `sidecar`, `model`, `notice`, `ui`, `exe`, `zip`; for lite `notice-lite`, `zip-lite` |
| `packaging/build-all.sh <step>...` | `check`, `tools`, `npm`, `model`, `linux`, `windows` |
| `packaging/publish.sh <step>` | `check` (no network), `notes [LAST]` (print the release notes) |

`--help` on each script lists its steps, one line each.

### Settings

Environment variables the build scripts read:

| Variable | What it does |
|---|---|
| `CAIRN_JOBS` | Cargo jobs (default 5). |
| `CAIRN_EMBED_MODEL_SRC` | A folder that already holds the search model (`onnx/model_q4.onnx`, `onnx/model_q4.onnx_data`, `tokenizer.json`), used instead of `build/cache/embeddinggemma-300m`. |
| `CAIRN_APPIMAGE_FLAGS` | Extra arguments when the self-test starts the AppImage. |
| `CAIRN_PBS_TARBALL`, `CAIRN_WIN_PBS_TARBALL` | A local copy of the Linux or Windows Python download, checked against its sha256. |
| `CAIRN_HOST_PYTHON` | The Linux Python 3.12 that the Windows build runs pip with (default: `sidecar/.venv`). |

### What goes online

Downloads are kept in `build/cache/` (or the tool's own cache), so later runs download nothing; `make clean` keeps them. Python, the model and LiteLLM's wheels are checked against pinned sha256 values (`packaging/pip-pins.sh`, `packaging/model-pins.sh`); the other Python packages are pinned by version.

**For Linux** (`make release`):

- **Python:** the python-build-standalone tarball (about 34 MB) from github.com, unless a copy is in `build/cache/`.
- **Python packages:** the wheels listed in `sidecar/requirements.lock`, from pypi.org, into `build/cache/wheels/` (again after the lock file changes). The install into the bundle is offline.
- **The search model** (about 200 MB): copied from the folder in `CAIRN_EMBED_MODEL_SRC`, else from `build/cache/embeddinggemma-300m`. When neither has it, `packaging/fetch-model.sh` downloads the three files from Hugging Face (`onnx-community/embeddinggemma-300m-ONNX` at a pinned revision, no account needed). The model is under the Gemma Terms of Use, which go into NOTICE.
- **The AppImage tools** (linuxdeploy and its plugins): Tauri downloads them into `~/.cache/tauri` on the first AppImage build.
- **Rust crates and npm packages:** `make release` installs nothing, so they must already be on the machine (`cargo build` and `npm ci` have run before). `make` runs `npm ci` in a fresh copy, and cargo downloads the crates in its first build.
- **`make` only:** the Tauri command line tool and `cargo-xwin` (`cargo install`, from crates.io) and the Rust target for Windows (`rustup`), when they are missing or, for the two tools, older than the pinned versions.

**For Windows** (`make release-windows`), on top of that:

- **Python for Windows:** the python-build-standalone `install_only_stripped` tarball for `x86_64-pc-windows-msvc` (same version as the Linux one, about 21 MB) from github.com, into `build/cache/`.
- **The Windows wheels** listed in `sidecar/requirements.lock` (`pip download --platform win_amd64`, about 120 MB) from pypi.org, into `build/cache/wheels-win/`. The install into the bundle is offline.
- **The Microsoft C runtime and the Windows SDK:** `cargo-xwin` downloads them into `~/.cache/cargo-xwin` (about 1.1 GB) on its first build, under Microsoft's licence terms for them.
- **The Linux Python again, only when there is no `sidecar/.venv`:** pip needs a Linux Python 3.12, so the Linux python-build-standalone tarball is unpacked into `build/stage/host-python` (downloaded only if `build/cache/` doesn't have it).
- The model, the Rust crates and the npm packages are the same as for Linux.
