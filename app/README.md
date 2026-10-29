# Cairn — UI

The React UI of Cairn. It started as a clickable prototype and is being moved to the
Rust core area by area. How the whole app is run and built: [`../docs/development.md`](../docs/development.md).
Spec: [`../docs/requirements.md`](../docs/requirements.md).

- **Real data (Rust):** categories, settings, tasks, knowledge (groups, pages, history, links, search, drafts), AI providers and models, the glossary, conversations (messages, draft cards, file texts; `src/api/conversations.ts`), export/import. Read with TanStack Query hooks in `src/api/`.
- **UI state in the Zustand store:** the draft in progress while it moves between screens (chat card → review → merge; `store/creation.ts`, saved to the browser's `localStorage`).
- **LLM features (M5, real):**
  - creating a page from a file in the Assistant, and revising the draft by chatting (Rust jobs);
  - the save checks: topic, group, tags, glossary suggestions, duplicates by meaning;
  - *Keep one topic only*;
  - the merge screen's start and *Suggest merge*;
  - task → knowledge;
  - *Suggest groups*.
- **How the UI uses them:**
  - Streamed text arrives through `src/api/llm.ts` (`useLlmStream`, `cancelJob` for *Stop*, `isLlmUnavailable` when no model is set).
  - The "N possible glossary terms" card is `features/glossary/TermSuggestions.tsx`, shared by the chat card and the review step.
- **The Assistant (M6, real):** a message is a Rust job (`sendMessage` in `src/api/assistant.ts`); its answer, find list or proposal is stored on the assistant message, so the UI only reads `useMessages` and the streamed text.
  - `features/assistant/AnswerView.tsx`: Markdown with `[n]` markers as links, chips for the cited sources, *N more searched*, the basis label, *Search all categories*, glossary suggestions.
  - `FindList.tsx` (find lists), `ProposalCard.tsx` (proposed changes), the composer's history left and 🌐 web search (one message; web sources open with `openUrl`, never in the window).
  - A file in a normal conversation goes with the message (`uploadId`); the empty *Create knowledge from a file* conversation and a draft being refined keep the M5 paths (`send.ts`).
- The same React + TypeScript code will move into the Tauri app later; only the
  data layer (`src/store`) is replaced.

## Run

```bash
cd app
npm install
npm run dev        # http://localhost:5173
npx tsc -b         # type-check
```

## Where to tweak the design

| What | Where |
|---|---|
| Colours, font, corner radius (light + dark) | `src/index.css`, section **CAIRN DESIGN TOKENS** |
| Pastel tints for tags/status/priority | `src/lib/labels.ts` (`TINT_CLASS`, `STATUS_TINT`, `PRIORITY_TINT`) |
| Top navigation | `src/components/layout/TopNav.tsx` |
| Rendered Markdown style | `src/index.css`, section **Rendered Markdown** |

## Layout

```
src/
  App.tsx                 route map
  index.css               design tokens + global styles
  lib/                    types, mock data, labels, formatting, mentions, search, workdays
  store/                  Zustand store, one slice file per feature area
  components/
    ui/                   shadcn/ui primitives (generated; edit freely)
    layout/               AppShell, TopNav, AskBar, CommandPalette, ThemeSync
    shared/               TintIcon, TagChip, StatusBadge, PriorityIcon, PageHeader, SectionCard, MarkdownView,
                          CategoryDot / CategoryIcon / categoryTint / GlobalDot (tint.tsx), ColorPicker, GlossaryTerm, MeaningNotice
    editor/               MarkdownEditor (CodeMirror 6, styled source), MarkdownField (Edit | Preview + Hints)
  features/
    home/  categories/  tasks/  knowledge/  glossary/  creation/  assistant/  settings/
```

## Conventions (read before changing code)

1. **Styling:** Tailwind classes + shadcn/ui components. Use semantic colours
   (`bg-card`, `text-muted-foreground`, `border`, `bg-primary`) and the pastel
   tints (`bg-tint-blue`, `text-tint-green-fg`, … or `TINT_CLASS[tint]`). Never
   hard-code hex/rgb colours; everything must work in dark mode.
   **Category colours** can be a preset or any user-chosen `#rrggbb` (D-40): always
   draw them with `CategoryDot`, `CategoryIcon` or `categoryTint(color)` from
   `components/shared/tint.tsx`, never with `TINT_CLASS[category.color]`.
   Cards: `rounded-2xl border bg-card p-5`. Use `SectionCard`/`PageHeader` where they fit.
2. **Tailwind classes must be literal strings.** Don't build class names
   dynamically (`'bg-' + color`); use a lookup map like `TINT_CLASS`.
3. **Zustand selectors must return stable values.** Select whole arrays/objects
   (`useAppStore((s) => s.tasks)`) and filter with `useMemo`, or use
   `useShallow`. A selector like `s.tasks.filter(...)` returns a new array every
   render and causes an infinite loop.
4. **State:** data owned by Rust goes through the TanStack Query hooks in
   `src/api/<area>.ts` (see docs/development.md). The Zustand store (`src/store/*.ts`)
   keeps UI state only, such as the draft in progress between the creation screens.
5. **Text (NFR-I18N-2, D-79):** the interface is in English or Simplified Chinese. Never write
   interface text in a component: put it in `src/i18n/en/<area>.ts` and its Chinese in
   `src/i18n/zh-CN/<area>.ts` (worded with [`docs/ui-terms-zh.md`](../docs/ui-terms-zh.md)), and read
   it with `const t = useT()`. A message with values is a function (`t.dates.overdue(3)`); code
   outside components uses `getT()`. Dates go through `lib/format.ts`, and enum names through
   `statusLabel`/`priorityLabel`/`typeLabel` in `lib/labels.ts`. Text Cairn stores as data stays in
   the code, in English. `node scripts/i18n-leftovers.mjs` (from the repo root) lists text still in
   the code. All inputs must accept Chinese (the font stack already includes CJK fonts).
6. **Mentions:** `#KEY-12` (task) and `[[Page title]]` (page). Render Markdown
   with `<MarkdownView text=… />` so mentions become chips.
7. **Editing Markdown:** use `<MarkdownField value onChange … />` (Edit | Preview and the Hints, D-62). Use
   `<MarkdownEditor />` directly only where the screen has its own toolbar (the page editor).
8. **Requirement IDs:** when a component implements a requirement, add a short
   comment with its ID (e.g. `// FR-TASK-SUM-1`), so the real build can trace it.
9. Keep components small and readable; the owner is new to front-end work.

## Screenshots (for visual checks)

With the dev server running:

```bash
node scripts/screenshot.mjs <outDir> /tasks /knowledge     # light theme
THEME=dark node scripts/screenshot.mjs <outDir> /tasks      # dark theme
FULL=1 ...                                                  # full-page
```
