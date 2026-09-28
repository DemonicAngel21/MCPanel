-- v0.2: player play-session history, recorded from console join/leave events.
-- Operators, whitelist and bans are NOT stored here: the server's own files are the
-- source of truth (ADR-0005). Player IP addresses are never recorded.

CREATE TABLE player_sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    server_id   TEXT NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    player_name TEXT NOT NULL,
    joined_at   INTEGER NOT NULL,
    left_at     INTEGER,
    -- 1 when MCPanel could not observe the end (it was closed); not counted as play time.
    interrupted INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX player_sessions_server_player_idx ON player_sessions (server_id, player_name COLLATE NOCASE);
CREATE INDEX player_sessions_open_idx ON player_sessions (server_id) WHERE left_at IS NULL;
