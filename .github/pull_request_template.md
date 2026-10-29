## What and why

<!-- What changes for the user, and which requirement rows (docs/requirements.md) it touches. -->

## Checklist

- [ ] The requirement rows are updated when behaviour changes (a decision is tagged `(D-N)`).
- [ ] Interface text is in `app/src/i18n/en/` and `app/src/i18n/zh-CN/` (or say that the Chinese is missing).
- [ ] `cargo test`, the type check, lint, `node scripts/i18n-leftovers.mjs` and the sidecar tests pass.
- [ ] `app/src/api/bindings.ts` is regenerated if a Rust command or type changed.
- [ ] Prompt templates: changed? Then the maintainer checks them on real models before merging.
- [ ] A new dependency, if any, is explained here.
- [ ] No API keys, real endpoints or personal data, also not in tests or screenshots.
