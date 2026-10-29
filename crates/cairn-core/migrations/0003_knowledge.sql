-- Knowledge pages (spec §5) with groups, tags, daily history, task links, the mention index,
-- attachments, editor drafts and the keyword search index.

-- FR-KN-GR-1: one level. Names unique within a category (FR-KN-ED-12). DM-7: description is LLM context.
CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    category_id TEXT NOT NULL REFERENCES categories (id),
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (category_id, name COLLATE NOCASE)
);

-- §5.1. Titles unique within a category (FR-KN-ED-12). The group must belong to the same category (checked in the core).
CREATE TABLE pages (
    id             TEXT PRIMARY KEY,
    category_id    TEXT NOT NULL REFERENCES categories (id),
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
    UNIQUE (category_id, title COLLATE NOCASE)
);
CREATE INDEX pages_category_group ON pages (category_id, group_id);
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
