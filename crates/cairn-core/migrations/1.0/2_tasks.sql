-- Tasks (spec §4) with labels, key aliases, links, comments, daily history, and the activity log.
-- Part of the schema at 1.0 (D-86); see 1_foundation.sql.

CREATE TABLE tasks (
    id           TEXT PRIMARY KEY,
    key          TEXT NOT NULL COLLATE NOCASE UNIQUE,   -- <CATEGORY-KEY>-<number>, e.g. PRX-12
    number       INTEGER NOT NULL,
    category_id  TEXT NOT NULL REFERENCES categories (id),
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
CREATE INDEX tasks_category_status ON tasks (category_id, status);
CREATE INDEX tasks_due_date ON tasks (due_date) WHERE due_date IS NOT NULL;

-- Free-text labels (§4.1), one row per label, for autocomplete and filtering.
CREATE TABLE task_labels (
    task_id TEXT NOT NULL REFERENCES tasks (id) ON DELETE CASCADE,
    label   TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (task_id, label)
);
CREATE INDEX task_labels_label ON task_labels (label);

-- Keys a task had before it moved to another category (FR-TASK-UI-6), so old mentions still resolve.
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

-- Recent activity for Home and category pages. ref_id points to a task or a page (no foreign key).
-- At most one 'task_updated' row per task per day, and one 'page_updated' per page per day; the core replaces it.
CREATE TABLE activity (
    id     TEXT PRIMARY KEY,
    kind   TEXT NOT NULL,        -- task_created | task_completed | task_updated | task_commented | page_created | page_updated
    ref_id TEXT NOT NULL,
    at     TEXT NOT NULL
);
CREATE INDEX activity_at ON activity (at);
CREATE INDEX activity_ref ON activity (ref_id);
