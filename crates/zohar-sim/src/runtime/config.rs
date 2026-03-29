use crate::chat::MobChatContent;
use crate::motion::EntityMotionSpeedTable;
use crate::navigation::MapNavigator;
use crate::types::MapInstanceKey;
use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use zohar_domain::Empire;
use zohar_domain::coords::LocalSize;
use zohar_domain::entity::mob::MobId;
use zohar_domain::entity::mob::MobPrototype;
use zohar_domain::entity::mob::spawn::SpawnRule;

#[derive(Debug, Clone)]
pub struct WanderConfig {
    pub decision_pause_idle_min: Duration,
    pub decision_pause_idle_max: Duration,
    pub post_move_pause_min: Duration,
    pub post_move_pause_max: Duration,
    pub wander_chance_denominator: u32,
    pub step_min_m: f32,
    pub step_max_m: f32,
}

impl Default for WanderConfig {
    fn default() -> Self {
        Self {
            decision_pause_idle_min: Duration::from_secs(3),
            decision_pause_idle_max: Duration::from_secs(5),
            post_move_pause_min: Duration::from_secs(1),
            post_move_pause_max: Duration::from_secs(3),
            wander_chance_denominator: 7,
            step_min_m: 3.0,
            step_max_m: 7.0,
        }
    }
}

#[derive(Resource, Clone)]
pub struct SharedConfig {
    pub motion_speeds: Arc<EntityMotionSpeedTable>,
    pub mobs: Arc<HashMap<MobId, MobPrototype>>,
    pub wander: WanderConfig,
    pub mob_chat: Arc<MobChatContent>,
}

#[derive(Resource)]
pub struct MapConfig {
    pub map_key: MapInstanceKey,
    pub empire: Option<Empire>,
    pub local_size: LocalSize,
    pub navigator: Option<Arc<MapNavigator>>,
    pub spawn_rules: Vec<SpawnRule>,
}
