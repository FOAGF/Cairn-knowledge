# Cairn

Cairn is a desktop app for notes and tasks, with an assistant that works on them. Everything stays on your computer: pages and tasks in a local database, and a small search model that runs on your CPU, so searching by meaning works offline. The assistant uses the language-model service you set up in Settings (Azure AI Foundry, the Claude API, DeepSeek, or any OpenAI-compatible endpoint).

The interface is in English or Simplified Chinese, and every field accepts Chinese.

Cairn is a personal project, shared as it is. It runs on **Ubuntu 24.04 or newer** and **Windows 11 (x64)**.

## Warning
This is a **Purely Vibe Coded Project** meant for relevantly small scale personal use. Proceed with caution.

## What it does

- **Tasks:** statuses, priorities, due dates, labels, comments, links between tasks, a list and a board, and a short history of every change.
- **Knowledge:** small Markdown pages in categories and groups, with *Edit* and *Preview*, pasted images, LaTeX formulas, spelling and grammar checks (Harper, offline), and links between pages and tasks. Pages can also live as Markdown files in a folder of your choice.
- **Creating pages:** from a file (Markdown, text, PDF, Word, images through OCR) or from a task, with a check for duplicates and pages that cover more than one topic.
- **Assistant:** answers from your own pages and tasks with the sources cited, finds tasks and pages, and proposes changes that you confirm. Web search per message where the provider offers it.
- **Search:** by words and by meaning, in one box (Ctrl+K).
- **Your data:** daily backups, export and import of everything, no telemetry.

## Download

Each release on the [Releases page](../../releases) has two Linux and two Windows files:

| File | What it is |
|---|---|
| `Cairn_<version>_amd64.deb` | Ubuntu package with the search model built in |
| `Cairn-lite_<version>_amd64.deb` | The same without the search model, about 160 MB smaller |
| `Cairn-<version>-windows-x64.zip` | Portable Windows folder with the search model |
| `Cairn-<version>-windows-x64-lite.zip` | The same without the search model |
| `SHA256SUMS` | Checksums of the files above |

**Full or lite?** Without the search model, search works by words only, and the duplicate check and the assistant find matches by words too. The model can be added later in Settings › Search: downloaded from Hugging Face after you accept the Gemma Terms of Use, or taken from a zip or folder you already have.

**Install on Ubuntu:** `sudo apt install ./Cairn_<version>_amd64.deb`. Remove it with `sudo apt remove cairn`.

**Install on Windows:** unzip the folder anywhere outside OneDrive and run `cairn.exe`. Your data is kept in the `data` folder next to it, so a new version is a new folder: move your data across with *Export* and *Import* in Settings › Backup & data.

There is no automatic updater; install a new version the same way.

## First start

A copy with no data opens on Home with two choices: **Start fresh**, or **Import from a backup or export**. API keys are never part of a backup or an export, so after an import you enter them again in Settings › AI providers.

## Where things are (Linux)

- **Data:** `~/.local/share/cairn/` (the database, attachments, backups, logs). Settings › General can move it.
- **Backups:** `~/.local/share/cairn/backups/`, once a day while Cairn is open.
- **Logs:** `~/.local/share/cairn/logs/`. They never hold page text or API keys.
- **API keys:** the system keychain, under the name `cairn`.
- **Third-party licences:** the `NOTICE` file in `/usr/lib/Cairn/`, with the Gemma Terms of Use that apply to the search model.

## Building it yourself

`make` builds everything from a fresh copy of the source, including the AppImage, which is not published. [docs/building.md](docs/building.md) lists what you need, every command, and what each one downloads.

## Documents

- [docs/requirements.md](docs/requirements.md): what Cairn does, requirement by requirement, with the decisions behind them.
- [docs/development.md](docs/development.md): how the code is laid out and how to work on it.
- [docs/plan.md](docs/plan.md): open items and work in progress.
- [CONTRIBUTING.md](CONTRIBUTING.md): how to propose a change.

## Licence

Cairn's own code is under the [MIT licence](LICENSE). The packages it ships with are under their own licences, listed in `NOTICE`; the search model is under the Gemma Terms of Use.
