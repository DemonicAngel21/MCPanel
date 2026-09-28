-- MCPanel schema v1 (MVP).
-- Minecraft configuration (server.properties, ops, whitelist, bans) is NOT stored here:
-- the server's own files are the source of truth (ADR-0005).
-- Timestamps are UTC unix milliseconds. IDs are UUIDv7 text.

CREATE TABLE java_runtimes (
    id               TEXT PRIMARY KEY NOT NULL,
    path             TEXT NOT NULL,
    major            INTEGER NOT NULL,
    version          TEXT NOT NULL,
    vendor           TEXT,
    arch             TEXT,
    is_64bit         INTEGER NOT NULL,
    source           TEXT NOT NULL CHECK (source IN ('detected', 'manual', 'managed')),
    valid            INTEGER NOT NULL,
    validation_error TEXT,
    validated_at     INTEGER NOT NULL
);
CREATE UNIQUE INDEX java_runtimes_path_uq ON java_runtimes (path COLLATE NOCASE);

CREATE TABLE servers (
    id                     TEXT PRIMARY KEY NOT NULL,
    name                   TEXT NOT NULL,
    directory              TEXT NOT NULL,
    software_id            TEXT NOT NULL,
    game_version           TEXT NOT NULL,
    build                  TEXT,
    jar                    TEXT NOT NULL,
    java_min_major         INTEGER,
    java_recommended_major INTEGER,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL
);
CREATE UNIQUE INDEX servers_directory_uq ON servers (directory COLLATE NOCASE);

CREATE TABLE server_launch_configs (
    server_id         TEXT PRIMARY KEY NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    java_runtime_id   TEXT REFERENCES java_runtimes (id) ON DELETE SET NULL,
    min_memory_mb     INTEGER NOT NULL,
    max_memory_mb     INTEGER NOT NULL,
    jvm_args_json     TEXT NOT NULL DEFAULT '[]',
    server_args_json  TEXT NOT NULL DEFAULT '[]',
    stop_timeout_secs INTEGER NOT NULL DEFAULT 60
);

-- Process bookkeeping for orphan detection after an MCPanel crash.
CREATE TABLE server_runtime_state (
    server_id          TEXT PRIMARY KEY NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    last_state         TEXT,
    pid                INTEGER,
    process_start_time INTEGER,
    last_started_at    INTEGER,
    last_ready_at      INTEGER,
    last_stopped_at    INTEGER,
    last_exit_code     INTEGER
);

CREATE TABLE jobs (
    id            TEXT PRIMARY KEY NOT NULL,
    kind          TEXT NOT NULL,
    server_id     TEXT,
    status        TEXT NOT NULL,
    progress      REAL,
    message       TEXT,
    created_at    INTEGER NOT NULL,
    started_at    INTEGER,
    finished_at   INTEGER,
    error_code    TEXT,
    error_message TEXT,
    result_json   TEXT
);
CREATE INDEX jobs_created_idx ON jobs (created_at DESC);

-- Append-only audit log. Never contains secrets. server_id intentionally has no FK so
-- history survives server deletion.
CREATE TABLE audit_events (
    id            TEXT PRIMARY KEY NOT NULL,
    occurred_at   INTEGER NOT NULL,
    actor         TEXT NOT NULL,
    action        TEXT NOT NULL,
    server_id     TEXT,
    target        TEXT,
    result        TEXT NOT NULL CHECK (result IN ('success', 'failure')),
    metadata_json TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX audit_events_time_idx ON audit_events (occurred_at DESC);
CREATE INDEX audit_events_server_idx ON audit_events (server_id, occurred_at DESC);

CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
