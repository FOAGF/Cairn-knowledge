# Plan

Open items and work in progress. What Cairn does is in [requirements.md](requirements.md); how finished work was done is in its pull request and in the version table of requirements.md §9.4. Remove an item here when it's done.

## Toward v1.0 (D-80)

v1.0 is the first stable version. The scope is in requirements.md (NFR-REL-6, NFR-PERF-1, NFR-PERF-5, NFR-INS-1).

- **Idle memory:** measure a fresh start and an idle state after use (search, editing, an Assistant answer) on a release build; then options for the maintainer to choose from (WebKit's cache and memory settings, unloading the search worker sooner, what the app process keeps after use). Nothing changes before the maintainer picks.
- **Publishing:** `make publish` is written (D-87; docs/building.md, "Publishing a release"). The GitHub command line tool is installed (2.45.0, which has every option `publish.sh` uses) and `origin` is https://github.com/FOAGF/Cairn-knowledge.git (2026-10-30). Still to do: `gh auth login`, push, then rehearse with a pre-release of the current 0.x when the maintainer says so.
- **Repository settings** on GitHub: CONTRIBUTING.md, "For the maintainer".
- **The 1.0 release:** the next release from `main` is 1.0, because the schema fold (D-86) is a 1.0 change. Its version-table row (§9.4, which `make publish` also takes the notes from) covers what changed since 0.17.0 (2026-10-29 and 30, not yet committed):
  - every build into `build/` (D-85), `make clean`, docs/building.md in a basic and an advanced part; `make` upgrades a Tauri command line tool or `cargo-xwin` older than the pinned version;
  - the schema folded (D-86): 0.x's steps frozen in `migrations/0.x/`, the schema at 1.0 in `migrations/1.0/`, versions counting on from 10; compatibility fixtures for schemas 6 to 10 and one in external knowledge mode (`crates/cairn-core/tests/compat.rs`);
  - `make publish` (D-87), and `make release-for-publish` for the four files it uploads;
  - KaTeX 0.18.10 (through an npm override, FR-KN-ED), Pillow 13.0.0 with uploaded images read only as PNG or JPEG (FR-KN-UP-1), certifi 2026.10.19, ammonia 4.2.4, rustls-webpki 0.103.16, webpki-roots 1.0.11, ctutils 0.4.7, pulldown-cmark 0.13.5, inout 0.2.5, Tauri command line tool 2.12.4;
  - fixes: `measure_data` refuses a folder that holds data (it looked for `cairn.db`); `scripts/setup-sidecar-dev.sh` makes the model folder only when the model is complete.
- **The first full builds after these changes:** `make release-for-publish` (both `.deb` packages and both Windows zips) is the first build with the `build/` layout and with Pillow 13 in the sidecar bundles (the import smoke test, the pruning list, the Windows wheels). Then `check-windows.ps1` on the new zip.

## The maintainer's checks

Things only the maintainer can check: on real models, on real data, or by hand.

- **Real models:**
  - DeepSeek's web search: `poc/k-deepseek/web_check.py --responses`.
  - The prompt checks: `poc/h-create/llm_check.py` and `poc/i-assistant/llm_check.py`.
  - In the app: the Assistant's template cards; the effort footer ("thought for N tokens"); references with `#` and `[[`; a task made from a referenced task's latest comment; a long referenced task saying what wasn't sent.
  - One live Assistant call per provider in use since 0.16.0, which removed LiteLLM's Rust extension.
- **After installing:**
  - mass changes;
  - unsaved page changes;
  - external knowledge mode, on a copy of a notes folder;
  - image paste and drop on Linux;
  - the .deb package;
  - holding a key;
  - closed tasks' due dates;
  - board drops;
  - the Chinese interface's wording;
  - web links in pages;
  - *Edit…* on proposals;
  - *Add to dictionary*.
  - formulas in a page (KaTeX 0.18);
  - *Create from file* with a PNG and a JPEG (Pillow 13; a file of another format named `.png` is refused).
- **Lite packages:** install a lite package; download the search model from Home's notice; *Remove* it, then add it back with *From file…*.
- **Windows:** run `check-windows.ps1` on a current zip (`make release-windows`), and go through [windows-check.md](windows-check.md). External mode on Windows is untested: case-insensitive paths, locked files, long paths, and images in the folder.
- **On this machine:** VS Code uses nearly all inotify watches, so live folder watching can fail. Raise `fs.inotify.max_user_watches`, or exclude `target/`, `build/`, `poc/` and `sidecar/.venv` in VS Code's `files.watcherExclude`.

## Audit findings left as they are

What `make audit` still reports after the dependency updates of 2026-10-29 (Rust: 10 vulnerabilities and 17 warnings, npm: 9, Python: 13), all of it below. None reaches Cairn as it is used; look again when a dependency below moves, and triage anything new the audit reports.

- **zip 8.6.0** (RUSTSEC-2026-0367, -0368, -0369): about `extract()`, which unpacks to disk with links and permissions. Neither Cairn's import (it copies each entry itself, by its checked name) nor Harper calls it. zip 9 would not clear it while Harper uses zip 8.
- **zip 7.2.0**: listed by `candle-core` in `Cargo.lock`, an optional package that is never compiled into Cairn.
- **zstd, zstd-safe, zstd-sys** (RUSTSEC-2026-0391, -0395 to -0406, -0410): a build dependency of `harper-thesaurus`, used only while compiling to compress Harper's own data; the app decodes with `ruzstd`.
- **filelock 3.32.7** (13 CVEs, fixed in 4.x): LiteLLM requires `filelock<4.0`, up to its newest release (1.106.6). The advisories looked up concern `ReadWriteLock`. Move with the next LiteLLM that allows 4.x.
- **npm, 9 findings** (body-parser, deepmerge, fast-uri, qs, systeminformation, undici, zod): all in the `shadcn` command line tool's own dependencies. The app bundles only `shadcn/tailwind.css`; nothing of the tool ships. Left in `dependencies` (maintainer's choice), so `npm audit --omit=dev` keeps reporting them.
- **Warnings:** unmaintained `bincode`, `paste`, `proc-macro-error`, `zbus_names`, `zvariant`; unsound `glib` 0.18 (Tauri's GTK 3 stack on Linux). Only their upstreams can change them.

## Known limits

- Some screens still load every task or every page: the task links panel, the page sidebar, the Assistant's find list, the task-to-page step. At 10,000 pages and 20,000 tasks, editing a page reaches about 1.4 GB. A slimmer task list would help them all.
- The indexer sorts every waiting piece while it holds the database, so a first scan or import of thousands of pages waits behind it.
- Left as they are (D-52): an over-long Assistant question shows an error instead of being shortened; Claude's "I'll search…" text streams into the answer; a find with topic words looks at 500 matches at most.
- `make` from a fresh copy of the source has never been run end to end.
