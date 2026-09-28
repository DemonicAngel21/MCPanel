-- v0.2: automatic restart policy and crash history.

CREATE TABLE server_restart_policies (
    server_id    TEXT PRIMARY KEY NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    enabled      INTEGER NOT NULL,
    max_attempts INTEGER NOT NULL,
    window_secs  INTEGER NOT NULL,
    delay_secs   INTEGER NOT NULL,
    stable_secs  INTEGER NOT NULL,
    crash_backup INTEGER NOT NULL
);

CREATE TABLE crash_events (
    id                TEXT PRIMARY KEY NOT NULL,
    server_id         TEXT NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    occurred_at       INTEGER NOT NULL,
    exit_code         INTEGER,
    kind              TEXT NOT NULL,
    message           TEXT NOT NULL,
    attempt           INTEGER NOT NULL,
    action            TEXT NOT NULL CHECK (action IN ('restart', 'gave_up', 'not_restartable', 'disabled')),
    restart_at        INTEGER,
    crash_report      TEXT,
    console_tail_json TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX crash_events_server_idx ON crash_events (server_id, occurred_at DESC);
