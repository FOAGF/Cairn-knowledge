# Plan

Open items and work in progress. What Cairn does is in [requirements.md](requirements.md); how finished work was done is in its pull request and in the version table of requirements.md §9.4. Remove an item here when it's done.

## Toward v1.0 (D-80)

v1.0 is the first stable version. The scope is in requirements.md (NFR-REL-6, NFR-PERF-1, NFR-PERF-5, NFR-INS-1).

- **Compatibility tests (NFR-REL-6):** imports and upgrades already handle data from 0.10.2 on (schema 6). Still to do:
  - a fixture database and export for each schema version from 6 on, made with sample data by the code of that version;
  - a test that opens each fixture and imports each export.
- **Idle memory:** measure a fresh start and an idle state after use (search, editing, an Assistant answer) on a release build; then options for the maintainer to choose from (WebKit's cache and memory settings, unloading the search worker sooner, what the app process keeps after use). Nothing changes before the maintainer picks.
- **Dependencies:** update Node.js and Rust; KaTeX to 0.18.2; `cargo update -p ammonia -p rustls-webpki -p webpki-roots`; `TAURI_CLI_VERSION` 2.12.4 or later in `packaging/build-all.sh`; then `make audit`.
- **Publishing:** `make publish` makes a draft GitHub release from the maintainer's own builds: the full and lite `.deb` and Windows zip, with `SHA256SUMS` and notes from the version table. A pre-release of the current 0.x rehearses it first, when the maintainer says so.
- **Repository settings** on GitHub: CONTRIBUTING.md, "For the maintainer".

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
- **Lite packages:** install a lite package; download the search model from Home's notice; *Remove* it, then add it back with *From file…*.
- **Windows:** run `check-windows.ps1` on a current zip (`make release-windows`), and go through [windows-check.md](windows-check.md). External mode on Windows is untested: case-insensitive paths, locked files, long paths, and images in the folder.
- **On this machine:** VS Code uses nearly all inotify watches, so live folder watching can fail. Raise `fs.inotify.max_user_watches`, or exclude `target/`, `poc/`, `sidecar/.venv` and `packaging/` in VS Code's `files.watcherExclude`.

## Known limits

- Some screens still load every task or every page: the task links panel, the page sidebar, the Assistant's find list, the task-to-page step. At 10,000 pages and 20,000 tasks, editing a page reaches about 1.4 GB. A slimmer task list would help them all.
- The indexer sorts every waiting piece while it holds the database, so a first scan or import of thousands of pages waits behind it.
- Left as they are (D-52): an over-long Assistant question shows an error instead of being shortened; Claude's "I'll search…" text streams into the answer; a find with topic words looks at 500 matches at most.
- `make` from a fresh copy of the source has never been run end to end.
