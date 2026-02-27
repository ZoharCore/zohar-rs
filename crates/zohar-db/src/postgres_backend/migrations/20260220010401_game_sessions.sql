CREATE SCHEMA IF NOT EXISTS game;

CREATE TABLE IF NOT EXISTS game.sessions (
    username TEXT PRIMARY KEY,
    server_id TEXT,
    connection_id TEXT,
    last_heartbeat TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    login_token BIGINT,
    login_issued_at TIMESTAMPTZ,
    peer_ip TEXT,
    state TEXT NOT NULL DEFAULT 'ACTIVE',
    CHECK (state IN ('AUTHED', 'ACTIVE'))
);

CREATE INDEX IF NOT EXISTS idx_game_sessions_login_token
ON game.sessions (login_token);
