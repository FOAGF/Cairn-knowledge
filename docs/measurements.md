# Measurements

What `make measure` found on the owner's machine, one dated section per run, oldest first. The targets come from
[requirements.md](requirements.md) (NFR-PERF-1 to NFR-PERF-5); they are guidelines. Going over one is reported here
and looked into, but it never fails a build (NFR-PERF-6, D-51).

## What it measures

- **Start-up to a usable window:** from launching the file until Home shows the page count and the Today list of the
  full-scale data (NFR-PERF-2: 3 s).
- **Memory:** PSS summed over the whole process tree (the app, the WebKit processes, the AI workers), in four states:
  idle, after a search by meaning (the search model is loaded), while editing a page (spelling and grammar checks are
  loaded), and after an Assistant answer (mock LLM, never a real model). Each state has its own fresh start of the app.
  PSS counts shared pages once, so it is smaller than the sum of RSS (NFR-PERF-1: idle about 400 MB, peak about 1.5 GB).
- **Search times:** keyword search and hybrid search (keyword plus meaning) through the app's own commands, at 10,000
  pages and 20,000 tasks; median and 95th percentile over a fixed set of queries (NFR-PERF-3: 300 ms and 1 s).
- **CPU over 60 s idle** for the whole tree (NFR-PERF-5).
- **The AppImage size.**

The data is made by `crates/cairn-core/examples/measure_data.rs` in a throw-away folder: 10,000 pages and 20,000 tasks
(some Chinese), about 20 categories, groups, tags, comments, a glossary and task-page links. The search vectors are
generated (random unit vectors with the app's model id and dimensions), so no long indexing is needed; the scan's
speed doesn't depend on what the vectors say, and the real model still embeds each query.

## How to run it

`make measure` (or `scripts/measure/measure.sh [AppImage]`). It uses the newest AppImage under
`src-tauri/target/release/bundle/appimage/`, loads the CPU for several minutes and needs a screen (the app opens its
window while it runs). Run it only when the machine is free. Options and details are in `scripts/measure/measure.sh`.

## How to read it

Each section starts with the machine and the build, then one table: the result, the target and whether the result is
within it. Compare sections only from the same machine. A first run after a reboot has colder file caches, so its
start-up is slower than the others.

The results follow, one section per run. Nothing has been run yet when there is none below.


## 2026-10-17 20:05: Cairn 0.7.0

- **Run:** `Cairn_0.7.0_amd64.AppImage`, commit c4850fb
- **Machine:** Intel(R) Core(TM) Ultra 7 165H, 22 logical cores, 62.3 GB RAM, Linux 7.0.0-28-generic, Ubuntu 24.04.4 LTS
- **Data:** generated in 46 s; model `embeddinggemma-300m-q4`, 256 dims; the index was ready at the start.

| Measure | Result | Target | |
|---|---|---|---|
| Start-up to a usable window | 0.98 s (median of 4 starts: 2.93, 0.92, 1.00, 0.95) | under 3 s (NFR-PERF-2) | within |
| Memory, idle (no models loaded) | 281 MB PSS (webkit 185, app 96; 3 processes) | about 400 MB (NFR-PERF-1) | within |
| Memory, after a search by meaning (search model loaded) | 751 MB PSS (search worker 447, webkit 187, app 116; 4 processes) | peak about 1500 MB | within |
| Memory, editing a page (spelling and grammar checks) | 1136 MB PSS (webkit 732, app 404; 3 processes) | peak about 1500 MB | within |
| Memory, after an Assistant answer (mock LLM) | 966 MB PSS (search worker 442, ai worker 233, webkit 186, app 105; 5 processes) | peak about 1500 MB | within |
| Peak memory over all four runs | 1141 MB PSS | about 1500 MB (NFR-PERF-1) | within |
| Keyword search | median 41 ms, 95th percentile 64 ms, max 68 ms (180 searches: 12 queries x 15 rounds) | under 300 ms (the 95th percentile is compared) (NFR-PERF-3) | within |
| Hybrid search (model loaded) | median 209 ms, 95th percentile 259 ms, max 304 ms (150 searches: 10 queries x 15 rounds) | under 1000 ms (the 95th percentile is compared) (NFR-PERF-3) | within |
| CPU over 61 s idle | 1.87 % of one core (1.14 CPU seconds in 61 s; webkit 0.62 s, app 0.52 s) | none beyond indexing; under 1 % counts as none (NFR-PERF-5) | **over** |
| Design scale | 10,000 pages, 20,000 tasks, 75,682 search pieces | 10,000 pages and 20,000 tasks (NFR-PERF-4) | full scale |
| AppImage size | 388 MB | no target |  |

- idle: version 0.7.0
- idle: index 75682/75682 pieces ready, model embeddinggemma-300m-q4 dims 256, sidecars {"ai":{"state":"stopped","detail":null},"embed":{"state":"stopped","detail":null}}
- meaning: first hybrid search (loads the model) 1580 ms, meaning ready
- editing: editor underlines 27 (dispatched; 6507 characters; lint classes cm-lintRange cm-lintRange-warning cm-lint-grammar | cm-lintRange cm-lintRange-error cm-lint-spelling), lint_text hits 3
- assistant: assistant answered in 4268 ms, 138 chars
- Memory is PSS summed over the app's whole process tree (median of the samples over a 6 s hold). Each state comes from its own fresh start of the app.
- The Assistant state includes the search worker too: the Assistant looks up sources by meaning before it answers.
- Start-up runs from launching the file until Home shows the page count and the Today list of this data (the page's clock, `Date.now()`). It includes the AppImage's own start.
- The search model loads in the first hybrid search of the meaning run; the vectors in this data are generated, the query vector is real.
