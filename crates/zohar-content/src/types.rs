pub mod chat;
pub mod empires;
pub mod maps;
pub mod mob_groups;
pub mod mobs;
pub mod motion;
pub mod player;
pub mod spawns;

#[derive(Debug, Default, Clone)]
pub struct ContentCatalog {
    pub player_class_base_stats: Vec<player::PlayerClassBaseStats>,
    pub level_exp: Vec<player::LevelExp>,
    pub maps: Vec<maps::ContentMap>,
    pub map_terrain_flags: Vec<maps::TerrainFlagsGrid>,
    pub town_spawns: Vec<maps::MapTownSpawn>,
    pub mobs: Vec<mobs::ContentMob>,
    pub mob_groups: Vec<mob_groups::MobGroupRecord>,
    pub mob_group_groups: Vec<mob_groups::MobGroupGroupRecord>,
    pub player_motion_profiles: Vec<motion::PlayerMotionProfile>,
    pub empire_start_configs: Vec<empires::EmpireStartConfig>,
    pub spawn_rules: Vec<spawns::SpawnRuleRecord>,
    pub motion: Vec<motion::ContentMotion>,
    pub mob_chat_strategies: Vec<chat::MobChatStrategy>,
    pub mob_chat_lines: Vec<chat::MobChatLine>,
}
