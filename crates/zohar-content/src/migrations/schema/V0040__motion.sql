-- Minimal motion model for movement timing lookup.

CREATE TABLE enum_motion_mode (value TEXT PRIMARY KEY);
CREATE TABLE enum_motion_set_kind (value TEXT PRIMARY KEY);
CREATE TABLE enum_motion_action (value TEXT PRIMARY KEY);

CREATE TABLE motion_set (
  motion_set_id INTEGER PRIMARY KEY,
  set_kind TEXT NOT NULL REFERENCES enum_motion_set_kind(value)
);

CREATE TABLE motion_set_mob (
  motion_set_id INTEGER NOT NULL REFERENCES motion_set(motion_set_id),
  mob_id INTEGER PRIMARY KEY REFERENCES mob_proto(mob_id)
);
CREATE INDEX motion_set_mob_set_idx ON motion_set_mob(motion_set_id);

CREATE TABLE player_motion_profile (
  profile_id INTEGER PRIMARY KEY,
  legacy_race_num INTEGER NOT NULL UNIQUE,
  player_class TEXT NOT NULL REFERENCES enum_player_class(value),
  gender TEXT NOT NULL REFERENCES enum_gender(value)
);

CREATE TABLE motion_set_player_profile (
  motion_set_id INTEGER NOT NULL REFERENCES motion_set(motion_set_id),
  profile_id INTEGER PRIMARY KEY REFERENCES player_motion_profile(profile_id)
);
CREATE INDEX motion_set_player_profile_set_idx ON motion_set_player_profile(motion_set_id);

-- Legacy index values are normalized to enum_motion_action.
-- 1 WAIT, 2 WALK, 3 RUN, 5 DAMAGE, 6 DAMAGE_FLYING, 7 STAND_UP,
-- 8 DAMAGE_BACK, 9 DAMAGE_FLYING_BACK, 10 STAND_UP_BACK, 11 DEAD,
-- 12 DEAD_BACK, 13 NORMAL_ATTACK, 14 COMBO_ATTACK_1, 15 COMBO_ATTACK_2,
-- 16 COMBO_ATTACK_3, 25 SPAWN, 32 STOP, 33 SPECIAL_1, 34 SPECIAL_2,
-- 35 SPECIAL_3, 36 SPECIAL_4, 37 SPECIAL_5, 38 SPECIAL_6,
-- 171 SKILL_1, 172 SKILL_2, 173 SKILL_3, 174 SKILL_4, 175 SKILL_5.
CREATE TABLE motion_entry (
  motion_id INTEGER PRIMARY KEY,
  motion_set_id INTEGER NOT NULL REFERENCES motion_set(motion_set_id),
  motion_mode TEXT NOT NULL REFERENCES enum_motion_mode(value),
  motion_action TEXT NOT NULL REFERENCES enum_motion_action(value),
  duration_ms INTEGER NOT NULL,
  accum_x REAL,
  accum_y REAL,
  source TEXT NOT NULL
);

CREATE INDEX motion_entry_set_idx ON motion_entry(motion_set_id, motion_mode, motion_action);
