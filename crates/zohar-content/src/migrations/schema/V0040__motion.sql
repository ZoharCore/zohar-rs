-- Minimal motion model for movement timing lookup.

CREATE TABLE enum_motion_mode (value TEXT PRIMARY KEY);
CREATE TABLE enum_motion_set_kind (value TEXT PRIMARY KEY);
CREATE TABLE enum_motion_action (value TEXT PRIMARY KEY);

CREATE TABLE motion_set (
  motion_set_id TEXT PRIMARY KEY,
  set_kind TEXT NOT NULL REFERENCES enum_motion_set_kind(value)
);

CREATE TABLE motion_set_mob (
  motion_set_id TEXT NOT NULL REFERENCES motion_set(motion_set_id),
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
  motion_set_id TEXT NOT NULL REFERENCES motion_set(motion_set_id),
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
  motion_set_id TEXT NOT NULL REFERENCES motion_set(motion_set_id),
  motion_mode TEXT NOT NULL REFERENCES enum_motion_mode(value),
  motion_action TEXT NOT NULL REFERENCES enum_motion_action(value),
  variant_index INTEGER NOT NULL DEFAULT 0,
  weight INTEGER NOT NULL DEFAULT 100,
  duration_ms INTEGER NOT NULL,
  accum_x REAL,
  accum_y REAL,
  source TEXT NOT NULL,
  UNIQUE (motion_set_id, motion_mode, motion_action, variant_index)
);

CREATE TABLE motion_hit_window (
  motion_id INTEGER NOT NULL REFERENCES motion_entry(motion_id) ON DELETE CASCADE,
  hit_index INTEGER NOT NULL DEFAULT 0,
  start_ms INTEGER NOT NULL,
  end_ms INTEGER,
  PRIMARY KEY (motion_id, hit_index)
);
CREATE INDEX idx_motion_hit_window_motion ON motion_hit_window(motion_id);

CREATE TABLE motion_fly_event (
  motion_id INTEGER NOT NULL REFERENCES motion_entry(motion_id) ON DELETE CASCADE,
  event_index INTEGER NOT NULL DEFAULT 0,
  release_ms INTEGER NOT NULL,
  fly_file TEXT,
  PRIMARY KEY (motion_id, event_index)
);
CREATE INDEX idx_motion_fly_event_motion ON motion_fly_event(motion_id);

CREATE TABLE motion_fly_data (
  fly_file TEXT PRIMARY KEY,
  init_vel REAL NOT NULL DEFAULT 200.0,
  bomb_range REAL NOT NULL DEFAULT 10.0,
  accel_x REAL NOT NULL DEFAULT 0.0,
  accel_y REAL NOT NULL DEFAULT 0.0,
  accel_z REAL NOT NULL DEFAULT 0.0,
  gravity REAL NOT NULL DEFAULT 0.0,
  is_homing INTEGER NOT NULL DEFAULT 0,
  homing_start_time REAL NOT NULL DEFAULT 0.0,
  homing_max_angle REAL NOT NULL DEFAULT 0.0,
  max_range REAL NOT NULL DEFAULT 2500.0
);
