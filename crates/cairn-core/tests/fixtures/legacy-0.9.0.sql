-- Released Cairn 0.9.0 schema, copied verbatim from c6db13ae023b2819a9b47fdb6b2f6c4325953fce.
-- Test fixture only; contains no user data.

-- 0001_foundation.sql
-- M1 foundation: projects and settings.

-- DM-1..4, DM-7. `key` is the task-key prefix (e.g. PRX in PRX-12). Timestamps are ISO 8601 UTC text.
CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    key         TEXT NOT NULL COLLATE NOCASE UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    color       TEXT NOT NULL DEFAULT 'blue',
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    is_general  INTEGER NOT NULL DEFAULT 0 CHECK (is_general IN (0, 1)),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- At most one General project (DM-4: it always exists and cannot be deleted).
CREATE UNIQUE INDEX projects_one_general ON projects (is_general) WHERE is_general = 1;

-- The id matches the UI prototype's sample data, so mock tasks still resolve during M1-M2.
INSERT INTO projects (id, name, key, description, color, status, is_general, created_at, updated_at)
VALUES ('p-general', 'General', 'GEN', 'Tasks and knowledge that are not specific to a project.',
        'slate', 'active', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- Non-secret app settings (UI-SET-*). One row per setting; `value` is JSON. API keys never go here (NFR-SEC-1).
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 0002_tasks.sql
-- M2: tasks (spec §4) with labels, key aliases, links, comments, daily history, and the activity log.

-- Task numbers per project come from this counter, so a number is never reused (§4.1), even after a delete.
ALTER TABLE projects ADD COLUMN next_task_number INTEGER NOT NULL DEFAULT 1;

CREATE TABLE tasks (
    id           TEXT PRIMARY KEY,
    key          TEXT NOT NULL COLLATE NOCASE UNIQUE,   -- <PROJECT-KEY>-<number>, e.g. PRX-12
    number       INTEGER NOT NULL,
    project_id   TEXT NOT NULL REFERENCES projects (id),
    title        TEXT NOT NULL,
    type         TEXT NOT NULL DEFAULT 'general' CHECK (type IN ('bug', 'feature', 'general')),
    description  TEXT NOT NULL DEFAULT '',                -- Markdown with mentions
    status       TEXT NOT NULL DEFAULT 'todo'
                 CHECK (status IN ('todo', 'in_progress', 'blocked', 'done', 'wont_do')),
    priority     TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('high', 'medium', 'low')),
    due_date     TEXT,                                    -- YYYY-MM-DD, local calendar date
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    completed_at TEXT                                     -- set on done / wont_do, cleared on reopen (FR-TASK-ST-4)
);
CREATE INDEX tasks_project_status ON tasks (project_id, status);
CREATE INDEX tasks_due_date ON tasks (due_date) WHERE due_date IS NOT NULL;

-- Free-text labels (§4.1), one row per label, for autocomplete and filtering.
CREATE TABLE task_labels (
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    label   TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (task_id, label)
);
CREATE INDEX task_labels_label ON task_labels (label);

-- Keys a task had before it moved to another project (FR-TASK-UI-6), so old mentions still resolve.
CREATE TABLE task_key_aliases (
    key     TEXT PRIMARY KEY COLLATE NOCASE,
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE
);
CREATE INDEX task_key_aliases_task ON task_key_aliases (task_id);

-- Task links (FR-TASK-LN-1..2, -4). For 'parent', from = parent and to = sub-task.
-- For 'relates' (symmetric) the core stores the pair in a fixed order, so A-B and B-A can't both exist.
CREATE TABLE task_links (
    id           TEXT PRIMARY KEY,
    type         TEXT NOT NULL CHECK (type IN ('relates', 'parent')),
    from_task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    to_task_id   TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    created_at   TEXT NOT NULL,
    CHECK (from_task_id <> to_task_id),
    UNIQUE (type, from_task_id, to_task_id)
);
CREATE UNIQUE INDEX task_links_one_parent ON task_links (to_task_id) WHERE type = 'parent';
CREATE INDEX task_links_to ON task_links (to_task_id);

-- Comments (FR-TASK-CM-1): timestamped Markdown, editable.
CREATE TABLE task_comments (
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT
);
CREATE INDEX task_comments_task ON task_comments (task_id, created_at);

-- Daily history (FR-TASK-HI-1..4): one snapshot per task per local calendar day, overwritten by each change.
CREATE TABLE task_versions (
    task_id  TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    day      TEXT NOT NULL,      -- YYYY-MM-DD, local time zone
    snapshot TEXT NOT NULL,      -- JSON TaskSnapshot
    PRIMARY KEY (task_id, day)
);

-- Recent activity for Home and project pages. ref_id points to a task now, to pages from M3 (no foreign key).
-- At most one 'task_updated' row per task per day; the core replaces it.
CREATE TABLE activity (
    id     TEXT PRIMARY KEY,
    kind   TEXT NOT NULL,        -- task_created | task_completed | task_updated (page_* from M3)
    ref_id TEXT NOT NULL,
    at     TEXT NOT NULL
);
CREATE INDEX activity_at ON activity (at);
CREATE INDEX activity_ref ON activity (ref_id);

-- 0003_knowledge.sql
-- M3: knowledge pages (spec §5) with groups, tags, daily history, task links, the mention index,
-- attachments, editor drafts and the keyword search index.

-- FR-KN-GR-1: one level. Names unique within a project (FR-KN-ED-12). DM-7: description is LLM context.
CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL REFERENCES projects (id),
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (project_id, name COLLATE NOCASE)
);

-- §5.1. Titles unique within a project (FR-KN-ED-12). The group must belong to the same project (checked in the core).
CREATE TABLE pages (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL REFERENCES projects (id),
    group_id       TEXT REFERENCES groups (id) ON DELETE SET NULL,   -- deleting a group ungroups its pages
    title          TEXT NOT NULL,
    body           TEXT NOT NULL DEFAULT '',                         -- Markdown; page mentions are [Title](page:<id>)
    source_info    TEXT,                                             -- optional free text (§5.1)
    origin         TEXT NOT NULL DEFAULT 'manual' CHECK (origin IN ('manual', 'file', 'task')),
    origin_task_id TEXT REFERENCES tasks (id) ON DELETE SET NULL,
    check_status   TEXT NOT NULL DEFAULT 'not_checked' CHECK (check_status IN ('checked', 'not_checked')),
    archived       INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE (project_id, title COLLATE NOCASE)
);
CREATE INDEX pages_project_group ON pages (project_id, group_id);
CREATE INDEX pages_updated ON pages (updated_at);

CREATE TABLE page_tags (
    page_id TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    tag     TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (page_id, tag)
);
CREATE INDEX page_tags_tag ON page_tags (tag);

-- FR-KN-ED-5: one snapshot per page per local day, overwritten by each change (as task_versions).
CREATE TABLE page_versions (
    page_id  TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    day      TEXT NOT NULL,
    snapshot TEXT NOT NULL,      -- JSON PageSnapshot
    PRIMARY KEY (page_id, day)
);

-- DM-5 / FR-TASK-LN-3: manual links between tasks and pages, from either side.
CREATE TABLE task_page_links (
    id         TEXT PRIMARY KEY,
    task_id    TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    page_id    TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    UNIQUE (task_id, page_id)
);
CREATE INDEX task_page_links_page ON task_page_links (page_id);

-- DM-MN-4 backlinks: who mentions whom, rebuilt for a source each time it is saved.
-- source_kind: page | task (description) | comment; target_kind: page | task. No foreign keys (mixed kinds);
-- the core removes rows when sources or targets are deleted. Derived data, but cheap and kept in exports.
CREATE TABLE mentions (
    source_kind TEXT NOT NULL CHECK (source_kind IN ('page', 'task', 'comment')),
    source_id   TEXT NOT NULL,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('page', 'task')),
    target_id   TEXT NOT NULL,
    PRIMARY KEY (source_kind, source_id, target_kind, target_id)
);
CREATE INDEX mentions_target ON mentions (target_kind, target_id);

-- FR-KN-ED-3, -11: pasted images, content-addressed files in <data>/attachments/<name>.
-- Referenced from Markdown as ![alt](attachment:<name>). Removed only when no page body, page version,
-- task description, comment or draft refers to them any more.
CREATE TABLE attachments (
    name       TEXT PRIMARY KEY,   -- <sha256 hex prefix>.<ext>
    mime       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

-- FR-KN-ED-10: unsaved editor content, saved every few seconds and on normal exit.
-- kind: page (existing page), new_page, task_description, comment (existing comment), new_comment (per task).
-- Not exported (the export skips it: see DERIVED_TABLES).
CREATE TABLE drafts (
    kind       TEXT NOT NULL,
    ref_id     TEXT NOT NULL,      -- page id, task id, comment id, or a client-chosen id for new items
    content    TEXT NOT NULL,      -- JSON chosen by the UI (e.g. { title, body, tags })
    updated_at TEXT NOT NULL,
    PRIMARY KEY (kind, ref_id)
);

-- FR-KN-UI-2 / FR-RAG-SR-1 (keyword part) / NFR-I18N-4: keyword index over pages and tasks.
-- Text is stored "spaced": a space is inserted between Han characters so unicode61 indexes each one;
-- queries are spaced the same way and run as phrases. Derived data: rebuilt after import.
CREATE VIRTUAL TABLE search_index USING fts5 (
    kind UNINDEXED,              -- page | task
    ref_id UNINDEXED,
    title,
    body,                        -- page body or task description + comments
    extra,                       -- tags / labels, source info, task key
    tokenize = 'unicode61 remove_diacritics 2'
);
-- Maps FTS rowids to pages/tasks: updates find a row by ref_id without scanning the index, and
-- searches read kind/ref_id from this small table instead of from the large FTS content rows (15x faster).
CREATE TABLE search_ref (
    rowid  INTEGER PRIMARY KEY,   -- = search_index.rowid
    kind   TEXT NOT NULL,         -- page | task
    ref_id TEXT NOT NULL UNIQUE
);

-- New activity kinds from M3: page_created, page_updated (one per page per day), task_commented.

-- 0004_ai.sql
-- M4: AI providers and models (spec §6.1, FR-LLM-EF-4) and the vector index (FR-RAG-ST-2, FR-RAG-IX-4).

-- FR-LLM-PV-1..7: one row per configured provider. API keys are never stored here (NFR-SEC-1):
-- they live in the OS secure store under the provider id.
CREATE TABLE providers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('azure_foundry', 'anthropic', 'openai_compatible', 'local')),
    endpoint    TEXT NOT NULL DEFAULT '',
    api_version TEXT,                                                 -- older Azure endpoint style only
    auth        TEXT NOT NULL CHECK (auth IN ('api_key', 'azure_cli', 'none')),
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)), -- FR-LLM-PV-6
    position    INTEGER NOT NULL DEFAULT 0,                           -- order of the cards in Settings
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (name COLLATE NOCASE)
);

-- Models offered by a provider (UI-SET-2). Effort columns follow FR-LLM-EF-1..4: NULL means "use the
-- built-in default", so a model works without any effort setup.
CREATE TABLE models (
    id             TEXT PRIMARY KEY,
    provider_id    TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    name           TEXT NOT NULL,                                     -- deployment / model name sent to the API
    label          TEXT NOT NULL,                                     -- shown in the model pickers
    format         TEXT NOT NULL CHECK (format IN ('chat_completions', 'responses', 'anthropic_messages')),
    vision         INTEGER NOT NULL DEFAULT 0 CHECK (vision IN (0, 1)),
    effort_enabled INTEGER NOT NULL DEFAULT 0 CHECK (effort_enabled IN (0, 1)), -- FR-LLM-EF-3
    effort_levels  TEXT,                                              -- JSON array; NULL = low, medium, high
    effort_default TEXT,                                              -- NULL = nothing is sent (FR-LLM-EF-1)
    effort_custom  TEXT,                                              -- JSON {level: value}; NULL = Auto (FR-LLM-EF-2)
    position       INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    UNIQUE (provider_id, label COLLATE NOCASE)
);
CREATE INDEX models_provider ON models (provider_id);

-- FR-RAG-IX-4 / FR-RAG-ST-2: the pieces of pages and tasks (and glossary entries, later) with their
-- vectors. Derived data (FR-DATA-7): not exported (export.rs DERIVED_TABLES), rebuilt after import.
CREATE TABLE chunks (
    id      INTEGER PRIMARY KEY,
    kind    TEXT NOT NULL CHECK (kind IN ('page', 'task', 'term')),
    ref_id  TEXT NOT NULL,
    seq     INTEGER NOT NULL,              -- order within the item, from 0
    heading TEXT NOT NULL DEFAULT '',      -- heading path, e.g. "Pre-start › Hoses"
    text    TEXT NOT NULL,                 -- the piece itself: shown as a snippet, sent to the LLM in M6
    hash    TEXT NOT NULL,                 -- of everything that goes into the vector (title, heading, text, terms)
    vector  BLOB,                          -- little-endian float32, normalised; NULL = waiting for the indexer
    UNIQUE (kind, ref_id, seq)
);
CREATE INDEX chunks_pending ON chunks (id) WHERE vector IS NULL;

-- Which model and dimensions made the vectors (FR-RAG-EM-2): keys `model` and `dims`.
-- A different model empties every vector, and the indexer fills them again.
CREATE TABLE vector_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- 0005_glossary.sql
-- M4: the glossary (spec §5.9, FR-KN-GL-1..8, D-36, D-38).

-- FR-KN-GL-1/2: one row per entry. project_id NULL = global; a project entry overrides a global entry
-- with the same term inside that project. Exported (FR-KN-GL-8).
CREATE TABLE glossary_entries (
    id          TEXT PRIMARY KEY,
    project_id  TEXT REFERENCES projects (id),
    term        TEXT NOT NULL,
    other_names TEXT NOT NULL DEFAULT '[]',                    -- JSON array of strings
    description TEXT NOT NULL DEFAULT '',
    page_id     TEXT REFERENCES pages (id) ON DELETE SET NULL, -- optional full explanation
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
-- One term per scope, ignoring case; a global entry and a project entry may share a term.
CREATE UNIQUE INDEX glossary_entries_scope_term
    ON glossary_entries (COALESCE(project_id, ''), term COLLATE NOCASE);

-- Which pages and tasks use which entry, by the FR-KN-GL-4 rules, with the entries that apply to
-- the item's project. Maintained by glossary.rs on every save; read by "Used in" (UI-KN-8) and to
-- re-index only the items an entry change affects (FR-RAG-IX-3). Derived data: not exported,
-- rebuilt after import.
CREATE TABLE glossary_uses (
    entry_id TEXT NOT NULL REFERENCES glossary_entries (id) ON DELETE CASCADE,
    kind     TEXT NOT NULL CHECK (kind IN ('page', 'task')),
    ref_id   TEXT NOT NULL,
    PRIMARY KEY (entry_id, kind, ref_id)
);
CREATE INDEX glossary_uses_item ON glossary_uses (kind, ref_id);

-- 0006_glossary_no_page_link.sql
-- M4: glossary entries no longer link to a page (owner's decision after using it, spec v1.18).
-- SQLite can't drop a column that has a foreign key, so the table is rebuilt. Dropping the old
-- table also empties glossary_uses (ON DELETE CASCADE); that index is derived and is rebuilt
-- right after this upgrade (db.rs rebuilds derived indexes when the schema version changes).
CREATE TABLE glossary_entries_new (
    id          TEXT PRIMARY KEY,
    project_id  TEXT REFERENCES projects (id),
    term        TEXT NOT NULL,
    other_names TEXT NOT NULL DEFAULT '[]',
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
INSERT INTO glossary_entries_new (id, project_id, term, other_names, description, created_at, updated_at)
    SELECT id, project_id, term, other_names, description, created_at, updated_at FROM glossary_entries;
DROP TABLE glossary_entries;
ALTER TABLE glossary_entries_new RENAME TO glossary_entries;
CREATE UNIQUE INDEX glossary_entries_scope_term
    ON glossary_entries (COALESCE(project_id, ''), term COLLATE NOCASE);

-- 0007_creation.sql
-- M5: the knowledge creation flows (spec §5.3–5.7, FR-KN-GL-7, FR-LLM-AS-5, D-41).

-- FR-LLM-AS-5 (built in M5): saved conversations. Exported.
CREATE TABLE conversations (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    scope      TEXT NOT NULL DEFAULT '[]',   -- JSON array of project ids; empty = everything (FR-LLM-AS-4)
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- One row per message, in `seq` order. `payload` is JSON for the parts only some messages have:
-- the attachment, processing steps, the draft card, glossary suggestions, an error, and in M6
-- citations and proposed actions (conversations.rs, `MessagePayload`). Exported.
CREATE TABLE messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    text            TEXT NOT NULL DEFAULT '',
    model_id        TEXT,   -- no foreign key: the model may be deleted later (its label is in the payload)
    effort          TEXT,
    payload         TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX messages_conversation_seq ON messages (conversation_id, seq);

-- FR-KN-UP-6: the text read from an uploaded file (OCR included), kept with its conversation so the
-- draft can still be revised from the source ("also include the safety section"). `text` becomes
-- NULL once the draft made from it is saved or discarded. The file itself is never kept. Exported.
CREATE TABLE chat_files (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    message_id      TEXT NOT NULL REFERENCES messages (id) ON DELETE CASCADE,  -- the user message with the file
    file_name       TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('text', 'pdf', 'docx', 'image')),
    size            INTEGER NOT NULL,
    pages           INTEGER NOT NULL DEFAULT 1,
    text            TEXT,
    created_at      TEXT NOT NULL
);
CREATE INDEX chat_files_conversation ON chat_files (conversation_id);

-- FR-KN-GL-7: suggested glossary terms the user ignored, per project; never suggested there again.
-- Exported (they are the user's decisions).
CREATE TABLE glossary_ignored (
    project_id TEXT NOT NULL REFERENCES projects (id),
    term       TEXT NOT NULL COLLATE NOCASE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (project_id, term)
);

-- FR-KN-UP-4a, UI-SET-2: optional context size of a model, in tokens. NULL = LiteLLM's known size,
-- or send the whole text and fall back when the model refuses.
ALTER TABLE models ADD COLUMN context_size INTEGER;

-- 0008_assistant.sql
-- M6, the Assistant (D-44).

-- FR-LLM-AS-12, UI-SET-2: the model's provider offers built-in web search. Set by the user; removed
-- automatically when the provider refuses web search, or (Chat Completions, local) ignores it.
ALTER TABLE models ADD COLUMN web_search INTEGER NOT NULL DEFAULT 0;

-- FR-LLM-AS-10: this conversation's history budget in tokens (*Widen for this conversation*).
-- NULL = the default in Settings › Models & defaults.
ALTER TABLE conversations ADD COLUMN history_budget INTEGER;

-- 0009_categories.sql
-- 0.9.0 (D-61): "project" becomes "category" everywhere, here in the schema and in the stored data.
-- Old databases, backups and exports (import migrates them, FR-DATA-4) all pass through this step.
-- SQLite rewrites the foreign keys, indexes and index expressions that use a renamed table or
-- column; the index names are changed here. Ids stay as they are (the General category keeps
-- 'p-general'), and so do the category keys in task keys.
ALTER TABLE projects RENAME TO categories;
ALTER TABLE tasks RENAME COLUMN project_id TO category_id;
ALTER TABLE groups RENAME COLUMN project_id TO category_id;
ALTER TABLE pages RENAME COLUMN project_id TO category_id;
ALTER TABLE glossary_entries RENAME COLUMN project_id TO category_id;
ALTER TABLE glossary_ignored RENAME COLUMN project_id TO category_id;

DROP INDEX projects_one_general;
CREATE UNIQUE INDEX categories_one_general ON categories (is_general) WHERE is_general = 1;
DROP INDEX tasks_project_status;
CREATE INDEX tasks_category_status ON tasks (category_id, status);
DROP INDEX pages_project_group;
CREATE INDEX pages_category_group ON pages (category_id, group_id);

-- The built-in description of General, unless the user changed it.
UPDATE categories SET description = 'Tasks and knowledge that are not specific to a category.'
 WHERE is_general = 1 AND description = 'Tasks and knowledge that are not specific to a project.';

-- JSON kept in columns: task history snapshots, the assistant's message parts and the editor's
-- unsaved drafts. Only object keys change: a quoted name directly followed by a colon is always a
-- key (a quote inside a JSON string is escaped), so nothing the user wrote is touched.
UPDATE task_versions SET snapshot = replace(snapshot, '"projectId":', '"categoryId":')
 WHERE instr(snapshot, '"projectId":') > 0;
UPDATE messages SET payload =
       replace(replace(replace(replace(payload,
           '"projectId":', '"categoryId":'),
           '"termsProjectId":', '"termsCategoryId":'),
           '"projectIds":', '"categoryIds":'),
           '"scopeProjectIds":', '"scopeCategoryIds":')
 WHERE instr(payload, 'roject') > 0;
UPDATE drafts SET content =
       replace(replace(replace(content,
           '"projectId":', '"categoryId":'),
           '"projectIds":', '"categoryIds":'),
           '"scopeProjectIds":', '"scopeCategoryIds":')
 WHERE instr(content, 'roject') > 0;

PRAGMA user_version = 9;
