-- D-94: when each task and page was last opened, or picked from a `#` or `[[` suggestion list, so
-- those lists show the most recently opened first. One row per item; the item's own `updated_at`
-- never changes for it. A deleted task or page takes its row with it.
CREATE TABLE opened (
    kind       TEXT NOT NULL CHECK (kind IN ('task', 'page')),
    item_id    TEXT NOT NULL,
    opened_at  TEXT NOT NULL,
    PRIMARY KEY (kind, item_id)
) WITHOUT ROWID;

CREATE INDEX opened_recent ON opened (kind, opened_at);

CREATE TRIGGER opened_task_deleted AFTER DELETE ON tasks BEGIN
    DELETE FROM opened WHERE kind = 'task' AND item_id = OLD.id;
END;

CREATE TRIGGER opened_page_deleted AFTER DELETE ON pages BEGIN
    DELETE FROM opened WHERE kind = 'page' AND item_id = OLD.id;
END;
