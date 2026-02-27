#[derive(Debug, Clone)]
pub struct MobGroupEntry {
    pub mob_id: i64,
}

#[derive(Debug, Clone)]
pub struct MobGroupRecord {
    pub group_id: i64,
    pub code: Option<String>,
    pub entries: Vec<MobGroupEntry>,
}

#[derive(Debug, Clone)]
pub struct MobGroupGroupEntry {
    pub group_id: i64,
    pub weight: i64,
}

#[derive(Debug, Clone)]
pub struct MobGroupGroupRecord {
    pub group_group_id: i64,
    pub code: Option<String>,
    pub entries: Vec<MobGroupGroupEntry>,
}
