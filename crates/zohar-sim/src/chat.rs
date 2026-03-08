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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mob_override_wins_over_type_default() {
        let mut content = MobChatContent::default();
        content.strategy_type_defaults.insert(
            ("idle".to_string(), MobKind::Monster),
            MobChatStrategyInterval {
                interval_min_sec: 10,
                interval_max_sec: 20,
            },
        );
        content.strategy_mob_overrides.insert(
            ("idle".to_string(), MobId::new(101)),
            MobChatStrategyInterval {
                interval_min_sec: 1,
                interval_max_sec: 2,
            },
        );

        let resolved = content
            .strategy_for("idle", MobKind::Monster, MobId::new(101))
            .expect("strategy");
        assert_eq!(resolved.interval_min_sec, 1);
        assert_eq!(resolved.interval_max_sec, 2);
    }
}
