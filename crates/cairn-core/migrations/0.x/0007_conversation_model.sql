-- FR-LLM-ST-2 (D-67): the model and effort picked in a conversation are kept with it, so switching
-- conversations or restarting brings them back. No foreign key: the model may be deleted later (the
-- Assistant then falls back to an enabled model). Effort NULL = not set.
ALTER TABLE conversations ADD COLUMN model_id TEXT;
ALTER TABLE conversations ADD COLUMN effort TEXT;

-- Conversations from before keep the model and effort of their last message that names a model.
UPDATE conversations SET
    model_id = (SELECT m.model_id FROM messages m
                 WHERE m.conversation_id = conversations.id AND m.model_id IS NOT NULL
                 ORDER BY m.seq DESC LIMIT 1),
    effort   = (SELECT m.effort FROM messages m
                 WHERE m.conversation_id = conversations.id AND m.model_id IS NOT NULL
                 ORDER BY m.seq DESC LIMIT 1);
