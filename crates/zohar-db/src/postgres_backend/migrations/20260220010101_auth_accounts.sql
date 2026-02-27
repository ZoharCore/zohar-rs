CREATE SCHEMA IF NOT EXISTS auth;

CREATE TABLE IF NOT EXISTS auth.accounts (
    username TEXT PRIMARY KEY,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO auth.accounts (username, password_hash)
VALUES ('admin', 'pw'),
       ('guest', 'pw'),
       ('guest2', 'pw'),
       ('guest3', 'pw')
ON CONFLICT (username) DO NOTHING;
