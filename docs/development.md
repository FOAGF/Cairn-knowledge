# Cairn development guide

How the real app is built and how to work on it. What to build is in [requirements.md](requirements.md), and the work in progress is in [plan.md](plan.md). UI conventions are in [app/README.md](../app/README.md).

## Layout

```
Cargo.toml              cargo workspace (shared target/ at the repo root)
crates/cairn-core/      synchronous core: database, domain logic, export/import (no Tauri)
  migrations/           the schema (src/migrations.rs, D-86): 0.x/ the frozen steps of 0.x, 1.0/ the schema
                        at 1.0 (one file per area), NNNN_name.sql the steps after 1.0
src-tauri/              thin Tauri layer: async commands, events, logging, app state
  src/commands/<area>.rs  one file per area; register each command in src/lib.rs specta_builder()
app/                    React UI (from the prototype)
  src/api/bindings.ts   GENERATED from Rust by tauri-specta; never edit by hand
  src/api/client.ts     query client, error helpers, DataChanged → query refresh
  src/api/<area>.ts     TanStack Query hooks for one area (useCategories, useUpdateCategory, ...)
sidecar/                Python AI worker (M4): LLM calls and the search model; PROTOCOL.md is the contract
  cairn_sidecar/        the package (`python -I -m cairn_sidecar --role ai|embed`)
  tests/                pytest suite (mock LLM servers, real model when linked)
  tools/live_check.py   owner-run check against real endpoints (poc/a-llm/config.toml format)
packaging/              release, install and audit scripts, the release overlay config (see "Release files")
build/                  everything the release scripts make (git-ignored): release files, stage/, cache/
```

## Release files

`make release`, `make install` and `make audit` are explained in [building.md](building.md). The scripts are in `packaging/` (they may move to a separate release repository later); what they make goes to `build/` (D-85): the release files at its top, the staged pieces in `build/stage/`, the downloads in `build/cache/`.

```
packaging/release.sh          the release steps: sidecar, model, notice, ui, appimage, selftest
packaging/build-sidecar.sh    python-build-standalone + the lock's wheels, pruned -> build/stage/sidecar/
packaging/build-model.sh      the search model, sha256 checked -> build/stage/models/
packaging/notice.py           the NOTICE file (Gemma terms + third-party licences) -> build/stage/NOTICE
packaging/tauri.release.conf.json  overlay for `cargo tauri build --config` (AppImage and .deb): identifier, bundled resources, package details
packaging/install.sh, audit.sh
packaging/publish.sh          `make publish`: the packages in build/ as a draft GitHub release, with SHA256SUMS and notes (D-87)

packaging/release-windows.sh        the Windows steps: sidecar, model, notice, ui, exe, zip (`make release-windows`)
packaging/build-sidecar-windows.sh  Windows Python + the Windows wheels, pruned, compiled files only -> build/stage/windows/sidecar/
packaging/tauri.release.windows.conf.json  overlay for the Windows build: the release identifier (no bundle, so no resources)
packaging/windows/                  files that go into the zip: README.txt, check-windows.ps1, ui-check.js
```

- **Two identifiers:** `src-tauri/tauri.conf.json` has the development identifier `app.cairn.desktop.dev`, so a development build and the installed app (`app.cairn.desktop`, set by the release overlay) never share the webview's storage (theme, filters, drafts kept in `localStorage`). The data folder and the keychain entry were already separate.
- **Resources only in release builds:** `bundle.resources` (Python 470 MB, the model 210 MB) is only in the overlay, so `cargo tauri dev` and plain debug builds never copy them. Development finds the sidecar through `sidecar/.venv` and `sidecar/models` (see Run).
  - Order of lookup (`SidecarConfig::resolve`): the `CAIRN_SIDECAR_PYTHON` and `CAIRN_EMBED_MODEL_DIR` variables, then `<resources>/sidecar/python/bin/python3` and `<resources>/models/embeddinggemma-300m`, then (debug builds only) the repo's dev setup.
  - If you ever build a debug binary with the release overlay (`cargo tauri build --debug --no-bundle --config packaging/tauri.release.conf.json`), the resources are copied to `target/debug/` and win over the dev setup from then on. Delete `target/debug/sidecar`, `target/debug/models` and `target/debug/NOTICE` afterwards.
- **Paths in a Tauri config** (the base config and the overlay) are relative to `src-tauri/`, not to the folder of the config file or the repo root. That is why the UI build hook says `"cwd": "../app"`.
- **Self-test:** `target/debug/cairn --self-test` (or `CAIRN_SELF_TEST=1`) starts the sidecar from the resolved paths, embeds one sentence and prints one JSON line (`"ok": true` and exit 0, or `"ok": false` and exit 1). No window opens. With a debug build, give it the dev variables (see Run). `make release` runs it on the finished AppImage with a throw-away data folder.
- **Bundle check without the network:** `CAIRN_SITE_PACKAGES_FROM=sidecar/.venv/lib/python3.12/site-packages CAIRN_PBS_TARBALL=<tarball> packaging/build-sidecar.sh` builds the bundle from the dev venv's copies of the locked packages instead of running pip. To run the sidecar tests with the bundle's Python, put pytest (and pygments, pluggy, iniconfig) on `PYTHONPATH` from the dev venv: `PYTHONPATH=<dir with those> build/stage/sidecar/python/bin/python3 -m pytest sidecar/tests`.
- **Changing the sidecar's dependencies:** edit `sidecar/requirements.lock` (the wheels are fetched again by the next `make release`), then check the pruning list in `build-sidecar.sh`: its import smoke test fails if something it removes is needed.
- **LiteLLM's wheels are pinned by sha256** in `packaging/pip-pins.sh` (its PyPI releases were hijacked twice in 2026: 1.82.7/1.82.8 in March, 1.107.0 on 2026-10-21). Both sidecar builds and `setup-sidecar-dev.sh` stop if the wheel doesn't match, or if the lock's `litellm` version differs from the pinned one. To bump LiteLLM, change the lock line and the pins together: take the new wheel names and sha256 values from `https://pypi.org/pypi/litellm/<version>/json`, and wait a few days after a release before taking it.

### Windows release files (M8)

`make release-windows` builds `build/Cairn-<version>-windows-x64.zip` on Linux (D-55 to D-59). Rebuild it with the same command (or one step, `packaging/release-windows.sh <step>`); the first run goes online for the Python tarball and the Windows wheels (see [building.md](building.md)), later runs do not.

- **`sidecar`:** `build-sidecar-windows.sh` unpacks the pinned Windows Python (`build/cache/`), runs the dev venv's pip (`pip download` and `pip install --target` with `--platform win_amd64 --python-version 3.12 --only-binary=:all: --no-deps`) and copies `cairn_sidecar` into `Lib\site-packages`. Then it prunes as the Linux script does (plus tcl/tk, test extension modules, type stubs, numpy headers) and compiles **compiled files only** (D-57): every `.py` becomes an unchecked-hash `.pyc` beside it (`compileall -b`, built by the Linux Python 3.12, whose bytecode is the same) and the `.py` is deleted. `KEEP_PY` in the script lists any `.py` that must stay (none so far; nothing in the locked packages reads a `.py` as data). It also checks that no locked package lacks a Windows wheel and that no Windows-only dependency is missing from the lock (only `colorama`, which tqdm uses if present, is). The import smoke test cannot run on Linux: `cairn.exe --self-test` on Windows is the test.
- **`exe`:** `cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle --config packaging/tauri.release.windows.conf.json`. With `--no-bundle` Tauri copies no resources, so the `zip` step lays out the folder itself: `cairn.exe`, `sidecar/python/`, `models/embeddinggemma-300m/`, `NOTICE` (from `notice.py --windows`: Windows Python licence and `dist-info` folders, Rust crates for the Windows target), and the three files from `packaging/windows/`.
- **`zip`:** Python's `zipfile`, files sorted, fixed time stamps, the text files with Windows line endings (`README.txt` and `check-windows.ps1` must stay plain ASCII). It refuses to add anything called `data`, `config.json` or `settings.json` (D-58). The folder name and zip name come from the version in `src-tauri/tauri.conf.json`.
- **Running `ui-check.js` on Linux** (what the check script runs inside the window on Windows): start a release build with `CAIRN_AUTOMATION=1 CAIRN_DATA_DIR=<empty folder> CAIRN_DEV_SCRIPT_FILE=packaging/windows/ui-check.js CAIRN_DEV_SCRIPT_AFTER=load CAIRN_AUTOMATION_OUT=<file> CAIRN_EXIT_AFTER_SECS=120 target/release/cairn`. Every line is `PASS`/`FAIL` with its time, then `DONE`.
- **Testing the compiled-only bundle here** (what Windows runs; this found the LiteLLM `.env` failure that the normal tests can't see):
  1. Copy `build/stage/sidecar` to a scratch folder.
  2. Compile it with its own Python (`python3 -I -m compileall -q -b --invalidation-mode unchecked-hash <lib/python3.12>`), then delete every `.py`, and compile the current `sidecar/cairn_sidecar` into its `site-packages` in the same way.
  3. Copy `sidecar/tests` next to it. In `helpers.py`, start the sidecar as Cairn does: `[sys.executable, "-I", "-m", "cairn_sidecar", "--role", role]`. Give `test_m5.py`'s `SAMPLES` an absolute path.
  4. Run `pytest` with that Python and `PYTHONPATH` pointing at a folder holding links to only `pytest`, `_pytest`, `pluggy`, `iniconfig` and `pygments` from `sidecar/.venv`.
  5. `cairn --self-test` with `CAIRN_SIDECAR_PYTHON` set to that Python checks the same from Rust.
  - Why `-m` matters: the tests normally start the sidecar with `python -c`, where libraries that look for their caller's source file take a different path.
- **Checking the Windows build** is the owner's job: `check-windows.ps1` (README.txt of the zip) and the click-through in [docs/windows-check.md](windows-check.md).

## Run

```bash
source ~/.cargo/env; source ~/.nvm/nvm.sh
cd app && npm run dev          # Vite on http://localhost:5173 (keep it running)
cargo tauri dev                # from the repo root; opens the app window using that Vite server
cargo test                     # core tests + regenerates app/src/api/bindings.ts
cd app && npx tsc -b           # type-check the UI
packaging/fetch-model.sh       # once: downloads the embedding model (200 MB)
scripts/setup-sidecar-dev.sh   # once: sidecar/.venv + the embedding model linked in
sidecar/.venv/bin/python -m pytest sidecar/tests -q   # sidecar tests (about 50 s)
```

- **A test that hangs:** run the suite with `-o faulthandler_timeout=45`; pytest then prints every thread's stack when one test runs longer than 45 s. An intermittent hang needs a loop: `for i in 1 2 3 4 5 6 7 8; do timeout 200 sidecar/.venv/bin/python -m pytest sidecar/tests -q -p no:cacheprovider -o faulthandler_timeout=45 > /tmp/x/run-$i.log 2>&1; done`. This is how the Claude stream deadlock was found (plan §18).

- **The AI worker in dev builds** runs from `sidecar/.venv` with the model in `sidecar/models/embeddinggemma-300m`. Override them with `CAIRN_SIDECAR_PYTHON` and `CAIRN_EMBED_MODEL_DIR`. Without them the app still works, but search falls back to keywords and LLM features report that the AI worker isn't set up.
- **API keys in dev builds** go to the system keychain under the service name `cairn-dev`, never `cairn`.
- **Live LLM check (owner):** `sidecar/.venv/bin/python sidecar/tools/live_check.py poc/a-llm/config.toml [--only NAME]`. It runs *Test connection* for every target through the real sidecar. On the stream row, "first after X s of Y s" with X much smaller than Y proves the stream is real.
  - **`--m5`** checks the M5 additions instead, per target:
    - an image sent by file path;
    - usage on a streamed call;
    - `finish_reason: length` on a cut-off reply;
    - `llm.model_info`.
  - **`--m6`** checks web search per target: a plain and a streamed call with `web_search`, showing the searches, the sources and whether any server-tool text leaked, then the `llm.test` `web` row. A provider that ignores or refuses web search is `INFO`, not a failure. A target with `web_search = false` in the config is skipped.
- **Prompt templates** (`crates/cairn-core/prompts/`):
  - A new or rewritten template is written by an Opus sub-agent at max effort (owner's rule, D-41; Fable until 2026-10-26) and checked on the owner's models with `poc/h-create/llm_check.py` (M5) or `poc/i-assistant/llm_check.py` (M6).
  - After a template changes, run `sidecar/.venv/bin/python poc/h-create/golden.py`. The Rust test `prompts::tests::golden_matches_the_python_renderer` then confirms both renderers still agree.

- **Debug builds use `~/.local/share/cairn-dev`**, never the real data folder. Set `CAIRN_DATA_DIR=/some/dir` to use a throw-away folder.
- **`CAIRN_EXIT_AFTER_SECS=N`** (debug builds, or release builds with `CAIRN_AUTOMATION=1`) closes the app after N seconds. Use it for scripted runs, so test windows close by themselves.
- **Text rendering on Linux:** the app sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` at startup (`src-tauri/src/main.rs`), because WebKitGTK's DMA-BUF path draws text in scrolling areas doubled on this machine. Set it to `0` yourself to compare.
  - **Its cost (D-75):** without the DMA-BUF renderer, WebKitGTK repaints the whole window when a change reaches the page's layout (about 25 ms here), slower than a held key repeats (30 ms). Keep what a key changes inside the editor's box (`followHeight` in `MarkdownEditor.tsx`, `contain: size layout style`); let text elsewhere that follows typing (counts, titles in bars) catch up after a pause (`lib/useSettled.ts`); don't re-render other text fields per key (React 19 rewrites each controlled input's `name` on every re-render): memoise them.
  - **Probes:** `scripts/probes/key-repeat.js` (one Backspace per frame in the page editor, its title and a task description; frames must stay well under 30 ms), `key-repeat-surfaces.js` (the Assistant's box, a comment, the new-task dialog, the tree's search), and `editor-box.js` (the box's height, popups, glossary marks, the tab's title). Each runs on a fresh seeded folder with `PROBE_AFTER=10`; add `WEBKIT_DISABLE_DMABUF_RENDERER=0` to compare.
- **`CAIRN_LOG=debug`** makes logging more detailed. Logs go to `<data>/logs/`, and also to stderr in debug builds.
- **Shared machine:** run builds as `CARGO_BUILD_JOBS=8 nice -n 10 cargo ...`. Never kill processes you didn't start.
  - **Parallel agents in worktrees each need their own `CARGO_TARGET_DIR`.** A target folder shared between worktrees can link another worktree's build of `cairn-core`, because the artifact hashes don't include the path. In M5 this made a test run use another branch's stubs.
  - Stop your own processes by the PID you saved when starting them, never by matching names (`pgrep -f` also matches your own shell).
  - The owner often has their own Vite (port 5173) and Cairn window open: leave them alone.

## Checking the app without clicking

- **Interface text (D-79):** `node scripts/i18n-leftovers.mjs [paths]` lists English still written in components (it should print `0 leftover strings`); a string that is data or a log line gets `// i18n-ignore: <reason>`. To see the Chinese interface in a scripted run, set the `language` row in the data folder's `settings` table to `"zh-CN"` before starting. *Follow system* reads the language the web view reports, which on Linux comes from the system locale, so it shows Chinese only when a `zh_CN` locale is installed and set.

- **Your own Vite server:** start it on another port, e.g. `cd app && node node_modules/vite/bin/vite.js --port 5190 --strictPort` in the background, and save its PID.
  - Start it with `node` directly, not `npx`: `npx` is a wrapper process, so stopping the saved PID would leave Vite itself running.
  - Started as `(cd app && node … &)`, `$!` is the subshell's PID, not Vite's. Take Vite's from the port instead: `ss -ltnp | grep ':5190'`.
- **Bake that URL into a debug build:** `CAIRN_DATA_DIR=/tmp/x CAIRN_EXIT_AFTER_SECS=2 cargo tauri dev --no-watch --config '{"build":{"devUrl":"http://localhost:5190/<route>"}}'`.
  - `CAIRN_EXIT_AFTER_SECS` closes the window the way its close button does, so the UI's close handling runs too.
  - If the window doesn't close within 5 s, it logs "did not close" and forces the exit.
- **Screenshot:** `CAIRN_DATA_DIR=/tmp/x scripts/capture-window.sh out.png [VAR=value ...]` runs that build and captures only the window of that process (`xwd` + `scripts/xwd2png.py`).
  - To set up a state first, pass `CAIRN_DEV_SCRIPT_FILE=<script>` with `CAIRN_DEV_SCRIPT_AFTER=5`, and `CAPTURE_AFTER_SECS` a few seconds later than the script needs; without `CAIRN_DEV_SCRIPT_AFTER` the script may start after the capture.
- **Fresh data:** a fresh `CAIRN_DATA_DIR` gets the sample data in dev builds, inserted by the app shell's dev seed.
- **Providers that need a key, in a probe:** start the debug build with `CAIRN_DEV_MEMORY_KEYS=1` (e.g. as an extra `VAR=value` of `ui-probe.sh`): API keys are kept in memory for that run, so `set_provider_key` never reaches the system keychain. Release builds ignore it.
- **Knowledge mode in development (D-72):** a fresh data folder asks where knowledge is kept. `CAIRN_DEV_KNOWLEDGE_MODE=internal` (the default of `ui-probe.sh` and `capture-window.sh`), `external=<folder>`, or `external=sample` (a copy of `crates/cairn-core/tests/fixtures/external-sample` next to the data folder, as `<data folder>-sample-knowledge`) presets it in debug builds. `CAIRN_DEV_TRASH_DIR=<folder>` makes debug builds move files deleted in external mode there instead of the system trash; set it for probes that delete pages.
- **DeepSeek without DeepSeek:** `scripts/mock_llm.py` also plays DeepSeek: a request with the `web_search` function tool gets a tool call first, `/v1/tools/search` and `/v1/tools/fetch` answer like DeepSeek's APIs, and `GET /models` lists two models. Point a DeepSeek provider's endpoint at `http://127.0.0.1:5298`.
- **Folder pickers in a probe:** the native folder dialog can't be driven, and Tauri's `invoke` can't be replaced. In development builds every folder picker (`app/src/lib/pickFolder.ts`: the Markdown folder dialogs, the mode chooser, *Switch to external mode…*, and the import's *Choose the folder…*) asks `window.__cairnPickFolder()` first when a probe sets it, e.g. `window.__cairnPickFolder = () => '/tmp/x/notes'`; returning `null` acts as a cancelled dialog.
- **Editors in a probe:** the CodeMirror view of an editor is `el.querySelector('.cm-content').cmTile.root.view`, so a probe can set the text and the cursor with `view.dispatch(...)`. To screenshot the cursor, switch its blinking off first: add a style `.cm-cursorLayer { animation: none !important }`.
- **Scripted UI checks:** `CAIRN_DATA_DIR=/tmp/x scripts/ui-probe.sh scripts/probes/<probe>.js` runs JavaScript inside the debug build's window (`CAIRN_DEV_SCRIPT_FILE`, debug builds only) and prints what it reports. Use it to open dialogs, fill inputs, click buttons and measure things without typing in the window. `scripts/probes/dialog-flash.js` is the regression check for dialogs flashing when they close: a fade must end with `0` then `-`, never `F`.
- **Mass changes (D-70):** `scripts/probes/bulk-tasks.js` and `scripts/probes/bulk-pages.js` (each opens its page itself) drive the selection bar on the demo data, each with `PROBE_AFTER=8 PROBE_EXIT=80`; every confirmation waits out its 5-second countdown. Run them on a fresh copy of a seeded data folder: they change and delete data. Radix menus and selects open on `pointerdown`, not `click` (the probes have a `press` helper). Task rows carry `aria-label="Select <KEY>"`, page rows in select mode `data-page-row="<id>"`.
- **External knowledge mode (D-72):** `scripts/probes/ext-probe.sh` runs a probe like `ui-probe.sh`. On top of that it takes:
  - `CAIRN_BIN`: copy a baked build somewhere of your own, so a later `cargo build` can't change it;
  - `CAIRN_DEV_KNOWLEDGE_MODE=none`: no preset mode, so the full-screen chooser shows;
  - `PROBE_DRIVER`: a bash script that runs next to the app and makes outside changes when the probe reports `@@<word>`; `ext-driver-lib.sh` has `wait_for`.

  The probes are `ext-chooser`, `ext-pages`, `ext-outside`, `ext-groups`, `ext-switch`, `ext-switch-warning`, `ext-more`, `ext-scale` and `ext-recheck`, each with its `-driver.sh`, plus `ext-import.js`, `ext-change-folder.js` (*Change folder…* there and back; prepend `FOLDER_A`/`FOLDER_B`, see its header) `ext-switch-backups.js` with its driver (one backup kept per folder, and the import refusing a switch backup), and `ext-internal-choice.js` (going internal offers and accepts only internal-mode switch backups). `crates/cairn-core/examples/measure_scan.rs` times a first scan in a release build of the core, batch by batch (`cargo run --release --example measure_scan -p cairn-core`). `ext-gen-folder.py` makes a large folder. Always work on copies of `crates/cairn-core/tests/fixtures/external-sample/`, and set `CAIRN_DEV_TRASH_DIR` to a scratch folder. **Don't point the dev app at the sample itself:** it sits inside the `cairn-core` crate, so `cargo tauri dev` takes every edit there (by you, or by Cairn writing a page) for a source change, and rebuilds and restarts the app. The tests also read those files. Use a copy outside the repo, e.g. `cp -a crates/cairn-core/tests/fixtures/external-sample ~/cairn-test-knowledge`, then *Change folder…* in Settings › General. **That sample folder is not in git** (owner's choice, `.gitignore`): it exists on the development machine only, so in a fresh clone the external-mode tests that read it fail, and *Use the sample folder* finds nothing.
- **Unsaved page changes (D-71):** `scripts/probes/pending-edit.js` (`PROBE_AFTER=8 PROBE_EXIT=75`, a fresh seeded folder: it saves, deletes and leaves changes behind) edits pages through the real editor and checks the pending tab, the question and the drafts rows; it ends by reporting the ids that `scripts/probes/pending-edit-restart.js` needs. Run that one twice on the same folder, with `const IDS = {p1:…, p2:…, p4:…}; const STAGE = 2` (then `3`) prepended (`PROBE_EXIT=35`): stage 2 checks the tab after a restart, stage 3 the adoption of the newest changes when no row says what waits. The pending tab is the `header nav a` holding `[aria-label="Not saved"]`; the row is `get_draft('new_page', 'pending')`.
- **Screenshots can catch other windows:** `xwd` captures what is on screen where the test window is, so a window on top of it (the owner's browser, say) ends up in the image. Look at each capture before using it, and delete any that shows something other than Cairn.
- **About's build time and commit:** release builds get `CAIRN_BUILD_EPOCH`, `CAIRN_GIT_COMMIT` and (with tracked changes) `CAIRN_GIT_DIRTY` from `src-tauri/build.rs` on every build; debug builds show the time their program file was written, and their commit updates when HEAD moves. The newest `target/release/build/cairn-*/output` shows what a release build recorded (a changed `build.rs` gets a new directory).
- **Drops from Linux file managers:** under WebKitGTK some send `text/uri-list` with **empty** data and name the file only in `text/html`, so `droppedLink` reads the HTML too. To imitate them in a probe: `dt.setData('text/uri-list', 'about:blank'); dt.setData('text/html', '<a href="file:///…">…</a>')`.
- **Never send an empty paste in a probe.** When a paste holds no text and no file, the editor asks Rust to read the **real system clipboard** (`read_clipboard_image`, for WebKitGTK's screenshot pastes). A simulated `ClipboardEvent` loses a `text/uri-list` (a drop keeps it), so it counts as empty too. Probes paste only real `File`s (`DataTransfer.items.add`), and test links with drops.
- **File watches used up (Linux).** Every program shares one limit on watched folders (`fs.inotify.max_user_watches`, 65,536 here), and VS Code open on this repo can use nearly all of it: the repo has about 49,000 folders (`poc/`, `target/`, `sidecar/.venv`, …). Then the Cairn watcher tests fail with "OS file watch limit reached", the app's external mode can't watch its folder, and `cargo tauri dev` itself stops before building. To bake a dev URL without it: `TAURI_CONFIG='{"build":{"devUrl":"http://localhost:5190/","beforeDevCommand":""}}' cargo build -p cairn`. To find the user, sum the `inotify wd:` lines in `/proc/*/fdinfo/*`. The cure is a higher limit, or VS Code's `files.watcherExclude`.
- **A plain `cargo build -p cairn` bakes `tauri.conf.json`'s devUrl (port 5173).** Before running `target/debug/cairn` for a probe or screenshot, bake your own port again with `cargo tauri dev --no-watch --config …` (above), or the window loads whatever runs on 5173.
- **LLM flows without real models:**
  1. Start `scripts/mock_llm.py`, an OpenAI-compatible mock that answers by the prompt template's first words: `python3 -I scripts/mock_llm.py 5298 <log>` in the background, saving its PID.
  2. Run `scripts/probes/m5-flows.js` with the sample files prepended: `CAIRN_DATA_DIR=/tmp/x PROBE_EXIT=150 scripts/ui-probe.sh "$(scripts/probes/with-samples.sh scripts/probes/m5-flows.js)"`.

     It adds a "Mock LLM" provider in that data folder and drives every M5 flow through the real sidecar: file drafts for md, pdf, a scanned pdf, jpg and docx; revisions; checks; duplicates; task drafts; merge help; *Suggest groups*.
  3. `scripts/probes/m5-composer.js` does the same through the real composer, and samples the streamed text.
  4. **M6, the Assistant:** `scripts/probes/m6-flows.js` (same command, `PROBE_EXIT=300`) drives every assistant flow through the real commands:
     - questions with sources and citations, a general question, a follow-up, a find list;
     - a scoped conversation with *Search all categories*;
     - proposals with *Confirm*, an unticked row and *Reject*;
     - a new page into a draft card, and a page change with its diff;
     - web search: a cited answer, and an ignored request that removes the model's tick;
     - the history budget, and a question about an attached PDF.

     The mock answers the M6 templates too. Web search: a request with `web_search_options` gets `url_citation` annotations, unless the question contains "ignore".
  5. **Effort (D-65):** the mock's log line shows the effort each request carried (`effort=high`, `-` when none), and a request with `reasoning_effort` low, medium or high reports 120, 480 or 1,500 reasoning tokens, so the reply's "· high effort · thought for N tokens" can be checked. The mock is OpenAI-compatible: create its provider with `kind: 'openai_compatible'`, `endpoint: 'http://127.0.0.1:5298/v1'`, `auth: 'none'`, and a model with `effort: { enabled: true, … }`.
  - **The Tauri tests' fake sidecar** (`src-tauri/tests/fixtures/fake_sidecar.py`) has an M6 answers block and an M6 actions block. Marker words in the message steer them (documented at the top of each block). It decides the effort level as the real worker does (`resolve_level`) and then reports 10 reasoning tokens per call.

  The mock must follow the templates when their first lines change.
- **White screenshots:** while the screen is locked or the screen saver is on, windows aren't painted, so every capture is white. Check with `gdbus call --session --dest org.gnome.ScreenSaver --object-path /org/gnome/ScreenSaver --method org.gnome.ScreenSaver.GetActive` (`(true,)` means locked).

## Automation and measuring (M7)

- **Release builds:** `CAIRN_EXIT_AFTER_SECS` and `CAIRN_DEV_SCRIPT_FILE` (with `CAIRN_DEV_SCRIPT_AFTER`) work in debug builds as before, and in release builds only when `CAIRN_AUTOMATION=1` is also set (`src-tauri/src/lib.rs`). `CAIRN_DEV_SCRIPT_AFTER=load` starts the script when the page has loaded, not after a delay; start-up measuring needs it.
- **Reporting from a script:** the release window's content security policy allows no `fetch` to a local listener. A script calls `invoke('automation_report', { line })` (`window.__TAURI_INTERNALS__.invoke`), which appends the line to the file named by `CAIRN_AUTOMATION_OUT`. It refuses unless `CAIRN_AUTOMATION=1`. `window.eval` itself isn't affected by the page's policy: the script runs, and only its own network calls are blocked.
- **Measuring (NFR-PERF-6, D-51):** `make measure` runs `scripts/measure/measure.sh`, which builds the data generator (`crates/cairn-core/examples/measure_data.rs`) and runs `scripts/measure/measure.py` on the newest AppImage; the script that runs inside the window is `scripts/measure/measure.js`. Results are appended to [docs/measurements.md](measurements.md).
  - Only when the owner says so: it loads a shared machine for several minutes.
  - Quick check of the pipeline on a debug binary (`cargo tauri build --debug --no-bundle`; run `npm run build` in `app/` first if `beforeBuildCommand` can't find npm): `MEASURE_DATA_BIN=target/debug/examples/measure_data scripts/measure/measure.sh target/debug/cairn --pages 300 --tasks 600 --cpu-secs 10 --rounds 3 --out -`. `--out -` prints the section without touching `docs/measurements.md`. Use a mock port of your own with `--mock-port`.
  - Data only: `cargo run --release -p cairn-core --example measure_data -- /some/empty/dir --pages 300 --tasks 600`, then `CAIRN_DATA_DIR=/some/empty/dir` for the app. Daily backups are off in that data, and the Assistant's default model is the mock LLM on port 5299.
  - If the app's embedding model id or dimensions change (`EMBED_MODEL_ID`, `EMBED_DIMS` in `src-tauri/src/sidecar.rs`), change `MODEL_ID` and `DIMS` in `measure_data.rs` too. Otherwise the app empties the generated vectors at start and the measuring run reports the index as not ready.

- **Probes in a build with the UI embedded** (`cargo tauri build --debug --no-bundle`): the strict policy blocks `ui-probe.sh`'s `fetch` reports there. Report with `automation_report` (above), or with an image request to a local listener (`new Image().src = 'http://127.0.0.1:PORT/…'`; web images are allowed).
- **Backups and first start (M7):**
  - A fresh data folder gets its first daily backup about 5 s after start, so every scripted run leaves one in `<data>/backups`.
  - `VITE_CAIRN_NO_SEED=1` (set when starting your Vite) skips the dev seed, so the first-start card on Home can be checked; Vite reads it at start.
  - Probes: `scripts/probes/m7-backups.js` (Backup card, *Back up now*, list, *Restore…*), `m7-first-start.js`, `m7-demo-reset.js`.
- **Sidecar log lines:** the sidecar writes `[sidecar] LEVEL text` to stderr (`sidecar/cairn_sidecar/rpc.py`); `sidecar.rs` maps the level (warnings and errors are always kept, info and debug only with *Detailed logs*). A line without the prefix (a traceback) is kept as a warning.

## Adding a feature end to end

1. **Schema:** add `crates/cairn-core/migrations/NNNN_name.sql` (the first is `0011`) and append it to `STEPS` in `src/migrations.rs`. Never edit a step that has been committed, nor anything in `migrations/0.x/` or `migrations/1.0/`. Steps are folded only at a major version change (D-86, NFR-REL-6). A new step needs a compatibility fixture for its schema version ([crates/cairn-core/tests/fixtures/compat/README.md](../crates/cairn-core/tests/fixtures/compat/README.md)). The schema version counts every step, 0.x's included, so it goes on from 10 (the schema at 1.0 makes version 10, D-86); NNNN is the version the step brings the database to.
2. **Core:** add a module in `crates/cairn-core/src/` (e.g. `categories.rs`) with plain functions that take `&Connection` or `&mut Db`, and return `cairn_core::Result<T>`.
   - Domain structs derive `Serialize, Deserialize, specta::Type` with `#[serde(rename_all = "camelCase")]`.
   - Timestamps are `String` (ISO 8601 UTC from `util::now_iso()`), and new ids come from `util::new_id()`.
   - User mistakes return `CoreError::Invalid` and clashes return `CoreError::Conflict`, with messages written for the user.
   - Unit tests go in the same file and use `Db::open_at(tempdir)`.
3. **Command:** add an `async fn` in `src-tauri/src/commands/<area>.rs` with `#[tauri::command] #[specta::specta]`. It calls the core through `state.with_db(|db| ...)`, and after a change it calls `events::emit_changed(&app, &[Entity::X])`. Register it in `specta_builder()` in `src-tauri/src/lib.rs`. A new kind of data needs a new `Entity` variant.
4. **Bindings:** `cargo test -p cairn` regenerates `app/src/api/bindings.ts`.
5. **Hooks:** add them in `app/src/api/<area>.ts`.
   - Query keys start with the entity name: `['categories']`, `['categories', id]`.
   - Mutations call `commands.x(...)`. They don't need to invalidate anything themselves, because Rust's `DataChanged` event does it. Errors are shown as toasts automatically.
6. **Screens:** replace the store reads (`useAppStore((s) => s.categories)`) with the hooks, and handle `isPending` and `error`. Remove the area's mock slice from `app/src/store` once nothing uses it.
7. **Export/import:** nothing to add: every table is exported automatically (`crates/cairn-core/src/export.rs`). Tables that only hold derived data (search/vector indexes) go in `DERIVED_TABLES` there; FTS virtual and shadow tables are skipped by themselves. Extend the round-trip test (`export_import_round_trip`) with rows for the new table.

## Conventions

- **Requirement IDs in comments** where code implements a specific requirement (e.g. `// FR-DATA-6`).
- **Rust:** `cargo fmt`, no `unwrap()` outside tests, errors via `CoreError`. Keep commands thin: logic lives in the core.
- **SQL:** plain SQL with `?N` parameters. Never build SQL by joining user text into the query string.
