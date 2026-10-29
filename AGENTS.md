# Instructions for AI agents

These are the rules for any AI agent working in this repository (Claude Code reads them through `CLAUDE.md`). Personal, machine-specific rules belong in a git-ignored `CLAUDE.local.md`, never here.

## Where things are

| Document | What it holds |
|---|---|
| [docs/requirements.md](docs/requirements.md) | The spec: what Cairn does, one requirement per row, and the decisions behind them. The source of truth for behaviour. |
| [docs/plan.md](docs/plan.md) | Open items and work in progress. |
| [docs/development.md](docs/development.md) | Code layout, running the app, checks, probes, the sidecar, automation. |
| [docs/building.md](docs/building.md) | Release builds and what each one downloads. |
| [app/README.md](app/README.md) | UI conventions. Read before changing anything in `app/`. |
| [crates/cairn-core/prompts/README.md](crates/cairn-core/prompts/README.md) | The prompt template format and how templates are checked. |
| [sidecar/PROTOCOL.md](sidecar/PROTOCOL.md) | The messages between Rust and the Python sidecar. |
| [docs/ui-terms-zh.md](docs/ui-terms-zh.md) | The approved Chinese terms for the interface. |
| `poc/` | Live check scripts for prompts and providers (`h-create`, `i-assistant`, `a-llm`, `k-deepseek`) and the Windows cross-build notes (`j-windows`). |

## The spec

- Read the requirement rows a change touches before changing code, and cite their IDs in code comments (e.g. `// FR-TASK-SUM-1`).
- **A decision goes into the rows it shapes**, written as final behaviour in plain requirement language and tagged `(D-N)` with the next free number. Overwrite superseded wording instead of adding history. §11 is only for decisions that fit no single requirement.
- Every release adds a row to the version table in §9.4 and bumps the spec's version at the top.

## Rules for the code

- **Interface text** lives in `app/src/i18n/en/<area>.ts(x)` with its Chinese in `app/src/i18n/zh-CN/`, worded with `docs/ui-terms-zh.md`. Components read it with `const t = useT()`. Never write interface text in a component. Text Cairn stores as data, log lines and messages from the Rust core stay English.
- **The schema only grows** (NFR-REL-6): a schema change is a new step appended in `crates/cairn-core/migrations/`; existing steps are never edited, folded or reordered. Data from 0.10.2 on must keep opening and importing.
- **Generated bindings:** `app/src/api/bindings.ts` is generated from Rust (`cargo test -p cairn --lib export_typescript_bindings`). Never edit it by hand.
- **Prompt templates** (`crates/cairn-core/prompts/`): a new template, or a rewrite of one, is written by an Opus sub-agent at max effort, which also adds judges for it to `poc/h-create/llm_check.py` or `poc/i-assistant/llm_check.py`. After a template changes, run `sidecar/.venv/bin/python poc/h-create/golden.py`. Only the maintainer runs the live checks on real models; never use real endpoints or keys.
- **Dependencies:** add one only with a reason in the pull request. LiteLLM is pinned by SHA-256 (`packaging/pip-pins.sh`); keep it that way.
- **Secrets:** never read, print or commit API keys or `poc/a-llm/config.toml` (git-ignored).
- Match the surrounding code: its naming, comment density and style. Keep components small; the maintainer is new to front-end work and to Rust.

## Checks before a commit

```bash
cargo test -p cairn-core                         # core tests
cargo test -p cairn                              # app tests; also regenerates bindings.ts
cd app && npx tsc --noEmit -p tsconfig.app.json  # the type check (`-p .` checks nothing)
cd app && npm run -s lint                        # oxlint; add no new warnings
node scripts/i18n-leftovers.mjs                  # must print "0 leftover strings"
sidecar/.venv/bin/python -m pytest sidecar/tests -q
```

A UI change is also checked in the real app (docs/development.md, "Checking the app without clicking").

## Releases

Releases are built on the maintainer's machine and published with `make publish`. Agents never create tags, releases or pushes unless the maintainer asks.
