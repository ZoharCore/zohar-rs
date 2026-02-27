use std::collections::HashMap;
use zohar_domain::MobId;
use zohar_domain::entity::player::{PlayerClass, PlayerGender};

/// Shared locomotion mode used to select movement motion speeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionMoveMode {
    Run,
    Walk,
}

impl Default for MotionMoveMode {
    fn default() -> Self {
        Self::Run
    }
}

/// Content-driven motion speeds for a single movement profile.
#[derive(Debug, Clone, Copy, Default)]
pub struct MotionSpeeds {
    pub run_units_per_sec: Option<f32>,
    pub walk_units_per_sec: Option<f32>,
}

impl MotionSpeeds {
    pub fn speed_for_mode(self, mode: MotionMoveMode) -> Option<f32> {
        match mode {
            MotionMoveMode::Run => self.run_units_per_sec,
            // If walk is not configured, fallback to run speed for safety.
            MotionMoveMode::Walk => self.walk_units_per_sec.or(self.run_units_per_sec),
        }
    }
}

pub type PlayerMotionSpeeds = MotionSpeeds;
pub type MobMotionSpeeds = MotionSpeeds;

/// Player motion speed lookup keyed by domain player profile.
#[derive(Debug, Clone, Default)]
pub struct PlayerMotionSpeedTable {
    by_profile: Vec<(PlayerMotionProfileKey, PlayerMotionSpeeds)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerMotionProfileKey {
    pub class: PlayerClass,
    pub gender: PlayerGender,
}

impl PlayerMotionSpeedTable {
    pub fn speed_for(&self, key: PlayerMotionProfileKey, mode: MotionMoveMode) -> Option<f32> {
        self.by_profile
            .iter()
            .find_map(|(candidate, speeds)| (*candidate == key).then_some(*speeds))
            .and_then(|speeds| speeds.speed_for_mode(mode))
    }

    pub fn upsert_speed(
        &mut self,
        key: PlayerMotionProfileKey,
        mode: MotionMoveMode,
        units_per_sec: f32,
    ) {
        if let Some((_, speeds)) = self
            .by_profile
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            match mode {
                MotionMoveMode::Run => speeds.run_units_per_sec = Some(units_per_sec),
                MotionMoveMode::Walk => speeds.walk_units_per_sec = Some(units_per_sec),
            }
            return;
        }

        let mut speeds = PlayerMotionSpeeds::default();
        match mode {
            MotionMoveMode::Run => speeds.run_units_per_sec = Some(units_per_sec),
            MotionMoveMode::Walk => speeds.walk_units_per_sec = Some(units_per_sec),
        }
        self.by_profile.push((key, speeds));
    }
}

/// Mob motion speed lookup keyed by mob id.
#[derive(Debug, Clone, Default)]
pub struct MobMotionSpeedTable {
    by_mob: HashMap<MobId, MobMotionSpeeds>,
}

impl MobMotionSpeedTable {
    pub fn speed_for(&self, mob_id: MobId, mode: MotionMoveMode) -> Option<f32> {
        self.by_mob
            .get(&mob_id)
            .copied()
            .and_then(|speeds| speeds.speed_for_mode(mode))
    }

    pub fn upsert_speed(&mut self, mob_id: MobId, mode: MotionMoveMode, units_per_sec: f32) {
        let speeds = self.by_mob.entry(mob_id).or_default();
        match mode {
            MotionMoveMode::Run => speeds.run_units_per_sec = Some(units_per_sec),
            MotionMoveMode::Walk => speeds.walk_units_per_sec = Some(units_per_sec),
        }
    }
}

/// Unified motion-lookup key across entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionEntityKey {
    Player(PlayerMotionProfileKey),
    Mob(MobId),
}

/// Motion speeds indexed by an entity key variant.
#[derive(Debug, Clone, Default)]
pub struct EntityMotionSpeedTable {
    player: PlayerMotionSpeedTable,
    mob: MobMotionSpeedTable,
}

impl EntityMotionSpeedTable {
    pub fn from_tables(player: PlayerMotionSpeedTable, mob: MobMotionSpeedTable) -> Self {
        Self { player, mob }
    }

    pub fn speed_for(&self, entity: MotionEntityKey, mode: MotionMoveMode) -> Option<f32> {
        match entity {
            MotionEntityKey::Player(key) => self.player.speed_for(key, mode),
            MotionEntityKey::Mob(mob_id) => self.mob.speed_for(mob_id, mode),
        }
    }

    pub fn upsert_speed(
        &mut self,
        entity: MotionEntityKey,
        mode: MotionMoveMode,
        units_per_sec: f32,
    ) {
        match entity {
            MotionEntityKey::Player(key) => self.player.upsert_speed(key, mode, units_per_sec),
            MotionEntityKey::Mob(mob_id) => self.mob.upsert_speed(mob_id, mode, units_per_sec),
        }
    }
}
