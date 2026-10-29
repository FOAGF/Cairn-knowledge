# The UI (app/)

- Read the conventions in [README.md](README.md) first, point 5 (interface text) in particular.
- Type check with `npx tsc --noEmit -p tsconfig.app.json`; `-p .` checks nothing. Lint with `npm run -s lint`.
- `src/api/bindings.ts` is generated from Rust; never edit it by hand.
- New interface text goes into `src/i18n/en/<area>` and `src/i18n/zh-CN/<area>` with the terms of `../docs/ui-terms-zh.md`, then `node ../scripts/i18n-leftovers.mjs` must print 0.
- Check a visible change in the real app, not only with the type check (`../docs/development.md`, "Checking the app without clicking").
