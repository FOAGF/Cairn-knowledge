# Compatibility fixtures

Data made by earlier versions of Cairn, which every later version must still open and import (NFR-REL-6). `tests/compat.rs` checks them. Never change these files: each one is what its version wrote.

Each folder holds the data folder (`data/`: `cairn.sqlite` and `attachments/`) and its export (`export.zip`), both made by `examples/compat_fixture.rs` at that version's own commit, through that version's own save paths. There is one per schema version, from the last release that used it, and one in external knowledge mode:

| Folder | Schema | Made by | Left out (that version doesn't have it) |
|---|---|---|---|
| `0.10.2` | 6 | Cairn 0.10.2 | the conversation's model and effort, archiving, the DeepSeek provider, the knowledge mode |
| `0.10.3` | 7 | Cairn 0.10.3 | archiving, the DeepSeek provider, the knowledge mode |
| `0.11.0` | 8 | Cairn 0.11.0 | the DeepSeek provider, the knowledge mode |
| `0.12.1` | 9 | Cairn 0.12.1 | the knowledge mode |
| `0.17.0` | 10 | Cairn 0.17.0 | |
| `0.17.0-external` | 10 | Cairn 0.17.0, with `external` | |

Knowledge is internal in all but `0.17.0-external`, so they have no nested groups and their external-mode tables are empty. `0.17.0-external` (external mode exists from 0.13.0, schema 10, D-72) also holds its knowledge folder (`notes/`): a group inside a group, a page written outside Cairn, and a group folder deleted outside Cairn, whose data waits in `removed_groups` and `removed_pages`. Its database records the folder it was made in, so the test points its copy at the copied `notes/` before opening it, and imports its export with that folder (FR-KN-EXT-14). After the checks, a full scan of the folder must find nothing to change.

## Adding one

A new schema step needs a fixture for its schema version; `every_schema_version_has_a_fixture` fails until it has one. Add it in the same change as the step, after the version in the workspace `Cargo.toml` is bumped to the release that brings the step (the export records that version, and the test compares it with the folder's name):

1. Make `compat_fixture.rs` write what the step adds.
2. Run `cargo run -p cairn-core --example compat_fixture -- <empty folder>`.
3. Copy that folder's `data/cairn.sqlite`, `data/attachments/` and `export.zip` here as `<version>/`.
4. Add the version to `FIXTURES` in `tests/compat.rs`, with checks for what the step added, and a row to the table above.

A step that changes what external mode keeps (`pages`, `groups`, `categories.folder`, `removed_pages`, `removed_groups`) also needs an external one: run the generator with `external` added after the folder, copy its `notes/` too, as `<version>-external/`, and add it to `EXTERNAL`.
