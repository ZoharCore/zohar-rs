use super::*;
use crate::chat::MobChatContent;
use crate::navigation::{MapNavigator, TerrainFlagsGrid};
use bevy::prelude::{App, Entity};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

use super::aggro::MobAggroDispatchBuffer;
use super::facts::{
    ActorDamaged, ActorRef, ActorSpecialEffect, ActorSpecialEffectKind, FrameFacts,
    PointVisualEffect,
};
use super::state::{
    LocalTransform, MapDirtyEntityPublicStates, MapPendingMovements, MapReplication, MapSpawnRules,
    MobAggro, MobAggroQueue, MobBrainMode, MobBrainState, MobMarker, MobMotion, MobMotionState,
    MobStatsComp, NetEntityId, PendingMovement, PlayerActivityComp, PlayerCommandQueue,
    PlayerIndex, PlayerMotion, PlayerMotionState, PlayerMovementAnimation, PlayerProgressionComp,
    PlayerStatTickerComp, PlayerStatsComp, RuntimeState, SharedConfig, SimDuration,
};
use super::util::sample_player_motion_at;
use crate::MapEventSender;
use crate::motion::{EntityMotionSpeedTable, MotionMoveMode, PlayerMotionProfileKey};
use crate::persistence::{PlayerPersistenceCoordinatorHandle, player_persistence_channel};
use crate::runtime::time::SimTickerClock;
use crate::types::MapInstanceKey;
use zohar_domain::appearance::{
    EntityKind, EntityPublicEquipment, EntityPublicFlags, EntityPublicSocial, EntityPublicSpeeds,
    EntityPublicState, EntityStateFlags, PlayerAppearance, PlayerVisualProfile,
};
use zohar_domain::coords::{
    Facing72, LocalDistMeters, LocalPos, LocalPosExt, LocalRotation, LocalSize,
};
use zohar_domain::entity::mob::spawn::{
    Direction, FacingStrategy, SpawnArea, SpawnRuleDef, SpawnTemplate,
};
use zohar_domain::entity::mob::{
    MobBattleType, MobCombatStats, MobId, MobKind, MobPrototypeDef, MobRank, MobRewards,
    PortalBehavior,
};
use zohar_domain::entity::player::skill::SkillId;
use zohar_domain::entity::player::{
    CoreStatAllocations, PlayerClass, PlayerGameplayBootstrap, PlayerId, PlayerPlaytime,
    PlayerProgressionSnapshot, PlayerRuntimeSnapshot, PlayerSnapshot,
};
use zohar_domain::entity::{EntityId, MovementAnimation, MovementKind};
use zohar_domain::{BehaviorFlags, MapId, TerrainFlags};
use zohar_gameplay::combat::HitFlags;
use zohar_gameplay::stats::game::{
    ActorStatSource, CompiledModifier, CompiledStatContribution, CoreStatBlock,
    DeterministicGrowthVersion, PlayerGrowthFormula, PlayerProgressionState, PlayerResourceFormula,
    PlayerStatSource, SourceSpeeds, Stat,
};
use zohar_map_port::{
    AttackIntent, AttackTargetIntent, ChatChannel, ChatIntent as PortChatIntent, ClientIntent,
    ClientIntentMsg, ClientTimestamp, CoreStatAllocationIntent, CoreStatKind, EnterMsg, LeaveMsg,
    MoveIntent, MovementArg, PlayerEvent, PlayerProgressionIntent, PlayerRestartIntent,
    PortalDestination, ProjectileEffectKind, SpecialEffectType,
};

fn sim_ms(value: u64) -> super::state::SimInstant {
    super::state::SimInstant::from_millis(value)
}

fn client_ts(value: u32) -> ClientTimestamp {
    ClientTimestamp::new(value)
}

fn facing(rot: u8) -> Facing72 {
    Facing72::try_from(rot).expect("valid facing")
}

fn set_sim_now(app: &mut App, now: impl Into<super::state::SimInstant>) {
    app.world_mut().resource_mut::<RuntimeState>().sim_now = now.into();
}

fn attack_from_test_code(code: u8) -> AttackIntent {
    if code == 0 {
        AttackIntent::Basic
    } else {
        AttackIntent::Skill(SkillId::ThreeWayCut)
    }
}

fn test_configs(map_key: MapInstanceKey) -> (SharedConfig, MapConfig) {
    (
        SharedConfig {
            motion_speeds: Arc::new(EntityMotionSpeedTable::default()),
            mobs: Arc::new(HashMap::new()),
            player_stats: Arc::new(test_player_stat_rules()),
            wander: WanderConfig::default(),
            mob_chat: Arc::new(MobChatContent::default()),
        },
        MapConfig {
            map_key,
            map_code: "test_map".to_string(),
            empire: None,
            local_size: LocalSize::new(16_384.0, 16_384.0),
            navigator: None,
            spawn_rules: Vec::new(),
        },
    )
}

fn test_player_stat_rules() -> crate::PlayerStatRules {
    test_player_stat_rules_with_level_exp((1..=120).map(|level| crate::LevelExpEntry {
        level,
        next_exp: i64::from(level) * 300,
        death_loss_pct: 0,
    }))
}

fn test_player_stat_rules_with_level_exp(
    level_exp: impl IntoIterator<Item = crate::LevelExpEntry>,
) -> crate::PlayerStatRules {
    fn class_config(
        class: PlayerClass,
        base_stats: CoreStatBlock,
    ) -> crate::PlayerClassStatsConfig {
        crate::PlayerClassStatsConfig {
            base_stats,
            stat_source: ActorStatSource::Player(PlayerStatSource {
                resources: PlayerResourceFormula {
                    base_max_hp: 600,
                    base_max_sp: 200,
                    base_max_stamina: 800,
                    hp_per_ht: 40,
                    sp_per_iq: 20,
                    stamina_per_ht: 5,
                },
                growth: PlayerGrowthFormula {
                    hp_per_level: (36, 44),
                    sp_per_level: (18, 22),
                    stamina_per_level: (5, 8),
                    version: DeterministicGrowthVersion::V1,
                },
                balance: zohar_gameplay::stats::game::default_player_balance_rules(class),
                speeds: SourceSpeeds::default(),
            }),
        }
    }

    crate::PlayerStatRules::new(
        crate::PlayerClassStatsTable::new(vec![
            (
                PlayerClass::Warrior,
                class_config(PlayerClass::Warrior, CoreStatBlock::new(6, 4, 3, 3)),
            ),
            (
                PlayerClass::Ninja,
                class_config(PlayerClass::Ninja, CoreStatBlock::new(4, 3, 6, 3)),
            ),
            (
                PlayerClass::Sura,
                class_config(PlayerClass::Sura, CoreStatBlock::new(5, 3, 3, 5)),
            ),
            (
                PlayerClass::Shaman,
                class_config(PlayerClass::Shaman, CoreStatBlock::new(3, 4, 3, 6)),
            ),
        ]),
        crate::LevelExpTable::new(level_exp),
    )
}

fn default_gameplay_bootstrap(
    player_id: PlayerId,
    class: PlayerClass,
    level: i32,
) -> PlayerGameplayBootstrap {
    PlayerGameplayBootstrap {
        player_id,
        class,
        level,
        exp_in_level: 0,
        core_stat_allocations: CoreStatAllocations::default(),
        stat_reset_count: 0,
        current_hp: None,
        current_sp: None,
        current_stamina: None,
    }
}

fn test_navigator(
    width: usize,
    height: usize,
    blocked_cells: &[(usize, usize)],
) -> Arc<MapNavigator> {
    let mut data = vec![TerrainFlags::empty(); width * height];
    for (x, y) in blocked_cells.iter().copied() {
        data[y * width + x] = TerrainFlags::BLOCK;
    }
    Arc::new(MapNavigator::new(
        TerrainFlagsGrid::new(1.0, width, height, data).expect("terrain flags grid"),
    ))
}

fn test_mob_proto(
    mob_id: MobId,
    mob_kind: MobKind,
    name: impl Into<String>,
    rank: MobRank,
    level: u32,
    move_speed: u8,
    attack_speed: u8,
    bhv_flags: BehaviorFlags,
) -> Arc<MobPrototypeDef> {
    Arc::new(MobPrototypeDef {
        mob_id,
        mob_kind,
        name: name.into(),
        rank,
        battle_type: MobBattleType::Melee,
        level,
        move_speed,
        attack_speed,
        aggressive_sight: 0,
        attack_range: 150,
        combat_extent_m: 1.0,
        combat: test_mob_combat(level),
        rewards: Default::default(),
        bhv_flags,
        empire: None,
    })
}

fn test_portal_proto(
    mob_id: MobId,
    portal_behavior: PortalBehavior,
    name: impl Into<String>,
) -> Arc<MobPrototypeDef> {
    Arc::new(MobPrototypeDef {
        mob_id,
        mob_kind: MobKind::Portal(portal_behavior),
        name: name.into(),
        rank: MobRank::Pawn,
        battle_type: MobBattleType::Melee,
        level: 1,
        move_speed: 0,
        attack_speed: 0,
        aggressive_sight: 0,
        attack_range: 0,
        combat_extent_m: 1.0,
        combat: test_mob_combat(1),
        rewards: Default::default(),
        bhv_flags: BehaviorFlags::empty(),
        empire: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn test_mob_proto_with_combat(
    mob_id: MobId,
    mob_kind: MobKind,
    name: impl Into<String>,
    rank: MobRank,
    battle_type: MobBattleType,
    level: u32,
    move_speed: u8,
    attack_speed: u8,
    aggressive_sight: u16,
    attack_range: u16,
    bhv_flags: BehaviorFlags,
) -> Arc<MobPrototypeDef> {
    test_mob_proto_with_combat_and_rewards(
        mob_id,
        mob_kind,
        name,
        rank,
        battle_type,
        level,
        move_speed,
        attack_speed,
        aggressive_sight,
        attack_range,
        bhv_flags,
        MobRewards::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn test_mob_proto_with_combat_and_rewards(
    mob_id: MobId,
    mob_kind: MobKind,
    name: impl Into<String>,
    rank: MobRank,
    battle_type: MobBattleType,
    level: u32,
    move_speed: u8,
    attack_speed: u8,
    aggressive_sight: u16,
    attack_range: u16,
    bhv_flags: BehaviorFlags,
    rewards: MobRewards,
) -> Arc<MobPrototypeDef> {
    Arc::new(MobPrototypeDef {
        mob_id,
        mob_kind,
        name: name.into(),
        rank,
        battle_type,
        level,
        move_speed,
        attack_speed,
        aggressive_sight,
        attack_range,
        combat_extent_m: 1.0,
        combat: test_mob_combat(level),
        rewards,
        bhv_flags,
        empire: None,
    })
}

fn test_mob_combat(level: u32) -> MobCombatStats {
    let level = level.min(i32::MAX as u32) as i32;
    let damage_min = 18 + level.saturating_mul(2);
    MobCombatStats {
        strength: level + 2,
        dexterity: level + 5,
        vitality: level + 4,
        intelligence: level + 1,
        damage_min,
        damage_max: damage_min + 4 + level / 4,
        max_hp: 100 + level.saturating_mul(26),
        defense: level + 3,
        damage_multiplier: 1.0,
    }
}

fn build_runtime_app(
    shared: SharedConfig,
    map: MapConfig,
    with_outbox: bool,
) -> (App, MapEventSender) {
    build_runtime_app_with_persistence(
        shared,
        map,
        PlayerPersistenceCoordinatorHandle::disabled(),
        with_outbox,
    )
}

fn build_runtime_app_with_persistence(
    shared: SharedConfig,
    map: MapConfig,
    player_persistence: PlayerPersistenceCoordinatorHandle,
    with_outbox: bool,
) -> (App, MapEventSender) {
    let (mut app, map_events) =
        build_map_app_with_options(shared, map, player_persistence, 64, with_outbox);
    app.update();
    (app, map_events)
}

fn advance_tick(app: &mut App) {
    run_pre_update(app);
    run_fixed_first(app);
    run_fixed_update(app);
    run_fixed_post_update(app);
}

fn run_pre_update(app: &mut App) {
    let _ = app.world_mut().try_run_schedule(bevy::prelude::PreUpdate);
}

fn run_fixed_first(app: &mut App) {
    app.world_mut().run_schedule(bevy::prelude::FixedFirst);
}

fn run_fixed_update(app: &mut App) {
    app.world_mut().run_schedule(bevy::prelude::FixedUpdate);
}

fn run_fixed_post_update(app: &mut App) {
    app.world_mut().run_schedule(bevy::prelude::FixedPostUpdate);
}

fn enter_player(
    map_events: &MapEventSender,
    player_id: PlayerId,
    player_net_id: EntityId,
    initial_pos: LocalPos,
) -> Receiver<PlayerEvent> {
    enter_player_with_appearance(
        map_events,
        player_id,
        player_net_id,
        initial_pos,
        PlayerAppearance::default(),
    )
}

fn enter_player_with_appearance(
    map_events: &MapEventSender,
    player_id: PlayerId,
    player_net_id: EntityId,
    initial_pos: LocalPos,
    appearance: PlayerAppearance,
) -> Receiver<PlayerEvent> {
    enter_player_with_gameplay(
        map_events,
        gameplay_bootstrap_from_appearance(player_id, &appearance),
        player_net_id,
        initial_pos,
        appearance,
    )
}

fn enter_player_with_gameplay(
    map_events: &MapEventSender,
    gameplay: PlayerGameplayBootstrap,
    player_net_id: EntityId,
    initial_pos: LocalPos,
    appearance: PlayerAppearance,
) -> Receiver<PlayerEvent> {
    map_events
        .enter_player(EnterMsg {
            player_id: gameplay.player_id,
            player_net_id,
            runtime_epoch: Default::default(),
            playtime: zohar_domain::entity::player::PlayerPlaytime::ZERO,
            initial_pos,
            gameplay,
            visual_profile: PlayerVisualProfile {
                name: appearance.name.clone(),
                gender: appearance.gender,
                empire: appearance.empire,
                body_part: appearance.body_part,
                guild_id: appearance.guild_id,
            },
        })
        .expect("player enter")
}

fn gameplay_bootstrap_from_appearance(
    player_id: PlayerId,
    appearance: &PlayerAppearance,
) -> PlayerGameplayBootstrap {
    default_gameplay_bootstrap(
        player_id,
        appearance.class,
        i32::try_from(appearance.level).unwrap_or(i32::MAX),
    )
}

fn attack_target(
    map_events: &MapEventSender,
    player_id: PlayerId,
    target: EntityId,
    attack_type: u8,
) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::Attack(AttackTargetIntent {
                target,
                attack: attack_from_test_code(attack_type),
            }),
        })
        .expect("attack intent");
}

fn select_target(map_events: &MapEventSender, player_id: PlayerId, target: EntityId) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::Target(zohar_map_port::TargetIntent { target }),
        })
        .expect("target intent");
}

fn move_player(map_events: &MapEventSender, player_id: PlayerId, target: LocalPos, ts: u32) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::Move(MoveIntent {
                kind: MovementKind::Move,
                arg: MovementArg::ZERO,
                facing: facing(0),
                target,
                client_ts: client_ts(ts),
            }),
        })
        .expect("move intent");
}

fn restart_player(map_events: &MapEventSender, player_id: PlayerId, intent: PlayerRestartIntent) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::Restart(intent),
        })
        .expect("restart intent");
}

fn _send_chat(map_events: &MapEventSender, player_id: PlayerId, message: &[u8]) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::Chat(PortChatIntent {
                channel: ChatChannel::Speak,
                message: message.to_vec(),
            }),
        })
        .expect("chat intent");
}

fn set_movement_animation(
    map_events: &MapEventSender,
    player_id: PlayerId,
    animation: MovementAnimation,
) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::SetMovementAnimation(animation),
        })
        .expect("movement animation intent");
}

fn send_core_stat_intent(
    map_events: &MapEventSender,
    player_id: PlayerId,
    stat: CoreStatKind,
    delta: i8,
) {
    map_events
        .try_send_client_intent(ClientIntentMsg {
            player_id,
            intent: ClientIntent::Progression(PlayerProgressionIntent::CoreStat(
                CoreStatAllocationIntent { stat, delta },
            )),
        })
        .expect("progression intent");
}

fn drain_player_events(rx: &mut Receiver<PlayerEvent>) -> Vec<PlayerEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn movement_events(events: &[PlayerEvent]) -> Vec<(EntityId, MovementKind)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntityMove(movement) => Some((movement.entity_id, movement.kind)),
            _ => None,
        })
        .collect()
}

fn movement_animation_events(events: &[PlayerEvent]) -> Vec<(EntityId, MovementAnimation)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::SetEntityMovementAnimation {
                entity_id,
                animation,
            } => Some((*entity_id, *animation)),
            _ => None,
        })
        .collect()
}

fn spawn_event_ids(events: &[PlayerEvent]) -> Vec<EntityId> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntitySpawn { snapshot } => Some(snapshot.entity_id),
            _ => None,
        })
        .collect()
}

fn portal_entry_destinations(events: &[PlayerEvent]) -> Vec<PortalDestination> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::PortalEntered { destination, .. } => Some(*destination),
            _ => None,
        })
        .collect()
}

fn stat_events(events: &[PlayerEvent]) -> Vec<(EntityId, Stat, i32)> {
    events
        .iter()
        .flat_map(|event| match event {
            PlayerEvent::SetEntityStats { entity_id, stats } => stats
                .iter()
                .map(|update| (*entity_id, update.stat, update.absolute))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn health_bar_events(events: &[PlayerEvent]) -> Vec<(EntityId, u8)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::SyncEntityHealthBar { entity_id, hp_pct } => Some((*entity_id, *hp_pct)),
            _ => None,
        })
        .collect()
}

fn damage_info_events(events: &[PlayerEvent]) -> Vec<(EntityId, u8, i32)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::DamageInfo {
                entity_id,
                flags,
                damage,
            } => Some((*entity_id, flags.bits(), *damage)),
            _ => None,
        })
        .collect()
}

fn special_effect_events(events: &[PlayerEvent]) -> Vec<(EntityId, SpecialEffectType)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::SpecialEffect { entity_id, effect } => Some((*entity_id, *effect)),
            _ => None,
        })
        .collect()
}

fn projectile_effect_events(
    events: &[PlayerEvent],
) -> Vec<(ProjectileEffectKind, EntityId, EntityId)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::CreateProjectileEffect {
                effect,
                start_entity_id,
                end_entity_id,
            } => Some((*effect, *start_entity_id, *end_entity_id)),
            _ => None,
        })
        .collect()
}

fn attack_movement_count(events: &[PlayerEvent], entity_id: EntityId) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                PlayerEvent::EntityMove(movement)
                    if movement.entity_id == entity_id && movement.kind == MovementKind::Attack
            )
        })
        .count()
}

fn dead_events(events: &[PlayerEvent]) -> Vec<EntityId> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntityDead { entity_id } => Some(*entity_id),
            _ => None,
        })
        .collect()
}

fn stunned_events(events: &[PlayerEvent]) -> Vec<EntityId> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntityStunned { entity_id } => Some(*entity_id),
            _ => None,
        })
        .collect()
}

fn despawn_events(events: &[PlayerEvent]) -> Vec<EntityId> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntityDespawn { entity_id } => Some(*entity_id),
            _ => None,
        })
        .collect()
}

fn public_state_change_events(events: &[PlayerEvent]) -> Vec<(EntityId, EntityPublicState)> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntityPublicStateChanged { entity_id, state } => {
                Some((*entity_id, *state))
            }
            _ => None,
        })
        .collect()
}

fn restart_town_event_count(events: &[PlayerEvent]) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, PlayerEvent::RestartTown))
        .count()
}

fn command_messages(events: &[PlayerEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::Chat {
                channel: ChatChannel::Command,
                message,
                ..
            } => Some(
                String::from_utf8_lossy(message)
                    .trim_end_matches('\0')
                    .to_string(),
            ),
            _ => None,
        })
        .collect()
}

fn test_public_state(move_speed: u8, attack_speed: u8) -> EntityPublicState {
    EntityPublicState {
        equipment: EntityPublicEquipment::default(),
        speeds: EntityPublicSpeeds {
            move_speed,
            attack_speed,
        },
        flags: EntityPublicFlags::default(),
        social: EntityPublicSocial::default(),
    }
}

fn info_messages(events: &[PlayerEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::Chat {
                channel: ChatChannel::Info,
                message,
                ..
            } => Some(
                String::from_utf8_lossy(message)
                    .trim_end_matches('\0')
                    .to_string(),
            ),
            _ => None,
        })
        .collect()
}

fn pending_snapshot(player_id: PlayerId) -> PlayerSnapshot {
    PlayerSnapshot {
        runtime: PlayerRuntimeSnapshot {
            id: player_id,
            runtime_epoch: Default::default(),
            map_key: "queued_map".to_string(),
            local_pos: LocalPos::new(1.0, 1.0),
            playtime: PlayerPlaytime::ZERO,
            current_hp: None,
            current_sp: None,
            current_stamina: None,
        },
        progression: PlayerProgressionSnapshot {
            level: 1,
            exp_in_level: 0,
            core_stat_allocations: CoreStatAllocations::default(),
            stat_reset_count: 0,
        },
    }
}

fn first_mob_entity(app: &mut App) -> Entity {
    let world = app.world_mut();
    let mut q = world.query::<(Entity, &MobMarker)>();
    q.iter(world)
        .next()
        .map(|(entity, _)| entity)
        .expect("mob entity")
}

fn first_mob_net_id(app: &mut App) -> EntityId {
    let world = app.world_mut();
    let mut q = world.query::<(&MobMarker, &NetEntityId)>();
    q.iter(world)
        .next()
        .map(|(_, net_id)| net_id.net_id)
        .expect("mob net id")
}

fn mob_net_ids(app: &mut App) -> Vec<EntityId> {
    let world = app.world_mut();
    let mut q = world.query::<(&MobMarker, &NetEntityId)>();
    q.iter(world).map(|(_, net_id)| net_id.net_id).collect()
}

fn mob_hp(app: &App, mob_entity: Entity) -> i32 {
    app.world()
        .entity(mob_entity)
        .get::<MobStatsComp>()
        .expect("mob stats")
        .0
        .read_packet(Stat::Hp)
}

fn player_hp(app: &App, player_id: PlayerId) -> i32 {
    let entity = player_entity(app, player_id);
    app.world()
        .entity(entity)
        .get::<PlayerStatsComp>()
        .expect("player stats")
        .0
        .read_packet(Stat::Hp)
}

fn player_max_hp(app: &App, player_id: PlayerId) -> i32 {
    let entity = player_entity(app, player_id);
    app.world()
        .entity(entity)
        .get::<PlayerStatsComp>()
        .expect("player stats")
        .0
        .read_packet(Stat::MaxHp)
}

fn player_pos(app: &App, player_id: PlayerId) -> LocalPos {
    let entity = player_entity(app, player_id);
    app.world()
        .entity(entity)
        .get::<LocalTransform>()
        .expect("player transform")
        .pos
}

fn set_player_progression_for_test(
    app: &mut App,
    player_id: PlayerId,
    level: i32,
    exp_in_level: u32,
    next_exp_in_level: u32,
) {
    let player_entity = player_entity(app, player_id);
    let mut entity = app.world_mut().entity_mut(player_entity);
    entity
        .get_mut::<PlayerProgressionComp>()
        .expect("player progression")
        .0
        .level = level;
    entity
        .get_mut::<PlayerProgressionComp>()
        .expect("player progression")
        .0
        .exp_in_level = i64::from(exp_in_level);
    entity
        .get_mut::<PlayerStatsComp>()
        .expect("player stats")
        .0
        .with_api_mut(|api| {
            api.set_player_progression(PlayerProgressionState::new(
                level,
                exp_in_level,
                next_exp_in_level,
            ));
        });
}

fn set_player_resource_for_test(app: &mut App, player_id: PlayerId, stat: Stat, value: i32) {
    let player_entity = player_entity(app, player_id);
    app.world_mut()
        .entity_mut(player_entity)
        .get_mut::<PlayerStatsComp>()
        .expect("player stats")
        .0
        .with_api_mut(|api| api.set_resource(stat, value).expect("set player resource"));
}

fn dead_player_runtime(
    mob_name: &'static str,
    player_net_id: EntityId,
) -> (
    App,
    MapEventSender,
    Receiver<PlayerEvent>,
    PlayerId,
    EntityId,
) {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            mob_name,
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );

    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);
    let mob_entity = first_mob_entity(&mut app);
    let player_id = PlayerId::from(1);
    let mut gameplay = default_gameplay_bootstrap(player_id, PlayerClass::Warrior, 1);
    gameplay.current_hp = Some(1);
    let mut rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay,
        player_net_id,
        LocalPos::new(10.0, 10.0),
        PlayerAppearance::default(),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx);

    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(10.5, 10.0),
        east_rot(),
        1_000,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, 1_000, 0);
    run_mob_ai(&mut app);
    run_fixed_post_update(&mut app);
    let events = drain_player_events(&mut rx);
    assert!(
        stunned_events(&events).contains(&player_net_id),
        "lethal mob attack should put the player into dying/stun, got {events:?}"
    );

    set_sim_now(&mut app, 10_000);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);
    assert!(
        dead_events(&events).contains(&player_net_id),
        "dying player should transition to dead, got {events:?}"
    );

    (app, inbound_tx, rx, player_id, player_net_id)
}

fn kill_mob_and_finalize_death(
    app: &mut App,
    inbound_tx: &MapEventSender,
    rx: &mut Receiver<PlayerEvent>,
    player_id: PlayerId,
    mob_net_id: EntityId,
    finalized_at_ms: u64,
) -> Vec<PlayerEvent> {
    let mut events = Vec::new();
    for _ in 0..64 {
        attack_target(inbound_tx, player_id, mob_net_id, 0);
        advance_tick(app);
        events.extend(drain_player_events(rx));
        if stunned_events(&events).contains(&mob_net_id) {
            break;
        }
    }
    assert!(
        stunned_events(&events).contains(&mob_net_id),
        "test mob should enter dying state before reward finalization, got {events:?}"
    );

    set_player_resource_for_test(app, player_id, Stat::Hp, 1);
    set_sim_now(app, finalized_at_ms);
    run_actor_lifecycle(app);
    events.extend(drain_player_events(rx));
    events
}

fn map_entity(app: &App) -> Entity {
    app.world()
        .resource::<RuntimeState>()
        .map_entity
        .expect("map entity")
}

fn pending_movements(app: &App) -> Vec<PendingMovement> {
    app.world()
        .entity(map_entity(app))
        .get::<MapPendingMovements>()
        .map(|pending| pending.0.clone())
        .unwrap_or_default()
}

fn clear_pending_movements(app: &mut App) {
    let map_entity = map_entity(app);
    app.world_mut()
        .entity_mut(map_entity)
        .get_mut::<MapPendingMovements>()
        .expect("map pending movements")
        .0
        .clear();
}

fn player_entity(app: &App, player_id: PlayerId) -> Entity {
    app.world()
        .resource::<PlayerIndex>()
        .0
        .get(&player_id)
        .copied()
        .expect("player entity")
}

fn run_mob_ai(app: &mut App) {
    super::mob_motion::sample_mob_motion(app.world_mut());
    super::mob_ai::process_mob_ai(app.world_mut());
    super::action_pipeline::process_actions(app.world_mut());
    super::combat::process_attack_commands(app.world_mut());
    super::actor_life::process_life_events(app.world_mut());
    super::actor_life::process_actor_lifecycle(app.world_mut());
    super::player::restart::process_player_restarts(app.world_mut());
    super::cleanup::process_cleanup_events(app.world_mut());
    super::player::persistence::process_player_dirty_events(app.world_mut());
    super::projection::project_frame_facts(app.world_mut());
}

fn run_actor_lifecycle(app: &mut App) {
    super::actor_life::process_life_events(app.world_mut());
    super::rewards::record_mob_death_reward_claims(app.world_mut());
    super::actor_life::process_actor_lifecycle(app.world_mut());
    super::player::restart::process_player_restarts(app.world_mut());
    super::rewards::grant_mob_death_rewards(app.world_mut());
    super::cleanup::process_cleanup_events(app.world_mut());
    super::player::persistence::process_player_dirty_events(app.world_mut());
    super::projection::project_frame_facts(app.world_mut());
    run_fixed_post_update(app);
}

fn east_rot() -> Facing72 {
    super::util::rotation_from_delta(LocalPos::new(1.0, 1.0), LocalPos::new(2.0, 1.0), facing(0))
}

fn north_rot() -> Facing72 {
    super::util::rotation_from_delta(LocalPos::new(1.0, 1.0), LocalPos::new(1.0, 2.0), facing(0))
}

fn set_stationary_mob(
    app: &mut App,
    mob_entity: Entity,
    pos: LocalPos,
    rot: Facing72,
    now_ms: u64,
) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    entity.get_mut::<LocalTransform>().expect("transform").pos = pos;
    entity.get_mut::<LocalTransform>().expect("transform").rot = rot;
    entity.get_mut::<MobMotion>().expect("mob motion").0 = MobMotionState {
        segment_start_pos: pos,
        segment_end_pos: pos,
        segment_start_at: sim_ms(now_ms),
        segment_end_at: sim_ms(now_ms),
    };
}

fn set_mob_chasing(
    app: &mut App,
    mob_entity: Entity,
    target: EntityId,
    now_ms: u64,
    next_attack_at_ms: u64,
) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    *entity.get_mut::<MobBrainState>().expect("brain") = MobBrainState {
        mode: MobBrainMode::Pursuit,
        target: Some(target),
        next_attack_at: sim_ms(next_attack_at_ms),
        attack_windup_until: super::state::SimInstant::ZERO,
        next_rethink_at: sim_ms(now_ms),
        ..MobBrainState::default()
    };
}

fn set_mob_returning(app: &mut App, mob_entity: Entity, now_ms: u64) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    *entity.get_mut::<MobBrainState>().expect("brain") = MobBrainState {
        mode: MobBrainMode::Return,
        target: None,
        next_rethink_at: sim_ms(now_ms),
        ..MobBrainState::default()
    };
}

fn set_mob_idle(app: &mut App, mob_entity: Entity, now_ms: u64) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    *entity.get_mut::<MobBrainState>().expect("brain") = MobBrainState {
        mode: MobBrainMode::Idle,
        target: None,
        wander_next_decision_at: sim_ms(now_ms),
        wander_wait_until: None,
        ..MobBrainState::default()
    };
}

fn sample_test_wander_candidate(
    rng: &mut SmallRng,
    current_pos: LocalPos,
    step_m: f32,
) -> LocalPos {
    let heading = LocalRotation::radians(rng.random_range(0.0..TAU));
    current_pos.shifted(heading, LocalDistMeters::new(step_m))
}

fn test_pos_inside_map(map_size: LocalSize, candidate: LocalPos) -> bool {
    candidate.x.is_finite()
        && candidate.y.is_finite()
        && candidate.x >= 0.0
        && candidate.y >= 0.0
        && candidate.x < map_size.width
        && candidate.y < map_size.height
}

fn find_seed_with_blocked_first_wander_candidate(
    map_size: LocalSize,
    navigator: &MapNavigator,
    current_pos: LocalPos,
    step_m: f32,
) -> (u64, LocalPos) {
    for seed in 0..100_000 {
        let mut rng = SmallRng::seed_from_u64(seed);
        let _ = rng.random_range(0..1);
        let first_candidate = sample_test_wander_candidate(&mut rng, current_pos, step_m);
        if !test_pos_inside_map(map_size, first_candidate)
            || navigator.segment_clear(current_pos, first_candidate)
        {
            continue;
        }

        let has_clear_retry = (0..7).any(|_| {
            let retry_candidate = sample_test_wander_candidate(&mut rng, current_pos, step_m);
            test_pos_inside_map(map_size, retry_candidate)
                && navigator.can_stand(retry_candidate)
                && navigator.segment_clear(current_pos, retry_candidate)
        });
        if has_clear_retry {
            return (seed, first_candidate);
        }
    }

    panic!("expected a deterministic wander seed with a blocked first candidate");
}

#[test]
fn sample_player_motion_at_interpolates_within_segment() {
    let mut motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(0.0, 0.0),
        segment_end_pos: LocalPos::new(10.0, 0.0),
        segment_start_ts: client_ts(100),
        segment_end_ts: client_ts(200),
        last_client_ts: client_ts(101),
    };

    let pos = sample_player_motion_at(LocalPos::new(0.0, 0.0), &mut motion, 150);
    assert!((pos.x - 5.0).abs() < 0.01);
    assert!((pos.y - 0.0).abs() < 0.01);
}

#[test]
fn sample_player_motion_at_clamps_to_segment_end_after_overshoot() {
    let mut motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(0.0, 0.0),
        segment_end_pos: LocalPos::new(10.0, 0.0),
        segment_start_ts: client_ts(100),
        segment_end_ts: client_ts(200),
        last_client_ts: client_ts(101),
    };

    assert_eq!(
        sample_player_motion_at(LocalPos::new(0.0, 0.0), &mut motion, 250),
        LocalPos::new(10.0, 0.0)
    );
}

#[test]
fn sample_player_motion_at_keeps_current_pos_for_stale_ts() {
    let mut motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(0.0, 0.0),
        segment_end_pos: LocalPos::new(10.0, 0.0),
        segment_start_ts: client_ts(100),
        segment_end_ts: client_ts(200),
        last_client_ts: client_ts(175),
    };

    let pos = sample_player_motion_at(LocalPos::new(7.5, 0.0), &mut motion, 150);
    assert!((pos.x - 7.5).abs() < 0.01);
    assert!((pos.y - 0.0).abs() < 0.01);
}

#[test]
fn sample_player_visual_position_at_uses_segment_progress_not_latest_endpoint() {
    let motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(0.0, 0.0),
        segment_end_pos: LocalPos::new(10.0, 0.0),
        segment_start_ts: client_ts(100),
        segment_end_ts: client_ts(200),
        last_client_ts: client_ts(200),
    };

    let pos = super::util::sample_player_visual_position_at(motion, client_ts(150));
    assert!((pos.x - 5.0).abs() < 0.01);
    assert!((pos.y - 0.0).abs() < 0.01);
}

#[test]
fn startup_ready_signal_fires_after_map_bootstrap() {
    let map_id = MapId::new(41);
    let (shared, map) = test_configs(MapInstanceKey::shared(1, map_id));
    let (startup_tx, startup_rx) = tokio::sync::oneshot::channel();
    let (mut app, _map_events) = build_map_app_with_options(
        shared,
        map,
        PlayerPersistenceCoordinatorHandle::disabled(),
        16,
        false,
    );
    app.insert_resource(StartupReadySignal::new(startup_tx));
    app.update();

    assert!(startup_rx.blocking_recv().is_ok());
}

#[test]
fn player_enter_enqueues_self_spawn_snapshot_once() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_101);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        guild_id: 42,
        level: 37,
        ..Default::default()
    };

    let mut map_rx = enter_player_with_appearance(
        &inbound_tx,
        player_id,
        player_net_id,
        initial_pos,
        appearance.clone(),
    );

    advance_tick(&mut app);

    let events = drain_player_events(&mut map_rx);
    assert_eq!(spawn_event_ids(&events), vec![player_net_id]);

    let self_spawn = events
        .iter()
        .find_map(|event| match event {
            PlayerEvent::EntitySpawn { snapshot } if snapshot.entity_id == player_net_id => {
                Some(snapshot)
            }
            _ => None,
        })
        .expect("self spawn event");

    assert_eq!(self_spawn.pos, initial_pos);
    assert_eq!(
        self_spawn.public_state.speeds.move_speed,
        appearance.move_speed
    );
    assert_eq!(
        self_spawn.public_state.speeds.attack_speed,
        appearance.attack_speed
    );
    assert!(matches!(
        self_spawn.kind,
        EntityKind::Player {
            class,
            gender,
        } if class == appearance.class && gender == appearance.gender
    ));
    let nameplate = self_spawn.nameplate.as_ref().expect("self spawn nameplate");
    assert_eq!(nameplate.name, appearance.name);
    assert_eq!(
        self_spawn.public_state.equipment.body_part,
        appearance.body_part
    );
    assert_eq!(nameplate.empire, Some(appearance.empire));
    assert_eq!(self_spawn.public_state.social.guild_id, appearance.guild_id);
    assert_eq!(nameplate.level, appearance.level);

    advance_tick(&mut app);
    let followup_events = drain_player_events(&mut map_rx);
    assert!(
        spawn_event_ids(&followup_events).is_empty(),
        "self should be bootstrapped once and stay excluded from AOI diffs"
    );
}

#[test]
fn core_stat_progression_intent_emits_stat_updates() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_102);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        level: 2,
        ..Default::default()
    };
    let mut gameplay = gameplay_bootstrap_from_appearance(player_id, &appearance);
    gameplay.current_hp = Some(640);

    let mut map_rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay,
        player_net_id,
        initial_pos,
        appearance,
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    send_core_stat_intent(&inbound_tx, player_id, CoreStatKind::St, 1);
    advance_tick(&mut app);

    let events = drain_player_events(&mut map_rx);
    let stat_events = stat_events(&events);

    assert!(
        stat_events
            .iter()
            .any(|(entity_id, _, absolute)| { *entity_id == player_net_id && *absolute == 7 }),
        "expected core stat increment event, got {stat_events:?}"
    );
    assert!(
        stat_events
            .iter()
            .any(|(entity_id, _, absolute)| { *entity_id == player_net_id && *absolute == 2 }),
        "expected stat point decrement event, got {stat_events:?}"
    );
}

#[test]
fn speed_stat_sync_replicates_public_state_change_to_visible_players() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_102);
    let bob_id = PlayerId::from(2);

    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(
        &inbound_tx,
        bob_id,
        EntityId(5_103),
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    {
        let entity = player_entity(&app, alice_id);
        let mut entity = app.world_mut().entity_mut(entity);
        let mut stats = entity.get_mut::<PlayerStatsComp>().expect("player stats");
        stats
            .0
            .with_api_mut(|api| {
                api.replace_source_bundle(
                    (),
                    CompiledStatContribution::new()
                        .with_modifier(CompiledModifier::plain(Stat::MovSpeed, 25))
                        .with_modifier(CompiledModifier::plain(Stat::AttSpeed, 10)),
                )
            })
            .expect("speed source bundle");
    }

    advance_tick(&mut app);

    let alice_updates = public_state_change_events(&drain_player_events(&mut alice_rx));
    let bob_updates = public_state_change_events(&drain_player_events(&mut bob_rx));

    assert!(
        alice_updates.iter().any(|(entity_id, state)| {
            *entity_id == alice_net_id
                && state.speeds.move_speed == 125
                && state.speeds.attack_speed == 110
        }),
        "subject should receive its own public-state speed change, got {alice_updates:?}"
    );
    assert!(
        bob_updates.iter().any(|(entity_id, state)| {
            *entity_id == alice_net_id
                && state.speeds.move_speed == 125
                && state.speeds.attack_speed == 110
        }),
        "visible observers should receive public-state speed change, got {bob_updates:?}"
    );
}

#[test]
fn level_step_stat_sync_stays_subject_only() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_102);
    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(
        &inbound_tx,
        PlayerId::from(2),
        EntityId(5_103),
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    {
        let entity = player_entity(&app, alice_id);
        let mut entity = app.world_mut().entity_mut(entity);
        let mut stats = entity.get_mut::<PlayerStatsComp>().expect("player stats");
        stats.0.with_api_mut(|api| {
            api.set_player_progression(PlayerProgressionState::new(1, 25, 100));
        });
    }

    advance_tick(&mut app);
    let alice_stats = stat_events(&drain_player_events(&mut alice_rx));
    let bob_stats = stat_events(&drain_player_events(&mut bob_rx));
    assert!(alice_stats.contains(&(alice_net_id, Stat::LevelStep, 1)));
    assert!(
        !bob_stats.contains(&(alice_net_id, Stat::LevelStep, 1)),
        "observer level-step visuals should be explicit point effects, got {bob_stats:?}"
    );
}

#[test]
fn point_visual_effects_drive_observer_level_step_and_level_up_packets() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_102);
    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(
        &inbound_tx,
        PlayerId::from(2),
        EntityId(5_103),
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    let alice_entity = player_entity(&app, alice_id);
    app.world_mut()
        .resource_mut::<FrameFacts>()
        .visuals
        .point_effects
        .push(PointVisualEffect::LevelStep {
            actor: ActorRef::new(alice_entity, alice_net_id),
        });
    advance_tick(&mut app);
    let alice_stats = stat_events(&drain_player_events(&mut alice_rx));
    let bob_stats = stat_events(&drain_player_events(&mut bob_rx));
    assert!(!alice_stats.contains(&(alice_net_id, Stat::LevelStep, 0)));
    assert!(
        bob_stats.contains(&(alice_net_id, Stat::LevelStep, 0)),
        "visible observers should receive level-step point effects, got {bob_stats:?}"
    );

    app.world_mut()
        .resource_mut::<FrameFacts>()
        .visuals
        .point_effects
        .push(PointVisualEffect::LevelUp {
            actor: ActorRef::new(alice_entity, alice_net_id),
            level: 2,
        });
    advance_tick(&mut app);
    let alice_stats = stat_events(&drain_player_events(&mut alice_rx));
    let bob_stats = stat_events(&drain_player_events(&mut bob_rx));
    assert!(!alice_stats.contains(&(alice_net_id, Stat::Level, 2)));
    assert!(
        bob_stats.contains(&(alice_net_id, Stat::Level, 2)),
        "visible observers should receive level-up point effects with the new absolute level for nameplates, got {bob_stats:?}"
    );
}

#[test]
fn special_effects_use_actor_view_and_subject_fanout() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_102);
    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(
        &inbound_tx,
        PlayerId::from(2),
        EntityId(5_103),
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    let alice_entity = player_entity(&app, alice_id);
    app.world_mut()
        .resource_mut::<FrameFacts>()
        .visuals
        .special_effects
        .push(ActorSpecialEffect {
            actor: ActorRef::new(alice_entity, alice_net_id),
            effect: ActorSpecialEffectKind::Critical,
        });
    advance_tick(&mut app);

    let alice_effects = special_effect_events(&drain_player_events(&mut alice_rx));
    let bob_effects = special_effect_events(&drain_player_events(&mut bob_rx));
    assert!(alice_effects.contains(&(alice_net_id, SpecialEffectType::Critical)));
    assert!(
        bob_effects.contains(&(alice_net_id, SpecialEffectType::Critical)),
        "visible observers should receive actor special effects, got {bob_effects:?}"
    );
}

#[test]
fn critical_damage_projects_legacy_critical_special_effect() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_102);
    let bob_id = PlayerId::from(2);
    let bob_net_id = EntityId(5_103);
    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(&inbound_tx, bob_id, bob_net_id, LocalPos::new(10.5, 10.0));
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    let alice_entity = player_entity(&app, alice_id);
    let bob_entity = player_entity(&app, bob_id);
    app.world_mut()
        .resource_mut::<FrameFacts>()
        .combat
        .damaged
        .push(ActorDamaged {
            attacker: ActorRef::new(alice_entity, alice_net_id),
            victim: ActorRef::new(bob_entity, bob_net_id),
            damage: 11,
            flags: HitFlags::NORMAL | HitFlags::CRITICAL,
        });
    advance_tick(&mut app);

    let alice_effects = special_effect_events(&drain_player_events(&mut alice_rx));
    let bob_effects = special_effect_events(&drain_player_events(&mut bob_rx));
    assert!(alice_effects.contains(&(bob_net_id, SpecialEffectType::Critical)));
    assert!(bob_effects.contains(&(bob_net_id, SpecialEffectType::Critical)));
}

#[test]
fn dirty_public_states_coalesce_by_entity_and_materialize_latest_state() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_net_id = EntityId(5_102);
    let bob_net_id = EntityId(5_103);
    let mut alice_rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(
        &inbound_tx,
        PlayerId::from(2),
        bob_net_id,
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    {
        let alice_entity = player_entity(&app, PlayerId::from(1));
        let mut alice = app.world_mut().entity_mut(alice_entity);
        let mut appearance = alice
            .get_mut::<super::state::PlayerAppearanceComp>()
            .expect("alice appearance");
        appearance.0.move_speed = 145;
        appearance.0.attack_speed = 130;
    }

    {
        let map_entity = map_entity(&app);
        let mut map_entity = app.world_mut().entity_mut(map_entity);
        let mut dirty = map_entity
            .get_mut::<MapDirtyEntityPublicStates>()
            .expect("dirty public states");
        dirty.mark_dirty(alice_net_id);
        dirty.mark_dirty(alice_net_id);
    }

    run_fixed_post_update(&mut app);

    let bob_updates = public_state_change_events(&drain_player_events(&mut bob_rx));
    let alice_updates_for_bob: Vec<_> = bob_updates
        .into_iter()
        .filter(|(entity_id, _)| *entity_id == alice_net_id)
        .collect();

    assert_eq!(
        alice_updates_for_bob,
        vec![(alice_net_id, test_public_state(145, 130))]
    );
}

#[test]
fn deferred_dirty_marker_for_removed_player_is_ignored() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_107);
    let mut map_rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(10.0, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    let player_entity = player_entity(&app, player_id);
    super::player::persistence::mark_player_dirty(app.world_mut(), player_entity);
    super::players::handle_player_leave(
        app.world_mut(),
        LeaveMsg {
            player_id,
            player_net_id,
        },
    );

    assert!(!app.world().entities().contains(player_entity));
    super::player::persistence::process_player_dirty_events(app.world_mut());
}

#[test]
fn passive_hp_recovery_emits_stat_update_when_cadence_is_due() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1_000);
    let player_net_id = EntityId(5_104);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        level: 2,
        ..Default::default()
    };
    let mut gameplay = gameplay_bootstrap_from_appearance(player_id, &appearance);
    gameplay.current_hp = Some(500);
    gameplay.current_sp = Some(100);

    let mut map_rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay,
        player_net_id,
        initial_pos,
        appearance,
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    {
        let entity = player_entity(&app, player_id);
        let mut entity = app.world_mut().entity_mut(entity);
        let mut ticker = entity
            .get_mut::<PlayerStatTickerComp>()
            .expect("player stat ticker");
        ticker.passive_hp.clock =
            SimTickerClock::scheduled(sim_ms(0), sim_ms(1_000), SimDuration::from_millis(250));
        ticker.passive_sp.clock =
            SimTickerClock::scheduled(sim_ms(0), sim_ms(1_000), SimDuration::from_millis(250));
    }

    set_sim_now(&mut app, 999);
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);
    assert!(
        stat_events(&drain_player_events(&mut map_rx)).is_empty(),
        "passive recovery should not emit before its cadence is due"
    );

    set_sim_now(&mut app, 1_000);
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);
    let stat_events = stat_events(&drain_player_events(&mut map_rx));
    assert!(
        stat_events.iter().any(|(entity_id, stat, absolute)| {
            *entity_id == player_net_id && *stat == Stat::Hp && *absolute > 500
        }),
        "expected passive HP recovery stat update, got {stat_events:?}"
    );
    assert!(
        stat_events.iter().any(|(entity_id, stat, absolute)| {
            *entity_id == player_net_id && *stat == Stat::Sp && *absolute > 100
        }),
        "expected passive SP recovery stat update, got {stat_events:?}"
    );
}

#[test]
fn passive_stamina_recovery_restores_after_legacy_stop_delay() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1_001);
    let player_net_id = EntityId(5_105);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        level: 2,
        ..Default::default()
    };
    let mut gameplay = gameplay_bootstrap_from_appearance(player_id, &appearance);
    gameplay.current_stamina = Some(400);

    let mut map_rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay,
        player_net_id,
        initial_pos,
        appearance,
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    {
        let entity = player_entity(&app, player_id);
        let mut entity = app.world_mut().entity_mut(entity);
        let mut stats = entity.get_mut::<PlayerStatsComp>().expect("stats");
        stats
            .0
            .with_api_mut(|api| api.set_resource(Stat::Stamina, 400).expect("set stamina"));
        let _ = stats.0.drain_sync();
        drop(stats);

        let mut activity = entity.get_mut::<PlayerActivityComp>().expect("activity");
        activity.last_movement_start_at = Some(sim_ms(0));
        drop(activity);

        let mut ticker = entity
            .get_mut::<PlayerStatTickerComp>()
            .expect("player stat ticker");
        ticker.stamina.clock =
            SimTickerClock::scheduled(sim_ms(0), sim_ms(2_999), SimDuration::from_millis(250));
    }

    set_sim_now(&mut app, 2_999);
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);
    assert!(
        stat_events(&drain_player_events(&mut map_rx)).is_empty(),
        "stamina should not restore before the legacy stopped delay"
    );

    {
        let entity = player_entity(&app, player_id);
        let mut entity = app.world_mut().entity_mut(entity);
        let mut ticker = entity
            .get_mut::<PlayerStatTickerComp>()
            .expect("player stat ticker");
        ticker.stamina.clock =
            SimTickerClock::scheduled(sim_ms(0), sim_ms(3_000), SimDuration::from_millis(250));
    }

    set_sim_now(&mut app, 3_000);
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);
    let stat_events = stat_events(&drain_player_events(&mut map_rx));
    assert!(
        stat_events.iter().any(|(entity_id, stat, absolute)| {
            *entity_id == player_net_id && *stat == Stat::Stamina && *absolute > 400
        }),
        "expected passive stamina recovery stat update, got {stat_events:?}"
    );
}

#[test]
fn stamina_depletion_movement_animation_replicates_to_observers() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1_001);
    let alice_net_id = EntityId(5_105);
    let bob_id = PlayerId::from(1_002);
    let bob_net_id = EntityId(5_106);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        level: 2,
        ..Default::default()
    };
    let mut gameplay = gameplay_bootstrap_from_appearance(alice_id, &appearance);
    gameplay.current_stamina = Some(1);

    let mut alice_rx =
        enter_player_with_gameplay(&inbound_tx, gameplay, alice_net_id, initial_pos, appearance);
    let mut bob_rx = enter_player(
        &inbound_tx,
        bob_id,
        bob_net_id,
        LocalPos::new(6_401.0, 6_400.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    {
        let entity = player_entity(&app, alice_id);
        let mut entity = app.world_mut().entity_mut(entity);
        let mut stats = entity.get_mut::<PlayerStatsComp>().expect("stats");
        stats
            .0
            .with_api_mut(|api| api.set_resource(Stat::Stamina, 1).expect("set stamina"));
        let _ = stats.0.drain_sync();
        drop(stats);

        entity.insert(PlayerMotion(PlayerMotionState {
            segment_start_pos: initial_pos,
            segment_end_pos: LocalPos::new(6_404.0, 6_400.0),
            segment_start_ts: client_ts(0),
            segment_end_ts: client_ts(2_000),
            last_client_ts: client_ts(0),
        }));
        entity.insert(PlayerMovementAnimation(MovementAnimation::Run));

        let mut activity = entity.get_mut::<PlayerActivityComp>().expect("activity");
        activity.last_movement_start_at = Some(sim_ms(0));
        activity.last_attack_at = Some(sim_ms(500));
        activity.preferred_movement_animation = MovementAnimation::Run;
        drop(activity);

        let mut ticker = entity
            .get_mut::<PlayerStatTickerComp>()
            .expect("player stat ticker");
        ticker.stamina.clock =
            SimTickerClock::scheduled(sim_ms(0), sim_ms(1_000), SimDuration::from_millis(250));
    }

    set_sim_now(&mut app, 1_000);
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);

    assert_eq!(
        movement_animation_events(&drain_player_events(&mut alice_rx)),
        vec![(alice_net_id, MovementAnimation::Walk)]
    );
    assert_eq!(
        movement_animation_events(&drain_player_events(&mut bob_rx)),
        vec![(alice_net_id, MovementAnimation::Walk)]
    );
}

#[test]
fn core_stat_progression_reports_cap_feedback() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_103);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        level: 30,
        ..Default::default()
    };
    let mut gameplay = gameplay_bootstrap_from_appearance(player_id, &appearance);
    gameplay.core_stat_allocations.allocated_str = 83;

    let mut map_rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay,
        player_net_id,
        initial_pos,
        appearance,
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    send_core_stat_intent(&inbound_tx, player_id, CoreStatKind::St, 1);
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    send_core_stat_intent(&inbound_tx, player_id, CoreStatKind::St, 1);
    advance_tick(&mut app);

    let events = drain_player_events(&mut map_rx);
    assert!(stat_events(&events).is_empty());
    assert!(
        info_messages(&events)
            .iter()
            .any(|message| message.contains("max (90)"))
    );
}

#[test]
fn core_stat_progression_does_not_apply_when_flush_enqueue_fails() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (handle, _rx) = player_persistence_channel(1);
    handle
        .try_schedule_autosave(pending_snapshot(PlayerId::from(999)))
        .expect("prefill persistence queue");
    let (mut app, inbound_tx) = build_runtime_app_with_persistence(shared, map, handle, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_104);
    let initial_pos = LocalPos::new(6_400.0, 6_400.0);
    let appearance = PlayerAppearance {
        name: "Alice".to_string(),
        level: 2,
        ..Default::default()
    };

    let mut map_rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay_bootstrap_from_appearance(player_id, &appearance),
        player_net_id,
        initial_pos,
        appearance,
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    send_core_stat_intent(&inbound_tx, player_id, CoreStatKind::St, 1);
    advance_tick(&mut app);

    let events = drain_player_events(&mut map_rx);
    assert!(
        stat_events(&events).is_empty(),
        "unexpected stat events: {events:?}"
    );
    assert!(
        info_messages(&events)
            .iter()
            .any(|message| message.contains("queue failure")),
        "expected queue failure feedback, got {events:?}"
    );
}

#[test]
fn player_enter_bootstraps_existing_visible_entities_before_fixed_update() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.0, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto(
            mob_id,
            MobKind::Monster,
            "bootstrap_wolf",
            MobRank::Pawn,
            1,
            100,
            100,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let existing_player_net_id = EntityId(5_104);
    let mut existing_rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        existing_player_net_id,
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut existing_rx);

    let mob_net_id = first_mob_net_id(&mut app);
    let new_player_net_id = EntityId(5_105);
    let mut map_rx = enter_player(
        &inbound_tx,
        PlayerId::from(2),
        new_player_net_id,
        LocalPos::new(10.25, 10.0),
    );

    run_pre_update(&mut app);

    let spawn_ids = spawn_event_ids(&drain_player_events(&mut map_rx));
    assert!(spawn_ids.contains(&new_player_net_id));
    assert!(spawn_ids.contains(&existing_player_net_id));
    assert!(spawn_ids.contains(&mob_net_id));
    assert_eq!(
        spawn_ids
            .iter()
            .filter(|&&entity_id| entity_id == new_player_net_id)
            .count(),
        1,
        "self spawn must not be duplicated by bootstrap AOI"
    );
}

#[test]
fn two_players_each_receive_self_spawn_and_one_peer_spawn_without_duplicate_self() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_102);
    let bob_id = PlayerId::from(2);
    let bob_net_id = EntityId(5_103);

    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(&inbound_tx, bob_id, bob_net_id, LocalPos::new(10.5, 10.0));

    advance_tick(&mut app);

    let alice_spawn_ids = spawn_event_ids(&drain_player_events(&mut alice_rx));
    let bob_spawn_ids = spawn_event_ids(&drain_player_events(&mut bob_rx));

    assert_eq!(alice_spawn_ids, vec![alice_net_id, bob_net_id]);
    assert_eq!(bob_spawn_ids, vec![bob_net_id, alice_net_id]);
}

#[test]
fn player_move_ignores_navigation_blockers_in_pre_alpha_policy() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, mut map) = test_configs(map_key);
    map.navigator = Some(test_navigator(16, 16, &[(5, 0)]));
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_101);
    let _rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(0.0, 0.0),
    );
    advance_tick(&mut app);

    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    move_player(&inbound_tx, player_id, LocalPos::new(5.0, 0.0), 1_000);

    advance_tick(&mut app);

    let transform = app
        .world()
        .entity(player_entity)
        .get::<LocalTransform>()
        .expect("transform");
    assert_eq!(transform.pos, LocalPos::new(5.0, 0.0));
}

#[test]
fn moving_into_map_transfer_portal_emits_portal_entry_event() {
    let map_id = MapId::new(41);
    let portal_mob_id = MobId::new(19_001);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    let portal_pos = LocalPos::new(100.0, 100.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(portal_mob_id),
        area: SpawnArea::new(portal_pos, LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        portal_mob_id,
        test_portal_proto(
            portal_mob_id,
            PortalBehavior::MapTransfer,
            "Yayang_Area 4002 8995",
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let mut map_rx = enter_player(
        &inbound_tx,
        player_id,
        EntityId(5_101),
        LocalPos::new(94.0, 100.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    set_sim_now(&mut app, 1_000);
    move_player(&inbound_tx, player_id, portal_pos, 1_000);
    advance_tick(&mut app);

    assert!(
        portal_entry_destinations(&drain_player_events(&mut map_rx)).is_empty(),
        "portal entry should wait until the in-flight position reaches the trigger"
    );

    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    let motion = app
        .world()
        .entity(player_entity)
        .get::<PlayerMotion>()
        .expect("player motion")
        .0;
    set_sim_now(&mut app, u64::from(motion.segment_end_ts.get()));
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);

    assert_eq!(
        portal_entry_destinations(&drain_player_events(&mut map_rx)),
        vec![PortalDestination::MapTransfer {
            world_pos: zohar_domain::coords::WorldPos::new(4002.0, 8995.0),
        }]
    );

    advance_tick(&mut app);
    assert!(
        portal_entry_destinations(&drain_player_events(&mut map_rx)).is_empty(),
        "portal entry should only emit once while the player remains inside the trigger"
    );
}

#[test]
fn moving_across_map_transfer_portal_segment_waits_for_visual_entry() {
    let map_id = MapId::new(41);
    let portal_mob_id = MobId::new(19_011);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    let portal_pos = LocalPos::new(100.0, 100.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(portal_mob_id),
        area: SpawnArea::new(portal_pos, LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        portal_mob_id,
        test_portal_proto(
            portal_mob_id,
            PortalBehavior::MapTransfer,
            "Yayang_Area 4002 8995",
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let mut map_rx = enter_player(
        &inbound_tx,
        player_id,
        EntityId(5_111),
        LocalPos::new(94.0, 100.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    set_sim_now(&mut app, 1_000);
    move_player(&inbound_tx, player_id, LocalPos::new(106.0, 100.0), 1_000);
    advance_tick(&mut app);

    assert!(
        portal_entry_destinations(&drain_player_events(&mut map_rx)).is_empty(),
        "accepting a long movement segment should not immediately trigger the portal"
    );

    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    let motion = app
        .world()
        .entity(player_entity)
        .get::<PlayerMotion>()
        .expect("player motion")
        .0;
    set_sim_now(
        &mut app,
        1_000
            + u64::from(
                motion
                    .segment_end_ts
                    .saturating_sub(motion.segment_start_ts)
                    .get()
                    / 2,
            ),
    );
    run_fixed_update(&mut app);
    run_fixed_post_update(&mut app);

    assert_eq!(
        portal_entry_destinations(&drain_player_events(&mut map_rx)),
        vec![PortalDestination::MapTransfer {
            world_pos: zohar_domain::coords::WorldPos::new(4002.0, 8995.0),
        }]
    );
}

#[test]
fn movement_animation_change_replicates_and_affects_player_move_duration() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, map) = test_configs(map_key);
    Arc::make_mut(&mut shared.motion_speeds).upsert_speed(
        crate::motion::MotionEntityKey::Player(PlayerMotionProfileKey {
            class: zohar_domain::entity::player::PlayerClass::Warrior,
            gender: zohar_domain::entity::player::PlayerGender::Male,
        }),
        MotionMoveMode::Run,
        4.5,
    );
    Arc::make_mut(&mut shared.motion_speeds).upsert_speed(
        crate::motion::MotionEntityKey::Player(PlayerMotionProfileKey {
            class: zohar_domain::entity::player::PlayerClass::Warrior,
            gender: zohar_domain::entity::player::PlayerGender::Male,
        }),
        MotionMoveMode::Walk,
        1.5,
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_106);
    let mut player_rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(1.0, 1.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut player_rx);

    set_movement_animation(&inbound_tx, player_id, MovementAnimation::Walk);
    advance_tick(&mut app);

    assert_eq!(
        movement_animation_events(&drain_player_events(&mut player_rx)),
        vec![(player_net_id, MovementAnimation::Walk)]
    );

    clear_pending_movements(&mut app);
    move_player(&inbound_tx, player_id, LocalPos::new(4.0, 1.0), 1_000);
    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == player_net_id)
        .expect("pending player movement");
    let expected_duration = super::util::duration_from_motion_speed(
        1.5,
        100,
        LocalPos::new(1.0, 1.0),
        LocalPos::new(4.0, 1.0),
    );

    assert_eq!(movement.duration, expected_duration);
}

#[test]
fn player_move_clamps_to_map_bounds_in_pre_alpha_policy() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_105);
    let _rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(1.5, 1.5),
    );
    advance_tick(&mut app);

    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    move_player(&inbound_tx, player_id, LocalPos::new(12.5, 9.5), 1_000);

    advance_tick(&mut app);

    let transform = app
        .world()
        .entity(player_entity)
        .get::<LocalTransform>()
        .expect("transform");
    assert!(transform.pos.x > 7.99 && transform.pos.x < 8.0);
    assert!(transform.pos.y > 7.99 && transform.pos.y < 8.0);
}

#[test]
fn noisy_player_move_backlog_does_not_evict_other_players_move_backlog() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let player_one = PlayerId::from(1);
    let player_one_net = EntityId(5_200);
    let player_two = PlayerId::from(2);
    let player_two_net = EntityId(5_201);

    let _ = enter_player(
        &inbound_tx,
        player_one,
        player_one_net,
        LocalPos::new(10.0, 10.0),
    );
    let _ = enter_player(
        &inbound_tx,
        player_two,
        player_two_net,
        LocalPos::new(12.0, 10.0),
    );
    advance_tick(&mut app);

    for idx in 0..40 {
        move_player(
            &inbound_tx,
            player_one,
            LocalPos::new(10.0 + idx as f32, 10.0),
            1_000 + idx,
        );
    }
    run_pre_update(&mut app);
    for idx in 40..(super::state::MAX_MOVE_INTENTS_PER_TICK as u32 + 2) {
        move_player(
            &inbound_tx,
            player_one,
            LocalPos::new(10.0 + idx as f32, 10.0),
            1_000 + idx,
        );
    }
    move_player(&inbound_tx, player_two, LocalPos::new(14.0, 10.0), 2_000);

    run_pre_update(&mut app);

    let player_one_entity = app.world().resource::<PlayerIndex>().0[&player_one];
    let player_two_entity = app.world().resource::<PlayerIndex>().0[&player_two];
    let player_one_queue = app
        .world()
        .entity(player_one_entity)
        .get::<PlayerCommandQueue>()
        .expect("player one command queue");
    let player_two_queue = app
        .world()
        .entity(player_two_entity)
        .get::<PlayerCommandQueue>()
        .expect("player two command queue");

    assert_eq!(
        player_one_queue
            .0
            .iter()
            .filter(|command| matches!(command, super::state::PlayerCommand::Move { .. }))
            .count(),
        super::state::MAX_MOVE_INTENTS_PER_TICK
    );
    assert!(matches!(
        player_two_queue.0.as_slice(),
        [super::state::PlayerCommand::Move { ts, target, .. }]
            if *ts == client_ts(2_000) && *target == LocalPos::new(14.0, 10.0)
    ));
}

#[test]
fn noisy_player_attack_backlog_does_not_evict_other_players_attack_backlog() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.0, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "attack_queue_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_net_id = first_mob_net_id(&mut app);
    let player_one = PlayerId::from(1);
    let player_one_net = EntityId(5_202);
    let player_two = PlayerId::from(2);
    let player_two_net = EntityId(5_203);

    let _ = enter_player(
        &inbound_tx,
        player_one,
        player_one_net,
        LocalPos::new(10.5, 10.0),
    );
    let _ = enter_player(
        &inbound_tx,
        player_two,
        player_two_net,
        LocalPos::new(11.0, 10.0),
    );
    advance_tick(&mut app);

    for _ in 0..(super::state::MAX_ATTACK_INTENTS_PER_TICK + 2) {
        attack_target(&inbound_tx, player_one, mob_net_id, 1);
    }
    attack_target(&inbound_tx, player_two, mob_net_id, 1);

    run_pre_update(&mut app);

    let player_one_entity = app.world().resource::<PlayerIndex>().0[&player_one];
    let player_two_entity = app.world().resource::<PlayerIndex>().0[&player_two];
    let player_one_queue = app
        .world()
        .entity(player_one_entity)
        .get::<PlayerCommandQueue>()
        .expect("player one command queue");
    let player_two_queue = app
        .world()
        .entity(player_two_entity)
        .get::<PlayerCommandQueue>()
        .expect("player two command queue");

    assert_eq!(
        player_one_queue
            .0
            .iter()
            .filter(|command| matches!(command, super::state::PlayerCommand::Attack { .. }))
            .count(),
        super::state::MAX_ATTACK_INTENTS_PER_TICK
    );
    assert!(matches!(
        player_two_queue.0.as_slice(),
        [super::state::PlayerCommand::Attack { target, attack }]
            if *target == mob_net_id && *attack == AttackIntent::Skill(SkillId::ThreeWayCut)
    ));
}

#[test]
fn same_tick_move_then_attack_uses_updated_player_position() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(13.0, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "same_tick_attack_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_208);
    let _ = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(10.0, 10.0),
    );
    advance_tick(&mut app);

    clear_pending_movements(&mut app);
    move_player(&inbound_tx, player_id, LocalPos::new(12.0, 10.0), 1_000);
    attack_target(&inbound_tx, player_id, mob_net_id, 1);
    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let movements = pending_movements(&app);
    assert!(movements.iter().any(|movement| {
        movement.entity_id == player_net_id
            && movement.kind == MovementKind::Move
            && movement.new_pos == LocalPos::new(12.0, 10.0)
    }));
    assert!(
        movements.iter().any(|movement| {
            movement.entity_id == player_net_id && movement.kind == MovementKind::Attack
        }),
        "same-tick attack should be validated from the post-move player position"
    );
}

#[test]
fn player_attack_applies_damage_to_mob_hp_without_stat_replication() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "damage_target_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let mut rx = enter_player(
        &inbound_tx,
        player_id,
        EntityId(5_208),
        LocalPos::new(10.0, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx);

    select_target(&inbound_tx, player_id, mob_net_id);
    advance_tick(&mut app);
    assert_eq!(
        health_bar_events(&drain_player_events(&mut rx)),
        vec![(mob_net_id, 100)]
    );

    let hp_before = mob_hp(&app, mob_entity);
    attack_target(&inbound_tx, player_id, mob_net_id, 0);
    advance_tick(&mut app);
    let hp_after = mob_hp(&app, mob_entity);
    let events = drain_player_events(&mut rx);
    let updates = stat_events(&events);
    let health_bars = health_bar_events(&events);
    let damage_infos = damage_info_events(&events);

    assert!(
        hp_after < hp_before,
        "expected player attack to reduce mob HP, before={hp_before}, after={hp_after}"
    );
    assert!(
        updates
            .iter()
            .all(|(entity_id, stat, _)| { !(*entity_id == mob_net_id && *stat == Stat::Hp) }),
        "mob HP should stay out of generic stat replication, got {events:?}"
    );
    assert!(
        health_bars
            .iter()
            .any(|(entity_id, hp_pct)| *entity_id == mob_net_id && *hp_pct < 100),
        "selected mob HP should replicate through target health packet, got {events:?}"
    );
    assert!(
        damage_infos.iter().any(|(entity_id, flags, damage)| {
            *entity_id == mob_net_id
                && *flags == zohar_map_port::DamageInfoFlags::NORMAL.bits()
                && *damage > 0
        }),
        "selected mob damage should emit damage info packet, got {events:?}"
    );
}

#[test]
fn selected_target_health_updates_require_current_visibility() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "visibility_target_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let mob_net_id = first_mob_net_id(&mut app);
    let alice_id = PlayerId::from(1);
    let bob_id = PlayerId::from(2);
    let bob_net_id = EntityId(5_216);
    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        EntityId(5_215),
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(&inbound_tx, bob_id, bob_net_id, LocalPos::new(10.0, 10.5));
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    select_target(&inbound_tx, bob_id, mob_net_id);
    advance_tick(&mut app);
    assert_eq!(
        health_bar_events(&drain_player_events(&mut bob_rx)),
        vec![(mob_net_id, 100)]
    );

    {
        let map_entity = map_entity(&app);
        let mut map_entity = app.world_mut().entity_mut(map_entity);
        let mut replication = map_entity
            .get_mut::<MapReplication>()
            .expect("map replication");
        assert!(replication.0.remove_visibility(bob_net_id, mob_net_id));
    }

    attack_target(&inbound_tx, alice_id, mob_net_id, 0);
    advance_tick(&mut app);

    let bob_events = drain_player_events(&mut bob_rx);
    assert!(
        health_bar_events(&bob_events)
            .into_iter()
            .all(|(entity_id, _)| entity_id != mob_net_id),
        "stale selected targets outside visibility must not receive mob HP updates, got {bob_events:?}"
    );
}

#[test]
fn player_killing_mob_emits_dead_packet_to_visible_players() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "dead_packet_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let mut rx = enter_player(
        &inbound_tx,
        player_id,
        EntityId(5_210),
        LocalPos::new(10.0, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx);

    let mut events = Vec::new();
    for _ in 0..64 {
        attack_target(&inbound_tx, player_id, mob_net_id, 0);
        advance_tick(&mut app);
        events.extend(drain_player_events(&mut rx));
        if stunned_events(&events).contains(&mob_net_id) {
            break;
        }
    }

    assert!(
        stunned_events(&events).contains(&mob_net_id),
        "lethal mob damage should first emit legacy stun packet, got {events:?}"
    );
    assert!(
        !dead_events(&events).contains(&mob_net_id),
        "mob should not emit dead packet before the dying delay, got {events:?}"
    );

    let damage_info_count = damage_info_events(&events).len();
    attack_target(&inbound_tx, player_id, mob_net_id, 0);
    advance_tick(&mut app);
    events.extend(drain_player_events(&mut rx));
    assert_eq!(
        damage_info_events(&events).len(),
        damage_info_count,
        "dying mob should reject further damage info, got {events:?}"
    );

    set_sim_now(&mut app, 10_000);
    run_actor_lifecycle(&mut app);
    events.extend(drain_player_events(&mut rx));

    assert!(
        dead_events(&events).contains(&mob_net_id),
        "mob death should emit legacy dead packet after dying delay, got {events:?}"
    );
    let dead_snapshot = super::spawn_events::make_entity_snapshot(
        app.world(),
        app.world().resource::<SharedConfig>(),
        mob_net_id,
    )
    .expect("dead mob snapshot");
    assert!(
        dead_snapshot
            .public_state
            .flags
            .state_flags
            .contains(EntityStateFlags::DEAD),
        "snapshots for already-dead actors must carry the legacy DEAD state flag"
    );

    attack_target(&inbound_tx, player_id, mob_net_id, 0);
    advance_tick(&mut app);
    events.extend(drain_player_events(&mut rx));
    assert_eq!(
        damage_info_events(&events).len(),
        damage_info_count,
        "dead mob should reject ghost damage info, got {events:?}"
    );

    set_sim_now(&mut app, 30_000);
    run_actor_lifecycle(&mut app);
    events.extend(drain_player_events(&mut rx));
    assert!(
        despawn_events(&events).contains(&mob_net_id),
        "dead mob should be eventually destroyed for observers, got {events:?}"
    );

    {
        let map_entity = map_entity(&app);
        let spawn_rules = app
            .world()
            .entity(map_entity)
            .get::<MapSpawnRules>()
            .expect("map spawn rules");
        let rule_state = &spawn_rules.rules[0];
        assert_eq!(rule_state.active_instances, 0);
        assert!(rule_state.entities.is_empty());
        assert_eq!(rule_state.respawn_at, Some(sim_ms(90_000)));
        assert_eq!(
            spawn_rules
                .scheduled_spawns
                .peek()
                .map(|scheduled| scheduled.0),
            Some((sim_ms(90_000), 0))
        );
    }

    set_sim_now(&mut app, 90_000);
    super::spawn::spawn_rules(app.world_mut());
    {
        let map_entity = map_entity(&app);
        let spawn_rules = app
            .world()
            .entity(map_entity)
            .get::<MapSpawnRules>()
            .expect("map spawn rules");
        let rule_state = &spawn_rules.rules[0];
        assert_eq!(rule_state.active_instances, 1);
        assert_eq!(rule_state.respawn_at, None);
        assert_eq!(rule_state.entities.len(), 1);
    }
}

#[test]
fn mob_death_reward_applies_exp_steps_level_up_refill_and_visuals_once() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    shared.player_stats = Arc::new(test_player_stat_rules_with_level_exp((1..=120).map(
        |level| crate::LevelExpEntry {
            level,
            next_exp: i64::from(level) * 10,
            death_loss_pct: 0,
        },
    )));
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 2,
        regen_time: Duration::from_secs(60),
    }));

    let proto = test_mob_proto_with_combat_and_rewards(
        mob_id,
        MobKind::Monster,
        "reward_wolf",
        MobRank::Pawn,
        MobBattleType::Melee,
        1,
        100,
        100,
        0,
        150,
        BehaviorFlags::empty(),
        MobRewards { experience: 1_000 },
    );
    Arc::make_mut(&mut shared.mobs).insert(mob_id, proto);

    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);
    let mobs = mob_net_ids(&mut app);
    assert_eq!(mobs.len(), 2);

    let alice_id = PlayerId::from(1);
    let alice_net_id = EntityId(5_220);
    let bob_id = PlayerId::from(2);
    let bob_net_id = EntityId(5_221);
    let mut alice_rx = enter_player(
        &inbound_tx,
        alice_id,
        alice_net_id,
        LocalPos::new(10.0, 10.0),
    );
    let mut bob_rx = enter_player(&inbound_tx, bob_id, bob_net_id, LocalPos::new(10.0, 10.5));
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    set_player_progression_for_test(&mut app, alice_id, 1, 7, 10);
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    let mut alice_events = kill_mob_and_finalize_death(
        &mut app,
        &inbound_tx,
        &mut alice_rx,
        alice_id,
        mobs[0],
        10_000,
    );
    let bob_events = drain_player_events(&mut bob_rx);
    let alice_stats = stat_events(&alice_events);
    assert!(
        alice_stats.contains(&(alice_net_id, Stat::Exp, 8)),
        "expected exp stat update, got {alice_events:?}"
    );
    assert!(
        alice_stats.contains(&(alice_net_id, Stat::LevelStep, 3)),
        "expected level-step stat update, got {alice_events:?}"
    );
    assert!(
        alice_stats.contains(&(alice_net_id, Stat::StatPoints, 1)),
        "expected stat-point update, got {alice_events:?}"
    );
    assert!(
        player_hp(&app, alice_id) > 1,
        "level-step reward should refill HP"
    );
    assert!(
        stat_events(&bob_events).contains(&(alice_net_id, Stat::LevelStep, 0)),
        "observer should receive the explicit level-step point visual"
    );
    assert!(
        projectile_effect_events(&alice_events)
            .iter()
            .any(|event| *event == (ProjectileEffectKind::Exp, mobs[0], alice_net_id))
    );

    set_player_progression_for_test(&mut app, alice_id, 1, 9, 10);
    advance_tick(&mut app);
    let _ = drain_player_events(&mut alice_rx);
    let _ = drain_player_events(&mut bob_rx);

    alice_events = kill_mob_and_finalize_death(
        &mut app,
        &inbound_tx,
        &mut alice_rx,
        alice_id,
        mobs[1],
        20_000,
    );
    let bob_events = drain_player_events(&mut bob_rx);
    let alice_stats = stat_events(&alice_events);
    assert!(alice_stats.contains(&(alice_net_id, Stat::Level, 2)));
    assert!(alice_stats.contains(&(alice_net_id, Stat::Exp, 0)));
    assert!(
        stat_events(&bob_events).contains(&(alice_net_id, Stat::Level, 2)),
        "observer should receive the explicit level-up point visual"
    );

    set_sim_now(&mut app, 21_000);
    run_actor_lifecycle(&mut app);
    let repeated = drain_player_events(&mut alice_rx);
    assert!(
        projectile_effect_events(&repeated)
            .into_iter()
            .all(|(_, start, _)| start != mobs[1]),
        "finalized death should not grant the same mob reward twice"
    );
}

#[test]
fn mob_attack_applies_damage_to_player_hp() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "damage_source_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let mob_entity = first_mob_entity(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_209);
    let mut rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(10.0, 10.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx);

    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(10.5, 10.0),
        east_rot(),
        1_000,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, 1_000, 0);

    let hp_before = player_hp(&app, player_id);
    run_mob_ai(&mut app);
    let hp_after = player_hp(&app, player_id);
    run_fixed_post_update(&mut app);
    let events = drain_player_events(&mut rx);

    assert!(
        hp_after < hp_before,
        "expected mob attack to reduce player HP, before={hp_before}, after={hp_after}"
    );
    assert!(
        damage_info_events(&events)
            .iter()
            .any(|(entity_id, flags, damage)| {
                *entity_id == player_net_id
                    && *flags == zohar_map_port::DamageInfoFlags::NORMAL.bits()
                    && *damage > 0
            }),
        "player damage should emit damage info packet, got {events:?}"
    );
}

#[test]
fn mob_drops_player_target_after_dead_phase() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.5, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "corpse_target_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_211);
    let mut gameplay = default_gameplay_bootstrap(player_id, PlayerClass::Warrior, 1);
    gameplay.current_hp = Some(1);
    let mut rx = enter_player_with_gameplay(
        &inbound_tx,
        gameplay,
        player_net_id,
        LocalPos::new(10.0, 10.0),
        PlayerAppearance::default(),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx);

    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(10.5, 10.0),
        east_rot(),
        1_000,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, 1_000, 0);

    run_mob_ai(&mut app);
    run_fixed_post_update(&mut app);
    let events = drain_player_events(&mut rx);
    assert!(
        stunned_events(&events).contains(&player_net_id),
        "lethal mob attack should put the player into dying/stun, got {events:?}"
    );
    assert!(
        attack_movement_count(&events, mob_net_id) > 0,
        "initial lethal hit should still replicate the mob attack, got {events:?}"
    );

    set_sim_now(&mut app, 10_000);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);
    assert!(
        dead_events(&events).contains(&player_net_id),
        "dying player should transition to dead, got {events:?}"
    );

    set_mob_chasing(&mut app, mob_entity, player_net_id, 10_100, 0);
    run_mob_ai(&mut app);
    run_fixed_post_update(&mut app);
    let events = drain_player_events(&mut rx);
    assert_eq!(
        attack_movement_count(&events, mob_net_id),
        0,
        "mob should not keep attacking a fully dead player, got {events:?}"
    );
    assert!(
        damage_info_events(&events).is_empty(),
        "corpse attacks should not produce damage info, got {events:?}"
    );
}

#[test]
fn restart_here_revives_dead_player_after_legacy_delay() {
    let player_net_id = EntityId(5_212);
    let (mut app, inbound_tx, mut rx, player_id, _) =
        dead_player_runtime("restart_here_wolf", player_net_id);
    let mob_net_id = first_mob_net_id(&mut app);
    let death_pos = player_pos(&app, player_id);

    set_sim_now(&mut app, 20_000);
    restart_player(&inbound_tx, player_id, PlayerRestartIntent::Here);
    run_pre_update(&mut app);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);

    assert_eq!(player_hp(&app, player_id), 50);
    assert_eq!(player_pos(&app, player_id), death_pos);
    assert!(command_messages(&events).contains(&"CloseRestartWindow".to_string()));

    // The client only processes DEAD→alive at spawn time, so the player entity
    // must be despawned+respawned for both self and observers.
    assert!(despawn_events(&events).contains(&player_net_id));
    assert!(spawn_event_ids(&events).contains(&player_net_id));

    // Main-character spawn purges dynamic actors client-side, so nearby entities
    // must be spawned again for the restarting client. A separate despawn is not
    // needed because the purge already removed them locally.
    assert!(!despawn_events(&events).contains(&mob_net_id));
    assert!(spawn_event_ids(&events).contains(&mob_net_id));

    let snapshot = super::spawn_events::make_entity_snapshot(
        app.world(),
        app.world().resource::<SharedConfig>(),
        player_net_id,
    )
    .expect("revived player snapshot");
    assert!(
        !snapshot
            .public_state
            .flags
            .state_flags
            .contains(EntityStateFlags::DEAD),
        "revived player snapshot must clear the legacy DEAD state flag"
    );
}

#[test]
fn restart_here_before_legacy_delay_sends_wait_feedback() {
    let player_net_id = EntityId(5_215);
    let (mut app, inbound_tx, mut rx, player_id, _) =
        dead_player_runtime("early_restart_here_wolf", player_net_id);
    let death_pos = player_pos(&app, player_id);

    set_sim_now(&mut app, 12_000);
    restart_player(&inbound_tx, player_id, PlayerRestartIntent::Here);
    run_pre_update(&mut app);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);

    assert_eq!(player_pos(&app, player_id), death_pos);
    assert_eq!(
        info_messages(&events),
        vec!["A new start is not possible at the moment. Please wait 8 seconds.".to_string()]
    );
    assert!(!command_messages(&events).contains(&"CloseRestartWindow".to_string()));
    assert!(!despawn_events(&events).contains(&player_net_id));
    assert!(!spawn_event_ids(&events).contains(&player_net_id));
}

#[test]
fn restart_resets_passive_recovery_clock_instead_of_catching_up_dead_time() {
    let player_net_id = EntityId(5_217);
    let (mut app, inbound_tx, mut rx, player_id, _) =
        dead_player_runtime("restart_recovery_wolf", player_net_id);

    set_sim_now(&mut app, 20_000);
    restart_player(&inbound_tx, player_id, PlayerRestartIntent::Here);
    run_pre_update(&mut app);
    run_actor_lifecycle(&mut app);
    let _ = drain_player_events(&mut rx);
    assert_eq!(player_hp(&app, player_id), 50);

    set_sim_now(&mut app, 20_040);
    super::player::stat_tickers::process_player_stat_tickers(app.world_mut());

    assert_eq!(
        player_hp(&app, player_id),
        50,
        "passive recovery must not apply the elapsed time spent dead immediately after restart"
    );
}

#[test]
fn restart_town_revives_dead_player_at_empire_town_after_legacy_delay() {
    let player_net_id = EntityId(5_213);
    let (mut app, inbound_tx, mut rx, player_id, _) =
        dead_player_runtime("restart_town_wolf", player_net_id);
    let death_pos = player_pos(&app, player_id);

    set_sim_now(&mut app, 17_000);
    restart_player(&inbound_tx, player_id, PlayerRestartIntent::Town);
    run_pre_update(&mut app);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);

    assert_eq!(player_hp(&app, player_id), 50);
    assert_eq!(player_pos(&app, player_id), death_pos);
    assert!(command_messages(&events).contains(&"CloseRestartWindow".to_string()));
    assert_eq!(restart_town_event_count(&events), 1);
    assert!(!despawn_events(&events).contains(&player_net_id));
    assert!(!spawn_event_ids(&events).contains(&player_net_id));
}

#[test]
fn restart_town_before_legacy_delay_sends_wait_feedback() {
    let player_net_id = EntityId(5_216);
    let (mut app, inbound_tx, mut rx, player_id, _) =
        dead_player_runtime("early_restart_town_wolf", player_net_id);
    let death_pos = player_pos(&app, player_id);

    set_sim_now(&mut app, 12_000);
    restart_player(&inbound_tx, player_id, PlayerRestartIntent::Town);
    run_pre_update(&mut app);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);

    assert_eq!(player_pos(&app, player_id), death_pos);
    assert_eq!(
        info_messages(&events),
        vec!["You cannot restart in the city yet. Wait another 5 seconds.".to_string()]
    );
    assert!(!command_messages(&events).contains(&"CloseRestartWindow".to_string()));
    assert_eq!(restart_town_event_count(&events), 0);
}

#[test]
fn dead_event_forces_town_restart_with_half_hp() {
    let player_net_id = EntityId(5_214);
    let (mut app, _, mut rx, player_id, _) =
        dead_player_runtime("forced_restart_wolf", player_net_id);
    let expected_hp = player_max_hp(&app, player_id) / 2;
    let death_pos = player_pos(&app, player_id);

    set_sim_now(&mut app, 190_000);
    run_actor_lifecycle(&mut app);
    let events = drain_player_events(&mut rx);

    assert_eq!(player_hp(&app, player_id), expected_hp);
    assert_eq!(player_pos(&app, player_id), death_pos);
    assert!(command_messages(&events).contains(&"CloseRestartWindow".to_string()));
    assert_eq!(restart_town_event_count(&events), 1);
    assert!(!despawn_events(&events).contains(&player_net_id));
    assert!(!spawn_event_ids(&events).contains(&player_net_id));
}

#[test]
fn same_tick_player_move_is_visible_to_mob_ai() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.0, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "same_tick_aggro_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            250,
            150,
            BehaviorFlags::AGGRESSIVE,
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_209);
    let _ = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(13.1, 10.0),
    );
    advance_tick(&mut app);

    move_player(&inbound_tx, player_id, LocalPos::new(12.0, 10.0), 1_000);
    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let brain = app
        .world()
        .entity(mob_entity)
        .get::<MobBrainState>()
        .copied()
        .expect("mob brain");
    assert_eq!(
        brain.target(),
        Some(player_net_id),
        "mob AI should acquire a player that moved into aggro range during this tick"
    );
}

#[test]
fn player_attack_emits_stimulus_dispatch_without_mutating_mob_queue() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(10.0, 10.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "stimulus_dispatch_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_204);
    let _ = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(10.5, 10.0),
    );
    advance_tick(&mut app);

    attack_target(&inbound_tx, player_id, mob_net_id, 1);
    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    super::player_actions::process_player_actions(app.world_mut());

    let dispatches = &app.world().resource::<MobAggroDispatchBuffer>().0;
    assert!(dispatches.iter().any(|dispatch| {
        dispatch.attacked_mob_entity == mob_entity
            && matches!(
                dispatch.aggro,
                MobAggro::ProvokedBy { attacker } if attacker == player_net_id
            )
    }));
    assert!(
        app.world()
            .entity(mob_entity)
            .get::<MobAggroQueue>()
            .expect("mob aggro queue")
            .0
            .is_empty(),
        "player intake should emit dispatches, not mutate mob queues directly"
    );
}

#[test]
fn stimulus_routing_fans_out_pack_before_mob_think_order_matters() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Group(Arc::from([mob_id, mob_id, mob_id])),
        area: SpawnArea::new(LocalPos::new(6_400.0, 6_400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "stimulus_pack_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let (mob_net_ids, mob_entities) = {
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &MobMarker, &NetEntityId)>();
        let rows = q
            .iter(world)
            .map(|(entity, _, net_id)| (entity, net_id.net_id))
            .collect::<Vec<_>>();
        (
            rows.iter().map(|(_, net_id)| *net_id).collect::<Vec<_>>(),
            rows.iter().map(|(entity, _)| *entity).collect::<Vec<_>>(),
        )
    };

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_204);
    let _ = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(6_401.0, 6_400.0),
    );
    advance_tick(&mut app);

    attack_target(&inbound_tx, player_id, mob_net_ids[0], 1);
    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    super::player_actions::process_player_actions(app.world_mut());
    super::aggro::route_mob_aggro(app.world_mut());

    for mob_entity in mob_entities {
        let queue = app
            .world()
            .entity(mob_entity)
            .get::<MobAggroQueue>()
            .expect("mob aggro queue");
        assert!(queue.0.iter().any(|aggro| {
            matches!(
                aggro,
                MobAggro::ProvokedBy { attacker } if *attacker == player_net_id
            )
        }));
    }
}

#[test]
fn fixed_update_orders_player_intake_before_stimulus_routing_before_mob_ai() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Group(Arc::from([mob_id, mob_id])),
        area: SpawnArea::new(LocalPos::new(6_500.0, 6_500.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "scheduler_order_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_206);
    let _ = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(6_501.0, 6_500.0),
    );
    advance_tick(&mut app);

    attack_target(&inbound_tx, player_id, mob_net_id, 1);
    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let targeted = {
        let world = app.world_mut();
        let mut query = world.query::<(&MobMarker, &MobBrainState)>();
        query
            .iter(world)
            .filter(|(_, brain)| brain.target() == Some(player_net_id))
            .count()
    };
    assert_eq!(
        targeted, 2,
        "full fixed-update should drain player intake, route pack stimuli, and let mob think consume them in the same tick"
    );
}

#[test]
fn fixed_timestep_switches_to_idle_rate_without_players() {
    let map_id = MapId::new(41);
    let (shared, map) = test_configs(MapInstanceKey::shared(1, map_id));
    let (app, _inbound_tx) = build_runtime_app(shared, map, false);

    assert_eq!(
        app.world()
            .resource::<bevy::prelude::Time<bevy::prelude::Fixed>>()
            .timestep(),
        Duration::from_secs(1)
    );
}

#[test]
fn fixed_timestep_switches_back_to_active_rate_on_player_enter() {
    let map_id = MapId::new(41);
    let (shared, map) = test_configs(MapInstanceKey::shared(1, map_id));
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    assert_eq!(
        app.world()
            .resource::<bevy::prelude::Time<bevy::prelude::Fixed>>()
            .timestep(),
        Duration::from_secs(1)
    );

    let _rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        EntityId(5_601),
        LocalPos::new(6_400.0, 6_400.0),
    );
    advance_tick(&mut app);

    assert_eq!(
        app.world()
            .resource::<bevy::prelude::Time<bevy::prelude::Fixed>>()
            .timestep(),
        Duration::from_millis(40)
    );
}

#[test]
fn player_attack_intent_causes_non_aggressive_mob_to_retaliate() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6_400.0, 6_400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "calm_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let mob_net_id = first_mob_net_id(&mut app);
    let mob_entity = first_mob_entity(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_602);
    let mut map_rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(6_401.0, 6_400.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    attack_target(&inbound_tx, player_id, mob_net_id, 7);
    app.world_mut()
        .resource_mut::<RuntimeState>()
        .packet_time_start = Instant::now() - Duration::from_secs(1);

    let mut events = Vec::new();
    for _ in 0..10 {
        advance_tick(&mut app);
        events.extend(drain_player_events(&mut map_rx));
    }

    assert!(
        movement_events(&events)
            .into_iter()
            .any(|(entity_id, kind)| entity_id == mob_net_id && kind == MovementKind::Attack),
        "retaliating mob should emit attack movement"
    );

    let brain = app
        .world()
        .entity(mob_entity)
        .get::<MobBrainState>()
        .copied()
        .expect("mob brain");
    assert_eq!(brain.target(), Some(player_net_id));
    assert!(matches!(
        brain.mode(),
        MobBrainMode::AttackWindup | MobBrainMode::Pursuit
    ));
}

#[test]
fn mob_chase_ignores_navigation_blockers_with_legacy_straight_segments() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.navigator = Some(test_navigator(8, 8, &[(2, 1), (3, 1)]));
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "legacy_wait_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let navigator = map.navigator.clone().expect("navigator");
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_604);
    let _map_rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(8.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 10_000);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("legacy chase packet");
    let follow_distance = super::mob_ai::mob_follow_distance_m(1.5);
    assert_eq!(movement.kind, MovementKind::Wait);
    assert_eq!(movement.rot, east_rot());
    assert_eq!(movement.new_pos, LocalPos::new(8.0 - follow_distance, 1.0));
    assert!(
        !navigator.segment_clear(LocalPos::new(1.0, 1.0), movement.new_pos),
        "legacy straight-line chase should ignore terrain blockers on the chase hot path"
    );
    assert!(
        (movement.new_pos.y - 1.0).abs() <= 0.01,
        "legacy straight-line chase should stay on the blocked straight line"
    );
}

#[test]
fn idle_wander_retries_blocked_segments_instead_of_walking_through_walls() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(12.0, 12.0);
    map.navigator = Some(test_navigator(
        12,
        12,
        &[
            (5, 2),
            (5, 3),
            (5, 4),
            (5, 5),
            (5, 6),
            (5, 7),
            (5, 8),
            (5, 9),
        ],
    ));
    shared.wander = WanderConfig {
        decision_pause_idle_min: Duration::ZERO,
        decision_pause_idle_max: Duration::ZERO,
        post_move_pause_min: Duration::ZERO,
        post_move_pause_max: Duration::ZERO,
        wander_chance_denominator: 1,
        step_min_m: 4.0,
        step_max_m: 4.0,
    };
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(4.0, 6.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto(
            mob_id,
            MobKind::Monster,
            "stable_wander_npc",
            MobRank::Pawn,
            1,
            100,
            100,
            BehaviorFlags::empty(),
        ),
    );
    let navigator = map.navigator.clone().expect("navigator");
    let current_pos = LocalPos::new(4.0, 6.0);
    let (seed, blocked_candidate) = find_seed_with_blocked_first_wander_candidate(
        map.local_size,
        navigator.as_ref(),
        current_pos,
        4.0,
    );
    let (mut app, _inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    app.world_mut().resource_mut::<RuntimeState>().rng = SmallRng::seed_from_u64(seed);
    clear_pending_movements(&mut app);
    set_stationary_mob(&mut app, mob_entity, current_pos, east_rot(), now_ms);
    set_mob_idle(&mut app, mob_entity, now_ms);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("idle wander packet");
    assert_eq!(movement.kind, MovementKind::Wait);
    assert!(
        !navigator.segment_clear(current_pos, blocked_candidate),
        "test seed must begin with a blocked wander sample"
    );
    assert!(
        navigator.segment_clear(current_pos, movement.new_pos),
        "idle wander should only emit a clear straight segment"
    );
    assert_ne!(
        movement.new_pos, blocked_candidate,
        "blocked first wander sample must be retried instead of emitted"
    );
}

#[test]
fn idle_wander_does_not_issue_a_second_move_while_first_move_is_in_flight() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(100.0, 100.0);
    shared.wander = WanderConfig {
        decision_pause_idle_min: Duration::ZERO,
        decision_pause_idle_max: Duration::ZERO,
        post_move_pause_min: Duration::from_millis(400),
        post_move_pause_max: Duration::from_millis(400),
        wander_chance_denominator: 1,
        step_min_m: 4.0,
        step_max_m: 4.0,
    };
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(50.0, 50.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto(
            mob_id,
            MobKind::Monster,
            "wander_in_flight_guard",
            MobRank::Pawn,
            1,
            100,
            100,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, _inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    app.world_mut().resource_mut::<RuntimeState>().rng = SmallRng::seed_from_u64(7);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(50.0, 50.0),
        east_rot(),
        now_ms,
    );
    set_mob_idle(&mut app, mob_entity, now_ms);

    run_mob_ai(&mut app);

    let first_move = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("first idle wander packet");

    clear_pending_movements(&mut app);
    set_sim_now(&mut app, now_ms + u64::from(first_move.duration / 2));

    run_mob_ai(&mut app);

    assert!(
        pending_movements(&app)
            .into_iter()
            .all(|movement| movement.entity_id != mob_net_id),
        "mob should not emit a second idle wander packet before the first movement ends"
    );
}

#[test]
fn idle_wander_post_move_pause_delays_the_next_wander_decision() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(100.0, 100.0);
    shared.wander = WanderConfig {
        decision_pause_idle_min: Duration::ZERO,
        decision_pause_idle_max: Duration::ZERO,
        post_move_pause_min: Duration::from_millis(400),
        post_move_pause_max: Duration::from_millis(400),
        wander_chance_denominator: 1,
        step_min_m: 4.0,
        step_max_m: 4.0,
    };
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(50.0, 50.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto(
            mob_id,
            MobKind::Monster,
            "wander_pause_guard",
            MobRank::Pawn,
            1,
            100,
            100,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, _inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    app.world_mut().resource_mut::<RuntimeState>().rng = SmallRng::seed_from_u64(9);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(50.0, 50.0),
        east_rot(),
        now_ms,
    );
    set_mob_idle(&mut app, mob_entity, now_ms);

    run_mob_ai(&mut app);

    let first_move = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("first idle wander packet");
    let movement_end_ms = now_ms + u64::from(first_move.duration);

    clear_pending_movements(&mut app);
    set_sim_now(&mut app, movement_end_ms);
    run_mob_ai(&mut app);
    assert!(
        pending_movements(&app)
            .into_iter()
            .all(|movement| movement.entity_id != mob_net_id),
        "post-move pause should suppress the next idle wander exactly at movement end"
    );

    clear_pending_movements(&mut app);
    set_sim_now(&mut app, movement_end_ms + 400);
    run_mob_ai(&mut app);
    assert!(
        pending_movements(&app)
            .into_iter()
            .any(|movement| movement.entity_id == mob_net_id),
        "mob should wander again once the configured post-move pause has elapsed"
    );
}

#[test]
fn mob_chase_wait_targets_full_follow_distance_and_duration_matches_motion_speed() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(24.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "follow_goal_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_net_id = EntityId(5_615);
    let _map_rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        player_net_id,
        LocalPos::new(20.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 10_000);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("legacy chase packet");
    let follow_distance = super::mob_ai::mob_follow_distance_m(1.5);
    let expected_end = LocalPos::new(20.0 - follow_distance, 1.0);
    let expected_duration = super::util::duration_from_motion_speed(
        super::state::DEFAULT_RUN_MOTION_SPEED_METER_PER_SEC,
        100,
        LocalPos::new(1.0, 1.0),
        expected_end,
    );
    assert_eq!(movement.kind, MovementKind::Wait);
    assert_eq!(movement.new_pos, expected_end);
    assert_eq!(movement.rot, east_rot());
    assert_eq!(movement.duration, expected_duration);
}

#[test]
fn mob_close_chase_stops_at_follow_distance() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(16.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "follow_distance_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_net_id = EntityId(5_616);
    let target_pos = LocalPos::new(4.0, 1.0);
    let _map_rx = enter_player(&inbound_tx, PlayerId::from(1), player_net_id, target_pos);
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 10_000);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("legacy chase packet");
    let follow_distance = super::mob_ai::mob_follow_distance_m(2.5);
    assert_eq!(movement.kind, MovementKind::Wait);
    assert!((target_pos.x - movement.new_pos.x - follow_distance).abs() <= 0.01);
    assert!((target_pos.y - movement.new_pos.y).abs() <= 0.01);
}

#[test]
fn mob_attacks_from_current_position_within_threshold() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "attack_from_current_pos_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_net_id = EntityId(5_617);
    let _map_rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        player_net_id,
        LocalPos::new(2.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        north_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 0);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("attack packet");
    assert_eq!(movement.kind, MovementKind::Attack);
    assert_eq!(movement.new_pos, LocalPos::new(1.0, 1.0));
    assert_eq!(movement.rot, east_rot());
    assert_eq!(movement.duration, 600);
    assert_eq!(
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.mode()),
        Some(MobBrainMode::AttackWindup)
    );
}

#[test]
fn mob_close_wait_chase_attacks_after_settling() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "close_wait_before_attack_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_net_id = EntityId(5_618);
    let target_pos = LocalPos::new(4.5, 1.0);
    let _map_rx = enter_player(&inbound_tx, PlayerId::from(1), player_net_id, target_pos);
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 0);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("close chase packet");
    assert_eq!(movement.kind, MovementKind::Wait);
    assert!(movement.new_pos.x > 1.0);

    clear_pending_movements(&mut app);
    set_sim_now(&mut app, now_ms.saturating_add(2_000));
    super::mob_motion::sample_mob_motion(app.world_mut());
    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("attack after close wait");
    assert_eq!(movement.kind, MovementKind::Attack);
    assert!(movement.new_pos.x > 1.0);
    assert_eq!(movement.rot, east_rot());
}

#[test]
fn melee_attack_windup_suppresses_follow_up_packets() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "windup_lock_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let player_net_id = EntityId(5_618);
    let _map_rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        player_net_id,
        LocalPos::new(2.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 0);

    run_mob_ai(&mut app);
    clear_pending_movements(&mut app);

    set_sim_now(&mut app, now_ms.saturating_add(100));
    run_mob_ai(&mut app);

    assert!(pending_movements(&app).is_empty());
    assert_eq!(
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.mode()),
        Some(MobBrainMode::AttackWindup)
    );
}

#[test]
fn aggro_received_during_attack_windup_is_processed_after_windup() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "windup_aggro_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let first_target_id = PlayerId::from(1);
    let first_target_net_id = EntityId(5_620);
    let _first_rx = enter_player(
        &inbound_tx,
        first_target_id,
        first_target_net_id,
        LocalPos::new(2.0, 1.0),
    );
    let retarget_player_id = PlayerId::from(2);
    let retarget_player_net_id = EntityId(5_621);
    let _second_rx = enter_player(
        &inbound_tx,
        retarget_player_id,
        retarget_player_net_id,
        LocalPos::new(4.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, first_target_net_id, now_ms, 0);
    run_mob_ai(&mut app);

    let windup_until_ms = app
        .world()
        .entity(mob_entity)
        .get::<MobBrainState>()
        .map(|brain| brain.attack_windup_until())
        .expect("brain windup");

    app.world_mut()
        .entity_mut(mob_entity)
        .get_mut::<MobAggroQueue>()
        .expect("mob aggro queue")
        .0
        .push(MobAggro::ProvokedBy {
            attacker: retarget_player_net_id,
        });

    clear_pending_movements(&mut app);
    set_sim_now(&mut app, now_ms.saturating_add(100));
    run_mob_ai(&mut app);

    let queued_aggro = app
        .world()
        .entity(mob_entity)
        .get::<MobAggroQueue>()
        .expect("mob aggro queue")
        .0
        .clone();
    assert_eq!(queued_aggro.len(), 1);
    assert!(matches!(
        queued_aggro[0],
        MobAggro::ProvokedBy { attacker } if attacker == retarget_player_net_id
    ));

    clear_pending_movements(&mut app);
    set_sim_now(
        &mut app,
        windup_until_ms.saturating_add(super::state::SimDuration::from_millis(1)),
    );
    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.kind == MovementKind::Wait)
        .expect("retargeted chase packet");
    assert!(movement.new_pos.x > 1.0);
    assert_eq!(
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.target()),
        Some(Some(retarget_player_net_id))
    );
}

#[test]
fn mob_resumes_wait_chase_after_attack_windup_expires() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "resume_chase_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_619);
    let _map_rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(2.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(1.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 0);
    run_mob_ai(&mut app);

    let windup_until_ms = app
        .world()
        .entity(mob_entity)
        .get::<MobBrainState>()
        .map(|brain| brain.attack_windup_until())
        .expect("brain windup");
    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    {
        let mut player = app.world_mut().entity_mut(player_entity);
        player
            .get_mut::<LocalTransform>()
            .expect("player transform")
            .pos = LocalPos::new(12.0, 1.0);
        player.get_mut::<PlayerMotion>().expect("player motion").0 = PlayerMotionState {
            segment_start_pos: LocalPos::new(12.0, 1.0),
            segment_end_pos: LocalPos::new(12.0, 1.0),
            segment_start_ts: windup_until_ms.to_client_timestamp(),
            segment_end_ts: windup_until_ms.to_client_timestamp(),
            last_client_ts: windup_until_ms.to_client_timestamp(),
        };
    }

    clear_pending_movements(&mut app);
    set_sim_now(
        &mut app,
        windup_until_ms.saturating_add(super::state::SimDuration::from_millis(1)),
    );
    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.kind == MovementKind::Wait)
        .expect("resumed chase packet");
    assert_eq!(movement.kind, MovementKind::Wait);
    assert!(movement.new_pos.x > 1.0);
    assert_eq!(
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.mode()),
        Some(MobBrainMode::Pursuit)
    );
}

#[test]
fn mid_walk_chase_reissues_wait_from_sampled_current_position_to_full_goal() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(24.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "mid_walk_rethink_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let player_net_id = EntityId(5_620);
    let _map_rx = enter_player(
        &inbound_tx,
        PlayerId::from(1),
        player_net_id,
        LocalPos::new(8.0, 1.0),
    );
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    {
        let mut entity = app.world_mut().entity_mut(mob_entity);
        entity.get_mut::<LocalTransform>().expect("transform").pos = LocalPos::new(1.0, 1.0);
        entity.get_mut::<LocalTransform>().expect("transform").rot = east_rot();
        entity.get_mut::<MobMotion>().expect("mob motion").0 = MobMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(5.0, 1.0),
            segment_start_at: sim_ms(800),
            segment_end_at: sim_ms(1_600),
        };
    }
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 10_000);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.kind == MovementKind::Wait)
        .expect("mid-walk rethink packet");
    let motion = app
        .world()
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| motion.0)
        .expect("mob motion");
    assert!((motion.segment_start_pos.x - 2.0).abs() <= 0.01);
    let follow_distance = super::mob_ai::mob_follow_distance_m(1.5);
    assert!((movement.new_pos.x - (8.0 - follow_distance)).abs() <= 0.01);
    assert!(
        (app.world()
            .entity(mob_entity)
            .get::<LocalTransform>()
            .expect("transform")
            .pos
            .x
            - 2.0)
            .abs()
            <= 0.01
    );
}

#[test]
fn mid_walk_chase_interrupts_into_attack_once_sampled_position_is_in_range() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(12.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "interrupt_close_attack_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let player_net_id = EntityId(5_621);
    let target_pos = LocalPos::new(3.5, 1.0);
    let _map_rx = enter_player(&inbound_tx, PlayerId::from(1), player_net_id, target_pos);
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    {
        let mut entity = app.world_mut().entity_mut(mob_entity);
        entity.get_mut::<LocalTransform>().expect("transform").pos = LocalPos::new(1.0, 1.0);
        entity.get_mut::<LocalTransform>().expect("transform").rot = east_rot();
        entity.get_mut::<MobMotion>().expect("mob motion").0 = MobMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(5.0, 1.0),
            segment_start_at: sim_ms(800),
            segment_end_at: sim_ms(1_600),
        };
    }
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 0);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("attack packet");
    assert_eq!(movement.kind, MovementKind::Attack);
    assert!((movement.new_pos.x - 2.0).abs() <= 0.01);
    assert_eq!(movement.rot, east_rot());

    let motion = app
        .world()
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| motion.0)
        .expect("mob motion");
    assert!((motion.segment_start_pos.x - 2.0).abs() <= 0.01);
    assert!((motion.segment_end_pos.x - 2.0).abs() <= 0.01);
}

#[test]
fn mid_walk_chase_keeps_current_segment_when_its_end_is_already_attackable() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(12.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "keep_attackable_segment_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let player_net_id = EntityId(5_622);
    let target_pos = LocalPos::new(3.6, 1.0);
    let _map_rx = enter_player(&inbound_tx, PlayerId::from(1), player_net_id, target_pos);
    advance_tick(&mut app);

    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    {
        let mut entity = app.world_mut().entity_mut(mob_entity);
        entity.get_mut::<LocalTransform>().expect("transform").pos = LocalPos::new(1.0, 1.0);
        entity.get_mut::<LocalTransform>().expect("transform").rot = east_rot();
        entity.get_mut::<MobMotion>().expect("mob motion").0 = MobMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(2.2, 1.0),
            segment_start_at: sim_ms(800),
            segment_end_at: sim_ms(1_600),
        };
    }
    set_mob_chasing(&mut app, mob_entity, player_net_id, now_ms, 10_000);

    run_mob_ai(&mut app);

    assert!(
        pending_movements(&app)
            .into_iter()
            .all(|movement| movement.kind != MovementKind::Wait),
        "a segment that already ends inside attack range should not be replaced mid-walk"
    );
}

#[test]
fn issue_mob_action_snaps_endpoints_to_wire_centimeters() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(8.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "wire_cm_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, _inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let now_ms = 1_000;
    let now_ts = 77;
    let current_pos = LocalPos::new(1.009, 1.004);
    let target_pos = LocalPos::new(3.019, 1.116);
    let current_rot = east_rot();
    let expected_start = LocalPos::new(1.00, 1.00);
    let expected_end = LocalPos::new(3.01, 1.11);
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(&mut app, mob_entity, current_pos, current_rot, now_ms);
    let shared = app.world().resource::<SharedConfig>().clone();
    let map_entity = map_entity(&app);

    let duration = super::mob_motion::issue_mob_action(
        app.world_mut(),
        map_entity,
        mob_entity,
        mob_net_id,
        MovementKind::Wait,
        current_pos,
        target_pos,
        super::util::rotation_from_delta(expected_start, expected_end, current_rot),
        mob_id,
        100,
        &shared,
        now_ms,
        now_ts,
        None,
    )
    .expect("issued wait movement");

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.kind == MovementKind::Wait)
        .expect("wait packet");
    let expected_duration = super::util::calculate_mob_move_duration_ms(
        shared.motion_speeds.as_ref(),
        mob_id,
        crate::motion::MotionMoveMode::Run,
        100,
        expected_start,
        expected_end,
    );
    let motion = app
        .world()
        .entity(mob_entity)
        .get::<MobMotion>()
        .map(|motion| motion.0)
        .expect("mob motion");
    assert_eq!(movement.new_pos, expected_end);
    assert_eq!(motion.segment_start_pos, expected_start);
    assert_eq!(motion.segment_end_pos, expected_end);
    assert_eq!(duration, expected_duration);
    assert_eq!(movement.duration, expected_duration);
}

#[test]
fn returning_mob_issues_wait_home_segment() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(16.0, 8.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "return_home_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            150,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, _inbound_tx) = build_runtime_app(shared, map, false);

    let mob_entity = first_mob_entity(&mut app);
    let mob_net_id = first_mob_net_id(&mut app);
    let now_ms = 1_000;
    set_sim_now(&mut app, now_ms);
    clear_pending_movements(&mut app);
    set_stationary_mob(
        &mut app,
        mob_entity,
        LocalPos::new(6.0, 1.0),
        east_rot(),
        now_ms,
    );
    set_mob_returning(&mut app, mob_entity, now_ms);

    run_mob_ai(&mut app);

    let movement = pending_movements(&app)
        .into_iter()
        .find(|movement| movement.entity_id == mob_net_id)
        .expect("return-home packet");
    assert_eq!(movement.kind, MovementKind::Wait);
    assert_eq!(movement.new_pos, LocalPos::new(1.0, 1.0));
    assert_eq!(
        movement.rot,
        super::util::rotation_from_delta(
            LocalPos::new(6.0, 1.0),
            LocalPos::new(1.0, 1.0),
            east_rot()
        )
    );
    assert_eq!(
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.mode()),
        Some(MobBrainMode::Return)
    );
}

#[test]
fn attacking_one_group_member_causes_the_whole_pack_to_retaliate() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Group(Arc::from([mob_id, mob_id, mob_id])),
        area: SpawnArea::new(LocalPos::new(6_400.0, 6_400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto_with_combat(
            mob_id,
            MobKind::Monster,
            "pack_wolf",
            MobRank::Pawn,
            MobBattleType::Melee,
            1,
            100,
            100,
            0,
            250,
            BehaviorFlags::empty(),
        ),
    );
    let (mut app, inbound_tx) = build_runtime_app(shared, map, true);

    let (mob_net_ids, mob_entities) = {
        let world = app.world_mut();
        let mut q = world.query::<(
            Entity,
            &MobMarker,
            &NetEntityId,
            Option<&super::state::MobPackId>,
        )>();
        let rows = q
            .iter(world)
            .map(|(entity, _, net_id, pack_id)| {
                (entity, net_id.net_id, pack_id.map(|id| id.pack_id))
            })
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 3);
        let first_pack = rows[0].2.expect("pack id");
        assert!(
            rows.iter()
                .all(|(_, _, pack_id)| pack_id == &Some(first_pack))
        );
        (
            rows.iter()
                .map(|(_, net_id, _)| *net_id)
                .collect::<Vec<_>>(),
            rows.iter()
                .map(|(entity, _, _)| *entity)
                .collect::<Vec<_>>(),
        )
    };

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_605);
    let mut map_rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(6_401.0, 6_400.0),
    );
    advance_tick(&mut app);
    let _ = drain_player_events(&mut map_rx);

    attack_target(&inbound_tx, player_id, mob_net_ids[0], 1);

    let mut events = Vec::new();
    for _ in 0..4 {
        advance_tick(&mut app);
        events.extend(drain_player_events(&mut map_rx));
    }

    let attack_emitters = movement_events(&events)
        .into_iter()
        .filter_map(|(entity_id, kind)| (kind == MovementKind::Attack).then_some(entity_id))
        .collect::<Vec<_>>();
    assert!(
        mob_net_ids
            .iter()
            .all(|mob_net_id| attack_emitters.contains(mob_net_id))
    );

    for mob_entity in mob_entities {
        let brain = app
            .world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .copied()
            .expect("pack member brain");
        assert_eq!(brain.target(), Some(player_net_id));
    }
}
