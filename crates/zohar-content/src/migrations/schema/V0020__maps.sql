-- Map definitions and static placement.
--
-- Coordinate system contract (single canonical storage unit):
-- - Every persisted coordinate is in meters.
-- - Coordinates that belong to a map (`x`, `y`, `start_x`, `start_y`) are local-to-map.
--
-- This keeps the emulator flexible (not constrained to legacy high-resolution grids)
-- while still validating coordinate safety against map dimensions.

CREATE TABLE map_def (
  map_id INTEGER PRIMARY KEY,
  code TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  map_width REAL NOT NULL CHECK (map_width > 0),
  map_height REAL NOT NULL CHECK (map_height > 0),
  empire TEXT REFERENCES enum_empire(value)
);

CREATE TABLE map_placement (
  map_id INTEGER PRIMARY KEY REFERENCES map_def(map_id),
  base_x REAL NOT NULL,
  base_y REAL NOT NULL
);

CREATE TABLE map_town_spawn (
  map_id INTEGER NOT NULL REFERENCES map_def(map_id),
  empire TEXT NOT NULL REFERENCES enum_empire(value),
  x REAL NOT NULL CHECK (x >= 0),
  y REAL NOT NULL CHECK (y >= 0),
  PRIMARY KEY (map_id, empire)
);

CREATE TABLE empire_start_config (
  empire TEXT PRIMARY KEY REFERENCES enum_empire(value),
  start_map_id INTEGER NOT NULL REFERENCES map_def(map_id),
  start_x REAL NOT NULL CHECK (start_x >= 0),
  start_y REAL NOT NULL CHECK (start_y >= 0)
);

-- Ensure map-local spawn coordinates always remain within the owning map bounds.
CREATE TRIGGER map_town_spawn_bounds_insert
BEFORE INSERT ON map_town_spawn
FOR EACH ROW
WHEN EXISTS (
  SELECT 1
  FROM map_def d
  WHERE d.map_id = NEW.map_id
    AND (NEW.x >= d.map_width OR NEW.y >= d.map_height)
)
BEGIN
  SELECT RAISE(ABORT, 'map_town_spawn out of map bounds');
END;

CREATE TRIGGER map_town_spawn_bounds_update
BEFORE UPDATE OF map_id, x, y ON map_town_spawn
FOR EACH ROW
WHEN EXISTS (
  SELECT 1
  FROM map_def d
  WHERE d.map_id = NEW.map_id
    AND (NEW.x >= d.map_width OR NEW.y >= d.map_height)
)
BEGIN
  SELECT RAISE(ABORT, 'map_town_spawn out of map bounds');
END;

CREATE TRIGGER empire_start_config_bounds_insert
BEFORE INSERT ON empire_start_config
FOR EACH ROW
WHEN EXISTS (
  SELECT 1
  FROM map_def d
  WHERE d.map_id = NEW.start_map_id
    AND (NEW.start_x >= d.map_width OR NEW.start_y >= d.map_height)
)
BEGIN
  SELECT RAISE(ABORT, 'empire_start_config out of map bounds');
END;

CREATE TRIGGER empire_start_config_bounds_update
BEFORE UPDATE OF start_map_id, start_x, start_y ON empire_start_config
FOR EACH ROW
WHEN EXISTS (
  SELECT 1
  FROM map_def d
  WHERE d.map_id = NEW.start_map_id
    AND (NEW.start_x >= d.map_width OR NEW.start_y >= d.map_height)
)
BEGIN
  SELECT RAISE(ABORT, 'empire_start_config out of map bounds');
END;
