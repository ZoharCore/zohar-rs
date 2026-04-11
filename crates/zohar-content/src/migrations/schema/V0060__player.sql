CREATE TABLE player_class_base_stats (
  class_key TEXT PRIMARY KEY REFERENCES enum_player_class(value),
  base_strength INTEGER NOT NULL,
  base_vitality INTEGER NOT NULL,
  base_dexterity INTEGER NOT NULL,
  base_intelligence INTEGER NOT NULL,
  base_hp INTEGER NOT NULL,
  base_sp INTEGER NOT NULL,
  hp_per_vitality INTEGER NOT NULL,
  sp_per_intelligence INTEGER NOT NULL,
  hp_per_level_min INTEGER NOT NULL,
  hp_per_level_max INTEGER NOT NULL,
  sp_per_level_min INTEGER NOT NULL,
  sp_per_level_max INTEGER NOT NULL,
  base_stamina INTEGER NOT NULL,
  stamina_per_vitality INTEGER NOT NULL,
  stamina_per_level_min INTEGER NOT NULL,
  stamina_per_level_max INTEGER NOT NULL
);

CREATE TABLE level_exp (
  level INTEGER PRIMARY KEY,
  next_exp INTEGER NOT NULL CHECK (next_exp BETWEEN 0 AND 4294967295),
  death_loss_pct INTEGER NOT NULL DEFAULT 0 CHECK (death_loss_pct BETWEEN 0 AND 100)
);
