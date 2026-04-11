pub mod chat;
pub mod empires;
pub mod maps;
pub mod mob_groups;
pub mod mobs;
pub mod motion;
pub mod player;
pub mod spawns;

use chat::{MobChatLine, MobChatStrategy};
use empires::EmpireStartConfig;
use maps::{ContentMap, MapTownSpawn, TerrainFlagsGrid};
use mob_groups::{MobGroupGroupRecord, MobGroupRecord};
use mobs::ContentMob;
use motion::{ContentMotion, PlayerMotionProfile};
use player::PlayerClassBaseStats;
use spawns::SpawnRuleRecord;

#[derive(Debug, Default, Clone)]
pub struct ContentCatalog {
    pub player_class_base_stats: Vec<PlayerClassBaseStats>,
    pub maps: Vec<ContentMap>,
    pub map_terrain_flags: Vec<TerrainFlagsGrid>,
    pub town_spawns: Vec<MapTownSpawn>,
    pub mobs: Vec<ContentMob>,
    pub mob_groups: Vec<MobGroupRecord>,
    pub mob_group_groups: Vec<MobGroupGroupRecord>,
    pub player_motion_profiles: Vec<PlayerMotionProfile>,
    pub empire_start_configs: Vec<EmpireStartConfig>,
    pub spawn_rules: Vec<SpawnRuleRecord>,
    pub motion: Vec<ContentMotion>,
    pub mob_chat_strategies: Vec<MobChatStrategy>,
    pub mob_chat_lines: Vec<MobChatLine>,
}
