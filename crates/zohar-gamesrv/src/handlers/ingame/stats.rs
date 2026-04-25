use zohar_domain::entity::EntityId;
use zohar_domain::stat::Stat;
use zohar_map_port::StatUpdate;
use zohar_protocol::game_pkt::ingame::InGameS2c;
use zohar_protocol::game_pkt::ingame::stats::StatsS2c;

use crate::adapters::ToProtocol;

pub(super) fn encode_entity_stat(entity_id: EntityId, stat: Stat, absolute: i32) -> Vec<InGameS2c> {
    let Some(stat_id) = stat.to_protocol() else {
        return Vec::new();
    };

    vec![
        StatsS2c::SyncEntityStat {
            net_id: entity_id.to_protocol(),
            stat_id,
            absolute,
        }
        .into(),
    ]
}

pub(super) fn encode_entity_stats(entity_id: EntityId, stats: Vec<StatUpdate>) -> Vec<InGameS2c> {
    stats
        .into_iter()
        .flat_map(|update| encode_entity_stat(entity_id, update.stat, update.absolute))
        .collect()
}
