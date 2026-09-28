-- v0.2: installed plugins/mods (MCPanel's record of files it installed or identified)
-- and content changes queued while a server runs. The files themselves stay in the
-- server folder; a missing record just means "unmanaged".

CREATE TABLE installed_content (
    server_id      TEXT NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    kind           TEXT NOT NULL CHECK (kind IN ('plugin', 'mod')),
    file_name      TEXT NOT NULL COLLATE NOCASE,
    name           TEXT NOT NULL,
    version_number TEXT,
    provider       TEXT,
    project_id     TEXT,
    version_id     TEXT,
    sha512         TEXT,
    installed_at   INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    PRIMARY KEY (server_id, kind, file_name)
);

CREATE TABLE pending_changes (
    id          TEXT PRIMARY KEY NOT NULL,
    server_id   TEXT NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('plugin', 'mod')),
    change_json TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);
CREATE INDEX pending_changes_server_idx ON pending_changes (server_id, created_at);
