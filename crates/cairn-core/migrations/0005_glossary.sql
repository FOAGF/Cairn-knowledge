-- The glossary (spec §5.9, FR-KN-GL-1..8, D-36, D-38).

-- FR-KN-GL-1/2: one row per entry. category_id NULL = global; a category entry overrides a global entry
-- with the same term inside that category. Exported (FR-KN-GL-8).
CREATE TABLE glossary_entries (
    id          TEXT PRIMARY KEY,
    category_id TEXT REFERENCES categories (id),
    term        TEXT NOT NULL,
    other_names TEXT NOT NULL DEFAULT '[]',                    -- JSON array of strings
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
-- One term per scope, ignoring case; a global entry and a category entry may share a term.
CREATE UNIQUE INDEX glossary_entries_scope_term
    ON glossary_entries (COALESCE(category_id, ''), term COLLATE NOCASE);

-- Which pages and tasks use which entry, by the FR-KN-GL-4 rules, with the entries that apply to
-- the item's category. Maintained by glossary.rs on every save; read by "Used in" (UI-KN-8) and to
-- re-index only the items an entry change affects (FR-RAG-IX-3). Derived data: not exported,
-- rebuilt after import.
CREATE TABLE glossary_uses (
    entry_id TEXT NOT NULL REFERENCES glossary_entries (id) ON DELETE CASCADE,
    kind     TEXT NOT NULL CHECK (kind IN ('page', 'task')),
    ref_id   TEXT NOT NULL,
    PRIMARY KEY (entry_id, kind, ref_id)
);
CREATE INDEX glossary_uses_item ON glossary_uses (kind, ref_id);
