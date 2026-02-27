-- Minimal mob rows needed for NPC spawn materialization.

CREATE TABLE enum_mob_type (value TEXT PRIMARY KEY);
CREATE TABLE enum_mob_rank (value TEXT PRIMARY KEY);

CREATE TABLE mob_proto (
  mob_id INTEGER PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  mob_type TEXT NOT NULL REFERENCES enum_mob_type(value),
  rank TEXT NOT NULL REFERENCES enum_mob_rank(value),
  level INTEGER NOT NULL DEFAULT 1,
  move_speed INTEGER NOT NULL DEFAULT 100,
  attack_speed INTEGER NOT NULL DEFAULT 100
);

CREATE TABLE mob_chat_strategy (
  chat_context TEXT NOT NULL,
  mob_type TEXT REFERENCES enum_mob_type(value),
  mob_id INTEGER REFERENCES mob_proto(mob_id),
  interval_min_sec INTEGER NOT NULL,
  interval_max_sec INTEGER NOT NULL,
  CHECK (interval_min_sec >= 1),
  CHECK (interval_max_sec >= interval_min_sec),
  CHECK (
    (mob_type IS NOT NULL AND mob_id IS NULL) OR
    (mob_type IS NULL AND mob_id IS NOT NULL)
  )
);

CREATE UNIQUE INDEX mob_chat_strategy_type_scope
  ON mob_chat_strategy (chat_context, mob_type)
  WHERE mob_type IS NOT NULL AND mob_id IS NULL;

CREATE UNIQUE INDEX mob_chat_strategy_mob_scope
  ON mob_chat_strategy (chat_context, mob_id)
  WHERE mob_type IS NULL AND mob_id IS NOT NULL;

CREATE TABLE mob_chat_line (
  mob_id INTEGER NOT NULL REFERENCES mob_proto(mob_id),
  chat_context TEXT NOT NULL,
  source_key TEXT NOT NULL,
  text TEXT NOT NULL,
  CHECK (length(trim(source_key)) > 0),
  CHECK (length(trim(text)) > 0),
  UNIQUE (mob_id, chat_context, source_key)
);
