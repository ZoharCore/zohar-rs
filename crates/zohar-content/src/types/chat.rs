use crate::types::mobs::MobType;

#[derive(Debug, Clone)]
pub struct MobChatStrategy {
    pub chat_context: String,
    pub mob_type: Option<MobType>,
    pub mob_id: Option<i64>,
    pub interval_min_sec: i64,
    pub interval_max_sec: i64,
}

#[derive(Debug, Clone)]
pub struct MobChatLine {
    pub mob_id: i64,
    pub chat_context: String,
    pub source_key: String,
    pub text: String,
}
