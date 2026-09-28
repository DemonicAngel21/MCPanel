-- v0.2: local backups, schedules and retention.
-- A backup record outlives its server (server_id becomes NULL); the archive file is
-- the user's data and is never deleted implicitly.

CREATE TABLE backups (
    id                 TEXT PRIMARY KEY NOT NULL,
    server_id          TEXT REFERENCES servers (id) ON DELETE SET NULL,
    server_name        TEXT NOT NULL,
    kind               TEXT NOT NULL CHECK (kind IN ('manual', 'scheduled', 'pre_restore')),
    status             TEXT NOT NULL CHECK (status IN ('creating', 'ready', 'failed')),
    path               TEXT NOT NULL,
    created_at         INTEGER NOT NULL,
    finished_at        INTEGER,
    size_bytes         INTEGER NOT NULL DEFAULT 0,
    content_bytes      INTEGER NOT NULL DEFAULT 0,
    file_count         INTEGER NOT NULL DEFAULT 0,
    sha256             TEXT,
    live               INTEGER NOT NULL DEFAULT 0,
    contains_sensitive INTEGER NOT NULL DEFAULT 0,
    software_id        TEXT NOT NULL,
    game_version       TEXT NOT NULL,
    note               TEXT,
    protected          INTEGER NOT NULL DEFAULT 0,
    skipped_json       TEXT NOT NULL DEFAULT '[]',
    error_message      TEXT
);
CREATE INDEX backups_server_created_idx ON backups (server_id, created_at DESC);

-- Schedule and GFS retention per server (spec: backup_policies + backup_schedules,
-- combined while only the local destination exists).
CREATE TABLE backup_policies (
    server_id        TEXT PRIMARY KEY NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    enabled          INTEGER NOT NULL,
    interval_minutes INTEGER NOT NULL CHECK (interval_minutes > 0),
    skip_if_idle     INTEGER NOT NULL,
    keep_last        INTEGER NOT NULL,
    keep_daily       INTEGER NOT NULL,
    keep_weekly      INTEGER NOT NULL,
    keep_monthly     INTEGER NOT NULL,
    last_run_at      INTEGER
);
