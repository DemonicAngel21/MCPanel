-- v0.3: notification inbox.

CREATE TABLE notifications (
    id         TEXT PRIMARY KEY NOT NULL,
    created_at INTEGER NOT NULL,
    server_id  TEXT REFERENCES servers (id) ON DELETE CASCADE,
    category   TEXT NOT NULL,
    severity   TEXT NOT NULL CHECK (severity IN ('info', 'success', 'warning', 'error')),
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    read       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX notifications_created_idx ON notifications (created_at DESC);
