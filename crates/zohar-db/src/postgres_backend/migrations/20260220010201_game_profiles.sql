CREATE SCHEMA IF NOT EXISTS game;

CREATE TABLE IF NOT EXISTS game.profiles (
    username TEXT PRIMARY KEY,
    empire TEXT,
    delete_code TEXT NOT NULL DEFAULT '1234567',
    banned_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (char_length(delete_code) = 7),
    CHECK (empire IN ('RED', 'YELLOW', 'BLUE') OR empire IS NULL)
);

INSERT INTO game.profiles (username, empire)
VALUES ('admin', 'BLUE'),
       ('guest', 'BLUE'),
       ('guest2', NULL),
       ('guest3', NULL)
ON CONFLICT (username) DO NOTHING;
