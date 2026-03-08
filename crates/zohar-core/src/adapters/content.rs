use crate::adapters::ToDomain;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;
use zohar_content::types::ContentCatalog;
use zohar_content::types::mobs::{MobAiFlags, MobRank as ContentMobRank, MobType};
use zohar_content::types::motion::{
    MotionAction as ContentMotionAction, MotionEntityKind, MotionMode,
};
use zohar_content::types::player::{Gender as ContentGender, PlayerClass as ContentPlayerClass};
use zohar_content::types::spawns::{SpawnTarget, SpawnType as ContentSpawnType};
use zohar_domain::coords::{LocalPos, LocalSize};
use zohar_domain::entity::mob::spawn::{
    Direction, FacingStrategy, SpawnArea, SpawnRule, SpawnRuleDef, SpawnTemplate,
    WeightedGroupChoice,
};
use zohar_domain::entity::mob::{MobId, MobKind, MobPrototype, MobPrototypeDef, MobRank};
use zohar_domain::entity::player::{PlayerClass, PlayerGender};
use zohar_domain::util::FlagsMapper;
use zohar_domain::{BehaviorFlags, DefId, MapId};
use zohar_sim::{
    EntityMotionSpeedTable, MobChatContent, MobChatLine, MobChatStrategyInterval, MotionEntityKey,
    MotionMoveMode, PlayerMotionProfileKey,
};

pub(crate) fn build_entity_motion_speeds(catalog: &ContentCatalog) -> EntityMotionSpeedTable {
    let mut by_profile_id = HashMap::<i64, PlayerMotionProfileKey>::new();
    for profile in &catalog.player_motion_profiles {
        by_profile_id.insert(
            profile.profile_id,
            PlayerMotionProfileKey {
                class: profile.player_class.to_domain(),
                gender: profile.gender.to_domain(),
            },
        );
    }

    let mut table = EntityMotionSpeedTable::default();
    for motion in &catalog.motion {
        if motion.motion_mode != MotionMode::General {
            continue;
        }

        let move_mode = match motion.motion_action {
            ContentMotionAction::Run => MotionMoveMode::Run,
            ContentMotionAction::Walk => MotionMoveMode::Walk,
            _ => continue,
        };

        let motion_entity = match motion.entity_kind {
            MotionEntityKind::Player => {
                let Some(profile_id) = motion.player_profile_id else {
                    continue;
                };
                let Some(&profile_key) = by_profile_id.get(&profile_id) else {
                    continue;
                };
                MotionEntityKey::Player(profile_key)
            }
            MotionEntityKind::Mob => {
                let Some(raw_mob_id) = motion.mob_id else {
                    continue;
                };
                let Some(mob_id) = def_id_from_i64::<zohar_domain::entity::mob::MobDefTag>(
                    raw_mob_id,
                    "motion.mob_id",
                ) else {
                    continue;
                };
                MotionEntityKey::Mob(mob_id)
            }
        };

        let duration_ms = motion.duration_ms as f64;
        if duration_ms <= 0.0 {
            continue;
        }

        let (Some(accum_x), Some(accum_y)) = (motion.accum_x, motion.accum_y) else {
            continue;
        };
        let distance_units = accum_x.hypot(accum_y);
        if !distance_units.is_finite() || distance_units <= f64::EPSILON {
            continue;
        }

        let units_per_sec = (distance_units / duration_ms) * 1000.0;
        if !units_per_sec.is_finite() || units_per_sec <= 0.0 {
            continue;
        }

        table.upsert_speed(motion_entity, move_mode, units_per_sec as f32);
    }

    table
}

pub(crate) fn build_mob_proto(catalog: &ContentCatalog) -> HashMap<MobId, MobPrototype> {
    let mut mob_proto: HashMap<MobId, MobPrototype> = HashMap::new();

    for mob in &catalog.mobs {
        let Some(mob_id) =
            def_id_from_i64::<zohar_domain::entity::mob::MobDefTag>(mob.mob_id, "mobs.mob_id")
        else {
            continue;
        };

        let mob_kind = mob.mob_type.to_domain().unwrap_or_else(|| {
            let fallback = MobKind::Monster;
            warn!(
                ?mob,
                ?fallback,
                "Unsupported mob type, overriding with fallback"
            );
            fallback
        });

        let proto = MobPrototype::new(MobPrototypeDef {
            mob_id,
            mob_kind,
            name: mob.name.clone(),
            rank: mob.rank.to_domain(),
            level: mob.level as u32,
            move_speed: mob.move_speed as u8,
            attack_speed: mob.attack_speed as u8,
            bhv_flags: mob.ai_flags.to_domain(),
            empire: None, // TODO FUTURE: add support for mob empires in catalog
        });

        mob_proto.insert(mob_id, proto);
    }

    mob_proto
}
pub(crate) fn build_mob_chat_content(catalog: &ContentCatalog) -> MobChatContent {
    let mut out = MobChatContent::default();

    for strategy in &catalog.mob_chat_strategies {
        let Some(interval_min_sec) = u32::try_from(strategy.interval_min_sec).ok() else {
            warn!(
                interval_min_sec = strategy.interval_min_sec,
                context = %strategy.chat_context,
                "Skipping chat strategy with out-of-range interval_min_sec"
            );
            continue;
        };
        let Some(interval_max_sec) = u32::try_from(strategy.interval_max_sec).ok() else {
            warn!(
                interval_max_sec = strategy.interval_max_sec,
                context = %strategy.chat_context,
                "Skipping chat strategy with out-of-range interval_max_sec"
            );
            continue;
        };
        if interval_min_sec == 0 || interval_max_sec < interval_min_sec {
            warn!(
                interval_min_sec,
                interval_max_sec,
                context = %strategy.chat_context,
                "Skipping chat strategy with invalid interval bounds"
            );
            continue;
        }
        let interval = MobChatStrategyInterval {
            interval_min_sec,
            interval_max_sec,
        };

        match (strategy.mob_type, strategy.mob_id) {
            (Some(mob_type), None) => {
                let Some(mob_kind) = mob_type.to_domain() else {
                    warn!(
                        ?strategy,
                        "Skipping chat strategy with unsupported mob_type"
                    );
                    continue;
                };
                out.strategy_type_defaults
                    .insert((strategy.chat_context.clone(), mob_kind), interval);
            }
            (None, Some(raw_mob_id)) => {
                let Some(mob_id) = def_id_from_i64::<zohar_domain::entity::mob::MobDefTag>(
                    raw_mob_id,
                    "mob_chat_strategy.mob_id",
                ) else {
                    continue;
                };
                out.strategy_mob_overrides
                    .insert((strategy.chat_context.clone(), mob_id), interval);
            }
            _ => {
                warn!(?strategy, "Skipping chat strategy with invalid scope shape");
            }
        }
    }

    for line in &catalog.mob_chat_lines {
        if line.text.trim().is_empty() {
            continue;
        }
        if line.source_key.trim().is_empty() {
            continue;
        }
        let Some(mob_id) = def_id_from_i64::<zohar_domain::entity::mob::MobDefTag>(
            line.mob_id,
            "mob_chat_line.mob_id",
        ) else {
            continue;
        };
        out.lines_by_mob
            .entry((line.chat_context.clone(), mob_id))
            .or_default()
            .push(MobChatLine {
                source_key: line.source_key.clone(),
                text: line.text.clone(),
            });
    }

    out
}

pub(crate) fn build_spawn_rules(catalog: &ContentCatalog) -> HashMap<MapId, Vec<SpawnRule>> {
    let mut spawn_rules: HashMap<MapId, Vec<SpawnRule>> = HashMap::new();

    let map_bounds: HashMap<MapId, (f32, f32)> = catalog
        .maps
        .iter()
        .filter_map(|map| {
            let map_id = def_id_from_i64::<zohar_domain::MapDefTag>(map.map_id, "maps.map_id")?;
            Some((map_id, (map.map_width, map.map_height)))
        })
        .collect();

    let mob_groups: HashMap<i64, Arc<[MobId]>> = catalog
        .mob_groups
        .iter()
        .map(|group| {
            let members: Vec<MobId> = group
                .entries
                .iter()
                .filter_map(|entry| {
                    def_id_from_i64::<zohar_domain::entity::mob::MobDefTag>(
                        entry.mob_id,
                        "mob_groups.mob_id",
                    )
                })
                .collect();
            (group.group_id, Arc::from(members))
        })
        .collect();

    let mob_group_groups: HashMap<i64, Arc<[WeightedGroupChoice]>> = catalog
        .mob_group_groups
        .iter()
        .map(|group_group| {
            let choices: Vec<WeightedGroupChoice> = group_group
                .entries
                .iter()
                .filter_map(|entry| {
                    let members = mob_groups.get(&entry.group_id)?;
                    let weight = u32::try_from(entry.weight.max(1)).ok()?;
                    Some(WeightedGroupChoice {
                        members: Arc::clone(members),
                        weight,
                    })
                })
                .collect();
            (group_group.group_group_id, Arc::from(choices))
        })
        .collect();

    for spawn in &catalog.spawn_rules {
        let Some(map_id) =
            def_id_from_i64::<zohar_domain::MapDefTag>(spawn.map_id, "spawn_rules.map_id")
        else {
            continue;
        };

        let template = match (&spawn.target, spawn.spawn_type) {
            (SpawnTarget::Mob(raw_mob_id), _) => {
                let Some(mob_id) = def_id_from_i64::<zohar_domain::entity::mob::MobDefTag>(
                    *raw_mob_id,
                    "spawn_rules.mob_id",
                ) else {
                    warn!(?spawn, "Invalid mob id in spawn rule, skipping");
                    continue;
                };
                SpawnTemplate::Mob(mob_id)
            }
            (SpawnTarget::Group(group_id), _) => {
                let Some(members) = mob_groups.get(group_id).cloned() else {
                    warn!(?spawn, group_id, "Missing mob_group for spawn, skipping");
                    continue;
                };
                if members.is_empty() {
                    warn!(?spawn, group_id, "mob_group has no members, skipping");
                    continue;
                }
                SpawnTemplate::Group(members)
            }
            (SpawnTarget::GroupGroup(group_group_id), _) => {
                let Some(choices) = mob_group_groups.get(group_group_id).cloned() else {
                    warn!(
                        ?spawn,
                        group_group_id, "Missing mob_group_group for spawn, skipping"
                    );
                    continue;
                };
                if choices.is_empty() {
                    warn!(
                        ?spawn,
                        group_group_id, "mob_group_group has no members, skipping"
                    );
                    continue;
                }
                SpawnTemplate::GroupGroup(choices)
            }
        };

        if matches!(spawn.spawn_type, ContentSpawnType::Exception) {
            continue;
        }

        let (center, extent) = if matches!(spawn.spawn_type, ContentSpawnType::Anywhere) {
            let Some((width, height)) = map_bounds.get(&map_id).copied() else {
                warn!(?spawn, "Missing map bounds for ANYWHERE spawn, skipping");
                continue;
            };
            (
                LocalPos::new(width * 0.5, height * 0.5),
                LocalSize::new((width * 0.5) - 1.0, (height * 0.5) - 1.0),
            )
        } else {
            (
                LocalPos::new(spawn.center_x, spawn.center_y),
                LocalSize::new(spawn.extent_x, spawn.extent_y),
            )
        };

        let facing = u8::try_from(spawn.direction)
            .ok()
            .and_then(Direction::from_content_raw)
            .map(FacingStrategy::Fixed)
            .unwrap_or(FacingStrategy::Random);

        let rule = SpawnRule::new(SpawnRuleDef {
            template,
            area: SpawnArea::new(center, extent),
            facing,
            max_count: spawn.max_count.max(1) as usize,
            regen_time: Duration::from_secs(spawn.regen_time_sec.max(1) as u64),
        });

        spawn_rules.entry(map_id).or_default().push(rule);
    }

    spawn_rules
}

fn def_id_from_i64<T>(raw: i64, field: &'static str) -> Option<DefId<T>> {
    match u32::try_from(raw) {
        Ok(value) => Some(DefId::new(value)),
        Err(error) => {
            warn!(
                %error,
                %field,
                raw,
                "Content id is out of range for definition id; skipping record"
            );
            None
        }
    }
}

impl ToDomain<PlayerClass> for ContentPlayerClass {
    fn to_domain(self) -> PlayerClass {
        match self {
            ContentPlayerClass::Warrior => PlayerClass::Warrior,
            ContentPlayerClass::Ninja => PlayerClass::Ninja,
            ContentPlayerClass::Sura => PlayerClass::Sura,
            ContentPlayerClass::Shaman => PlayerClass::Shaman,
        }
    }
}

impl ToDomain<PlayerGender> for ContentGender {
    fn to_domain(self) -> PlayerGender {
        match self {
            ContentGender::Male => PlayerGender::Male,
            ContentGender::Female => PlayerGender::Female,
        }
    }
}

impl ToDomain<MobRank> for ContentMobRank {
    fn to_domain(self) -> MobRank {
        match self {
            ContentMobRank::Pawn => MobRank::Pawn,
            ContentMobRank::SuperPawn => MobRank::SuperPawn,
            ContentMobRank::Knight => MobRank::Knight,
            ContentMobRank::SuperKnight => MobRank::SuperKnight,
            ContentMobRank::Boss => MobRank::Boss,
            ContentMobRank::King => MobRank::King,
        }
    }
}

impl ToDomain<Option<MobKind>> for MobType {
    fn to_domain(self) -> Option<MobKind> {
        Some(match self {
            MobType::Npc => MobKind::Npc,
            MobType::Monster => MobKind::Monster,
            MobType::Stone => MobKind::Stone,
            MobType::Warp => MobKind::Portal,
            _ => return None,
        })
    }
}

impl ToDomain<BehaviorFlags> for MobAiFlags {
    fn to_domain(self) -> BehaviorFlags {
        const MAPPER: FlagsMapper<MobAiFlags, BehaviorFlags> = FlagsMapper::new(&[
            (MobAiFlags::NOMOVE, BehaviorFlags::NO_MOVE),
            (MobAiFlags::AGGR, BehaviorFlags::AGGRESSIVE),
        ]);

        MAPPER.map(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_content::types::maps::ContentMap;
    use zohar_content::types::mob_groups::MobGroupRecord;
    use zohar_content::types::mobs::ContentMob;
    use zohar_content::types::spawns::{SpawnRuleRecord, SpawnSource, SpawnTarget, SpawnType};

    fn valid_map(map_id: i64) -> ContentMap {
        ContentMap {
            map_id,
            code: format!("map_{map_id}"),
            name: "map".to_string(),
            map_width: 512.0,
            map_height: 512.0,
            empire: None,
            base_x: Some(0.0),
            base_y: Some(0.0),
        }
    }

    fn valid_mob(mob_id: i64, mob_type: MobType) -> ContentMob {
        ContentMob {
            mob_id,
            code: format!("mob_{mob_id}"),
            name: "mob".to_string(),
            mob_type,
            rank: ContentMobRank::Pawn,
            level: 1,
            ai_flags: MobAiFlags::empty(),
            move_speed: 100,
            attack_speed: 100,
        }
    }

    fn spawn_record(map_id: i64, target: SpawnTarget, direction: i64) -> SpawnRuleRecord {
        SpawnRuleRecord {
            map_id,
            map_code: "map_1".to_string(),
            target,
            spawn_type: SpawnType::Mob,
            spawn_source: SpawnSource::Npc,
            center_x: 100.0,
            center_y: 100.0,
            extent_x: 10.0,
            extent_y: 10.0,
            direction,
            regen_time_sec: 60,
            regen_percent: 100,
            max_count: 2,
        }
    }

    #[test]
    fn build_mob_proto_skips_out_of_range_ids() {
        let catalog = ContentCatalog {
            mobs: vec![
                valid_mob(101, MobType::Monster),
                valid_mob(i64::from(u32::MAX) + 1, MobType::Monster),
            ],
            ..ContentCatalog::default()
        };

        let protos = build_mob_proto(&catalog);
        assert_eq!(protos.len(), 1);
        assert!(protos.contains_key(&MobId::new(101)));
    }

    #[test]
    fn build_mob_proto_falls_back_for_unsupported_type() {
        let catalog = ContentCatalog {
            mobs: vec![valid_mob(101, MobType::Door)],
            ..ContentCatalog::default()
        };

        let protos = build_mob_proto(&catalog);
        let proto = protos.get(&MobId::new(101)).expect("proto");
        assert_eq!(proto.mob_kind, MobKind::Monster);
    }

    #[test]
    fn build_mob_proto_preserves_ai_flags() {
        let mut mob = valid_mob(101, MobType::Monster);
        mob.ai_flags = MobAiFlags::NOMOVE | MobAiFlags::AGGR;

        let catalog = ContentCatalog {
            mobs: vec![mob],
            ..ContentCatalog::default()
        };

        let protos = build_mob_proto(&catalog);
        let proto = protos.get(&MobId::new(101)).expect("proto");
        assert!(proto.bhv_flags.contains(BehaviorFlags::NO_MOVE));
        assert!(proto.bhv_flags.contains(BehaviorFlags::AGGRESSIVE));
    }

    #[test]
    fn build_spawn_rules_skips_out_of_range_map_and_mob_ids() {
        let catalog = ContentCatalog {
            maps: vec![valid_map(1)],
            mob_groups: vec![MobGroupRecord {
                group_id: 1,
                code: None,
                entries: vec![],
            }],
            spawn_rules: vec![
                spawn_record(1, SpawnTarget::Mob(101), 1),
                spawn_record(i64::from(u32::MAX) + 1, SpawnTarget::Mob(101), 1),
                spawn_record(1, SpawnTarget::Mob(i64::from(u32::MAX) + 1), 1),
            ],
            ..ContentCatalog::default()
        };

        let rules = build_spawn_rules(&catalog);
        assert_eq!(rules.get(&MapId::new(1)).map(Vec::len), Some(1));
    }

    #[test]
    fn build_spawn_rules_maps_invalid_direction_to_random_facing() {
        let catalog = ContentCatalog {
            maps: vec![valid_map(1)],
            spawn_rules: vec![
                spawn_record(1, SpawnTarget::Mob(101), 1),
                spawn_record(1, SpawnTarget::Mob(101), 0),
            ],
            ..ContentCatalog::default()
        };

        let rules = build_spawn_rules(&catalog);
        let rules = rules.get(&MapId::new(1)).expect("rules for map");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].facing, FacingStrategy::Fixed(Direction::North));
        assert_eq!(rules[1].facing, FacingStrategy::Random);
    }
}
