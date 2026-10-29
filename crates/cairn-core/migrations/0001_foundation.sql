-- Foundation: categories and settings.

-- DM-1..4, DM-7. `key` is the task-key prefix (e.g. PRX in PRX-12). Timestamps are ISO 8601 UTC text.
CREATE TABLE categories (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    key              TEXT NOT NULL COLLATE NOCASE UNIQUE,
    description      TEXT NOT NULL DEFAULT '',
    color            TEXT NOT NULL DEFAULT 'blue',
    status           TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived')),
    is_general       INTEGER NOT NULL DEFAULT 0 CHECK (is_general IN (0, 1)),
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    -- Task numbers per category come from this counter, so a number is never reused (§4.1), even after a delete.
    next_task_number INTEGER NOT NULL DEFAULT 1
);

-- At most one General category (DM-4: it always exists and cannot be deleted).
CREATE UNIQUE INDEX categories_one_general ON categories (is_general) WHERE is_general = 1;

INSERT INTO categories (id, name, key, description, color, status, is_general, created_at, updated_at)
VALUES ('p-general', 'General', 'GEN', 'Tasks and knowledge that are not specific to a category.',
        'slate', 'active', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

-- Non-secret app settings (UI-SET-*). One row per setting; `value` is JSON. API keys never go here (NFR-SEC-1).
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
