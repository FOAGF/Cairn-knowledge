-- D-69: DeepSeek is a provider kind of its own (Cairn runs its web search through DeepSeek's Search
-- and Fetch API). SQLite can't change a CHECK constraint, so `providers` is rebuilt. Dropping it would
-- also delete every model (ON DELETE CASCADE; foreign keys stay on inside a migration), so the models
-- are kept aside and put back.
CREATE TEMP TABLE models_kept AS SELECT * FROM models;

CREATE TABLE providers_new (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL
                CHECK (kind IN ('azure_foundry', 'anthropic', 'openai_compatible', 'local', 'deepseek')),
    endpoint    TEXT NOT NULL DEFAULT '',
    api_version TEXT,                                                 -- older Azure endpoint style only
    auth        TEXT NOT NULL CHECK (auth IN ('api_key', 'azure_cli', 'none')),
    enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)), -- FR-LLM-PV-6
    position    INTEGER NOT NULL DEFAULT 0,                           -- order of the cards in Settings
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    UNIQUE (name COLLATE NOCASE)
);
INSERT INTO providers_new (id, name, kind, endpoint, api_version, auth, enabled, position, created_at, updated_at)
    SELECT id, name, kind, endpoint, api_version, auth, enabled, position, created_at, updated_at FROM providers;
DROP TABLE providers;
ALTER TABLE providers_new RENAME TO providers;

INSERT INTO models SELECT * FROM models_kept;
DROP TABLE models_kept;
