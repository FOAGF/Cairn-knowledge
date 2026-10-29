-- FR-LLM-AS-13 (D-68): conversations are archived instead of deleted. An archived conversation is
-- deleted for good three days after `archived_at`. NULL = not archived.
ALTER TABLE conversations ADD COLUMN archived_at TEXT;
