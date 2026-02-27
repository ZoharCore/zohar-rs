#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum SpawnType {
    #[strum(serialize = "MOB")]
    Mob,
    #[strum(serialize = "GROUP")]
    Group,
    #[strum(serialize = "AGGRESSIVE_GROUP")]
    AggressiveGroup,
    #[strum(serialize = "GROUP_GROUP")]
    GroupGroup,
    #[strum(serialize = "ANYWHERE")]
    Anywhere,
    #[strum(serialize = "EXCEPTION")]
    Exception,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString)]
pub enum SpawnSource {
    #[strum(serialize = "npc.txt")]
    Npc,
    #[strum(serialize = "boss.txt")]
    Boss,
    #[strum(serialize = "stone.txt")]
    Stone,
    #[strum(serialize = "regen.txt")]
    Regen,
}

#[derive(Debug, Clone)]
pub enum SpawnTarget {
    Mob(i64),
    Group(i64),
    GroupGroup(i64),
}

#[derive(Debug, Clone)]
pub struct SpawnRuleRecord {
    pub map_id: i64,
    pub map_code: String,
    pub target: SpawnTarget,
    pub spawn_type: SpawnType,
    pub spawn_source: SpawnSource,
    pub center_x: f32,
    pub center_y: f32,
    pub extent_x: f32,
    pub extent_y: f32,
    pub direction: i64,
    pub regen_time_sec: i64,
    pub regen_percent: i64,
    pub max_count: i64,
}
