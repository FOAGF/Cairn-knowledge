-- Saved conversations of the Assistant and of the knowledge creation flows (spec §5.3–5.7, FR-KN-GL-7,
-- FR-LLM-AS-5, D-41), and ignored glossary suggestions.

-- FR-LLM-AS-5: saved conversations. Exported.
CREATE TABLE conversations (
    id         TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    scope      TEXT NOT NULL DEFAULT '[]',   -- JSON array of category ids; empty = everything (FR-LLM-AS-4)
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    -- FR-LLM-AS-10: this conversation's history budget in tokens (*Widen for this conversation*).
    -- NULL = the default in Settings › Models & defaults.
    history_budget INTEGER
);

-- One row per message, in `seq` order. `payload` is JSON for the parts only some messages have:
-- the attachment, processing steps, the draft card, glossary suggestions, an error, citations,
-- proposed actions, and the effort sent and reasoning tokens reported (conversations.rs, `Payload`).
-- Exported.
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

-- FR-KN-GL-7: suggested glossary terms the user ignored, per category; never suggested there again.
-- Exported (they are the user's decisions).
CREATE TABLE glossary_ignored (
    category_id TEXT NOT NULL REFERENCES categories (id),
    term        TEXT NOT NULL COLLATE NOCASE,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (category_id, term)
);
