use std::collections::HashMap;
use zohar_domain::entity::mob::{MobId, MobKind};

#[derive(Debug, Clone, Copy)]
pub struct MobChatStrategyInterval {
    pub interval_min_sec: u32,
    pub interval_max_sec: u32,
}

#[derive(Debug, Clone)]
pub struct MobChatLine {
    pub source_key: String,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct MobChatContent {
    pub strategy_type_defaults: HashMap<(String, MobKind), MobChatStrategyInterval>,
    pub strategy_mob_overrides: HashMap<(String, MobId), MobChatStrategyInterval>,
    pub lines_by_mob: HashMap<(String, MobId), Vec<MobChatLine>>,
}

impl MobChatContent {
    pub fn strategy_for(
        &self,
        context: &str,
        mob_kind: MobKind,
        mob_id: MobId,
    ) -> Option<MobChatStrategyInterval> {
        self.strategy_mob_overrides
            .get(&(context.to_string(), mob_id))
            .copied()
            .or_else(|| {
                self.strategy_type_defaults
                    .get(&(context.to_string(), mob_kind))
                    .copied()
            })
    }

    pub fn lines_for(&self, context: &str, mob_id: MobId) -> Option<&[MobChatLine]> {
        self.lines_by_mob
            .get(&(context.to_string(), mob_id))
            .map(Vec::as_slice)
    }
}
