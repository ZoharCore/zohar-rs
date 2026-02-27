-- Mob groupings
CREATE TABLE mob_group
(
    group_id INTEGER PRIMARY KEY,
    code     TEXT
);

CREATE TABLE mob_group_entry
(
    group_id INTEGER NOT NULL REFERENCES mob_group (group_id),
    seq      INTEGER NOT NULL,
    mob_id   INTEGER NOT NULL REFERENCES mob_proto (mob_id),
    PRIMARY KEY (group_id, seq)
);

-- Group rotations with weighted spawn chances
CREATE TABLE mob_group_group
(
    group_group_id INTEGER PRIMARY KEY,
    code           TEXT
);

CREATE TABLE mob_group_group_entry
(
    group_group_id INTEGER NOT NULL REFERENCES mob_group_group (group_group_id),
    seq            INTEGER NOT NULL,
    group_id       INTEGER NOT NULL REFERENCES mob_group (group_id),
    weight         INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (group_group_id, seq)
);

CREATE TABLE enum_spawn_type
(
    value TEXT PRIMARY KEY
);
CREATE TABLE enum_spawn_source
(
    value TEXT PRIMARY KEY
);

-- Spawn rules for: mobs *OR* groups of mobs *OR* group group (weighted random group choice)
CREATE TABLE map_spawn_rule
(
    spawn_id              INTEGER PRIMARY KEY,
    map_id                INTEGER NOT NULL REFERENCES map_def (map_id),
    target_mob_id         INTEGER REFERENCES mob_proto (mob_id),
    target_group_id       INTEGER REFERENCES mob_group (group_id),
    target_group_group_id INTEGER REFERENCES mob_group_group (group_group_id),
    spawn_type            TEXT    NOT NULL REFERENCES enum_spawn_type (value),
    spawn_source          TEXT    NOT NULL REFERENCES enum_spawn_source (value),
    center_x              REAL    NOT NULL CHECK (center_x >= 0),
    center_y              REAL    NOT NULL CHECK (center_y >= 0),
    extent_x              REAL    NOT NULL CHECK (extent_x >= 0),
    extent_y              REAL    NOT NULL CHECK (extent_y >= 0),
    direction             INTEGER NOT NULL DEFAULT 0,
    regen_time_sec        INTEGER NOT NULL,
    regen_percent         INTEGER NOT NULL DEFAULT 100,
    max_count             INTEGER NOT NULL DEFAULT 1,
    CHECK (
        (target_mob_id IS NOT NULL AND target_group_id IS NULL AND target_group_group_id IS NULL)
            OR
        (target_mob_id IS NULL AND target_group_id IS NOT NULL AND target_group_group_id IS NULL)
            OR
        (target_mob_id IS NULL AND target_group_id IS NULL AND target_group_group_id IS NOT NULL)
        )
);

-- TODO: do we even need this? or only index on map_id is fine?
CREATE INDEX map_spawn_rule_npc_idx ON map_spawn_rule (map_id, spawn_source, target_mob_id);
CREATE INDEX map_spawn_rule_group_idx ON map_spawn_rule (map_id, spawn_source, target_group_id);
CREATE INDEX map_spawn_rule_group_group_idx ON map_spawn_rule (map_id, spawn_source, target_group_group_id);

-- Keep center-in-bounds validation but allow extents to cross boundaries.
CREATE TRIGGER map_spawn_rule_bounds_insert
    BEFORE INSERT
    ON map_spawn_rule
    FOR EACH ROW
    WHEN EXISTS (SELECT 1
                 FROM map_def d
                 WHERE d.map_id = NEW.map_id
                   AND (
                     NEW.center_x < 0 OR
                     NEW.center_y < 0 OR
                     NEW.center_x >= d.map_width OR
                     NEW.center_y >= d.map_height
                     ))
BEGIN
    SELECT RAISE(ABORT, 'map_spawn_rule out of map bounds');
END;

CREATE TRIGGER map_spawn_rule_bounds_update
    BEFORE UPDATE OF map_id, center_x, center_y, extent_x, extent_y
    ON map_spawn_rule
    FOR EACH ROW
    WHEN EXISTS (SELECT 1
                 FROM map_def d
                 WHERE d.map_id = NEW.map_id
                   AND (
                     NEW.center_x < 0 OR
                     NEW.center_y < 0 OR
                     NEW.center_x >= d.map_width OR
                     NEW.center_y >= d.map_height
                     ))
BEGIN
    SELECT RAISE(ABORT, 'map_spawn_rule out of map bounds');
END;
