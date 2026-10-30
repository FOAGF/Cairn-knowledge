-- D-72: external knowledge mode (spec §5.11). In external mode `pages` and `groups` are an index of
-- a folder of Markdown files, matched to each file and folder by its path.
--
-- `pages` and `groups` have inline UNIQUE constraints that SQLite can't drop, so both tables are
-- rebuilt. This step runs with foreign keys off (migrations.rs), as SQLite's procedure for such
-- changes requires: with them on, dropping the old tables would delete the tags, history and task
-- links of every page and ungroup every page. `PRAGMA foreign_key_check` must pass before it commits.

-- FR-KN-EXT-5: the category's folder (its name, directly in the root) once it is made or found.
ALTER TABLE categories ADD COLUMN folder TEXT;
CREATE UNIQUE INDEX categories_folder ON categories (folder) WHERE folder IS NOT NULL;

-- FR-KN-GR-1 / FR-KN-EXT-2: groups can hold groups in external mode (`parent_id`); `path` is the
-- group's folder relative to the root, `/`-separated (NULL in internal mode). Deleting a group row
-- deletes the rows of the groups inside it (the core moves their contents first when the user deletes one).
CREATE TABLE groups_new (
    id          TEXT PRIMARY KEY,
    category_id TEXT NOT NULL REFERENCES categories (id),
    parent_id   TEXT REFERENCES groups (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    path        TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
INSERT INTO groups_new (id, category_id, name, description, created_at, updated_at)
    SELECT id, category_id, name, description, created_at, updated_at FROM groups;

-- §5.1, FR-KN-EXT-4: `path` is the page's file relative to the root, `/`-separated (NULL in internal
-- mode); `file_hash` (SHA-256 of the file's bytes), `file_mtime` (nanoseconds since 1970) and
-- `file_size` are the file as last read or written, so a scan reads only changed files.
-- `origin = 'folder'`: found in the folder rather than made in Cairn.
CREATE TABLE pages_new (
    id             TEXT PRIMARY KEY,
    category_id    TEXT NOT NULL REFERENCES categories (id),
    group_id       TEXT REFERENCES groups (id) ON DELETE SET NULL,   -- deleting a group ungroups its pages
    title          TEXT NOT NULL,
    body           TEXT NOT NULL DEFAULT '',                         -- Markdown; page mentions are [Title](page:<id>)
    source_info    TEXT,                                             -- optional free text (§5.1)
    origin         TEXT NOT NULL DEFAULT 'manual' CHECK (origin IN ('manual', 'file', 'task', 'folder')),
    origin_task_id TEXT REFERENCES tasks (id) ON DELETE SET NULL,
    check_status   TEXT NOT NULL DEFAULT 'not_checked' CHECK (check_status IN ('checked', 'not_checked')),
    archived       INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    path           TEXT,
    file_hash      TEXT,
    file_mtime     INTEGER,
    file_size      INTEGER
);
INSERT INTO pages_new (id, category_id, group_id, title, body, source_info, origin, origin_task_id,
                       check_status, archived, created_at, updated_at)
    SELECT id, category_id, group_id, title, body, source_info, origin, origin_task_id,
           check_status, archived, created_at, updated_at FROM pages;

DROP TABLE pages;
DROP TABLE groups;
ALTER TABLE groups_new RENAME TO groups;
ALTER TABLE pages_new RENAME TO pages;

-- Uniqueness (D-72 contract): internal rows keep their rules (FR-KN-ED-12: titles unique within a
-- category, group names unique within a category); external rows are unique by path, exactly as the
-- file system has it.
CREATE UNIQUE INDEX groups_name_internal ON groups (category_id, name COLLATE NOCASE) WHERE path IS NULL;
CREATE UNIQUE INDEX groups_path ON groups (path) WHERE path IS NOT NULL;
CREATE INDEX groups_parent ON groups (parent_id);

CREATE UNIQUE INDEX pages_title_internal ON pages (category_id, title COLLATE NOCASE) WHERE path IS NULL;
CREATE UNIQUE INDEX pages_path ON pages (path) WHERE path IS NOT NULL;
CREATE INDEX pages_category_group ON pages (category_id, group_id);
CREATE INDEX pages_updated ON pages (updated_at);

-- FR-KN-EXT-4: pages and groups whose file or folder disappeared from the knowledge folder, kept
-- for 30 days with their Cairn-only data, so one that comes back (a sync that deletes and makes
-- again, a git checkout away and back, a drive or cloud folder back online) gets its old id and
-- data again, matched by path or, for a page, by its file's hash. Apart from `pages` and `groups`
-- so no other query sees them. No foreign keys: what they refer to may be gone.
CREATE TABLE removed_pages (
    id             TEXT PRIMARY KEY,
    path           TEXT NOT NULL,
    file_hash      TEXT,
    origin         TEXT NOT NULL,
    origin_task_id TEXT,
    check_status   TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    versions       TEXT NOT NULL DEFAULT '[]',   -- JSON [[day, snapshot], ...]
    task_links     TEXT NOT NULL DEFAULT '[]',   -- JSON [[link id, task id, created_at], ...]
    removed_at     TEXT NOT NULL
);
CREATE INDEX removed_pages_path ON removed_pages (path);
CREATE INDEX removed_pages_hash ON removed_pages (file_hash);

CREATE TABLE removed_groups (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    description TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    removed_at  TEXT NOT NULL
);
CREATE INDEX removed_groups_path ON removed_groups (path);
