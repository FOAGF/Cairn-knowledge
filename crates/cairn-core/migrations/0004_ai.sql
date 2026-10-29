-- AI providers and models (spec §6.1, FR-LLM-EF-4) and the vector index (FR-RAG-ST-2, FR-RAG-IX-4).

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
    -- FR-KN-UP-4a, UI-SET-2: optional context size, in tokens. NULL = LiteLLM's known size, or send the
    -- whole text and fall back when the model refuses.
    context_size   INTEGER,
    -- FR-LLM-AS-12, UI-SET-2: the provider offers built-in web search. Set by the user; removed
    -- automatically when the provider refuses web search, or (Chat Completions, local) ignores it.
    web_search     INTEGER NOT NULL DEFAULT 0,
    UNIQUE (provider_id, label COLLATE NOCASE)
);
CREATE INDEX models_provider ON models (provider_id);

-- FR-RAG-IX-4 / FR-RAG-ST-2: the pieces of pages, tasks and glossary entries with their
-- vectors. Derived data (FR-DATA-7): not exported (export.rs DERIVED_TABLES), rebuilt after import.
CREATE TABLE chunks (
    id      INTEGER PRIMARY KEY,
    kind    TEXT NOT NULL CHECK (kind IN ('page', 'task', 'term')),
    ref_id  TEXT NOT NULL,
    seq     INTEGER NOT NULL,              -- order within the item, from 0
    heading TEXT NOT NULL DEFAULT '',      -- heading path, e.g. "Pre-start › Hoses"
    text    TEXT NOT NULL,                 -- the piece itself: shown as a snippet and sent to the LLM
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
