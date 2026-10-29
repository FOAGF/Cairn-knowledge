# Contributing

Cairn is a personal project shared as it is. Issues and pull requests are welcome; the maintainer reviews every change and decides what goes in.

## Before you start

- For anything larger than a small fix, open an issue first, so we agree on the behaviour before the code.
- Read the requirement rows your change touches in [docs/requirements.md](docs/requirements.md). A change in behaviour updates those rows in the same pull request.
- How to set up, run and check the app is in [docs/development.md](docs/development.md).

## A pull request

- Targets `main`, with one topic per pull request.
- Passes the checks below; CI runs them too.
- Interface text goes into both `app/src/i18n/en/` and `app/src/i18n/zh-CN/`. If you can't write the Chinese, say so and the maintainer will add it.
- A change to a prompt template in `crates/cairn-core/prompts/` is checked by the maintainer on real models before it is merged, so expect that to take longer.
- A new dependency needs a reason in the description.
- Never include API keys, real endpoints or personal data, also not in tests or screenshots.

## Checks

```bash
cargo test -p cairn-core
cargo test -p cairn                              # needs libwebkit2gtk-4.1-dev; regenerates app/src/api/bindings.ts
cd app && npm ci && npx tsc --noEmit -p tsconfig.app.json && npm run -s lint
node scripts/i18n-leftovers.mjs
sidecar/.venv/bin/python -m pytest sidecar/tests -q
```

If `cargo test -p cairn` changes `app/src/api/bindings.ts`, commit the new file.

## For the maintainer: repository settings

Set once in the repository's Settings on GitHub:

1. **Rules › Rulesets › New branch ruleset**, target `main`:
   - *Require a pull request before merging*, with 1 approval and *Require review from Code Owners*;
   - *Require status checks to pass*: add the CI checks after their first run;
   - *Block force pushes* and *Restrict deletions*.
2. **Actions › General › Fork pull request workflows from outside collaborators:** *Require approval for first-time contributors* (or for all outside collaborators).
3. **Advanced Security:** turn on Dependabot alerts.

CI uses the `pull_request` trigger only, so a fork's code runs with read-only access and no secrets. The workflows need no secrets at all, and none may be added: live model checks run on the maintainer's machine.
