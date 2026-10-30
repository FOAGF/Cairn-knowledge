# Plan

Open items and work in progress. What Cairn does is in [requirements.md](requirements.md); how finished work was done is in its pull request and in the version table of requirements.md §9.4. Remove an item here when it's done.

## Toward v1.0 (D-80)

v1.0 is the first stable version. The scope is in requirements.md (NFR-REL-6, NFR-PERF-1, NFR-PERF-5, NFR-INS-1).

- **Idle memory:** measure a fresh start and an idle state after use (search, editing, an Assistant answer) on a release build; then options for the maintainer to choose from (WebKit's cache and memory settings, unloading the search worker sooner, what the app process keeps after use). Nothing changes before the maintainer picks.
- **Publishing:** `make publish` is written (D-87; docs/building.md, "Publishing a release"). The GitHub command line tool is installed (2.45.0, which has every option `publish.sh` uses) and `origin` is https://github.com/FOAGF/Cairn-knowledge.git (2026-10-30). `gh` is signed in and both branches are on GitHub (2026-10-30). Still to do: a first run when the maintainer says so (a draft stays private until it is published, and a second run updates it in place).
- **Repository settings** on GitHub: CONTRIBUTING.md, "For the maintainer" (the default branch is `develop`, set 2026-10-30).
- **The 1.0 release:** the version is 1.0.0, with its row in the version table (2026-10-30). Pull request #1 merges `develop` into `main`; its merge commit gets the tag `v1.0.0`. Still to do: the builds below and `check-windows.ps1`, then `make publish` from `main`. No release is published yet, so its notes are the README's introduction (D-87).
- **The first full builds after these changes:** `make release-for-publish` (both `.deb` packages and both Windows zips) is the first build with the `build/` layout and with Pillow 13 in the sidecar bundles (the import smoke test, the pruning list, the Windows wheels). Then `check-windows.ps1` on the new zip.

## After v1.0

- **Releases built on GitHub:** the maintainer wants to explore GitHub Actions building and publishing releases, instead of `make publish` on their own machine (D-80, D-87). Until then, `make publish` publishes from any branch and marks a release as a pre-release only while the version is 0.x, so a pre-release from `develop` after 1.0 is marked by hand (docs/building.md, "Publishing a release"). Things to settle then: pre-release version numbers (`1.1.0-rc.1`, since the official `v1.1.0` needs its own tag); a release workflow needs `contents: write` for its own `GITHUB_TOKEN` and must never run on pull requests (CONTRIBUTING.md allows no secrets); the Windows zip is cross-built with `cargo-xwin`, and `check-windows.ps1` still runs on a real Windows machine.

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
