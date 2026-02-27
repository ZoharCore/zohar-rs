-- P0 minimal core schema for NPC map spawns + motion loading.

CREATE TABLE content_meta (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  schema_version_major INTEGER NOT NULL,
  schema_version_minor INTEGER NOT NULL,
  schema_version_patch INTEGER NOT NULL,
  schema_hash TEXT,
  validated INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
