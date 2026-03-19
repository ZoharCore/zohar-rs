use super::*;
use crate::chat::MobChatContent;
use crate::navigation::{MapNavigator, TerrainFlagsGrid};
use bevy::prelude::{App, Entity};
use crossbeam_channel::Sender as InboundSender;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

use super::players::map_has_players;
use super::state::{
    LocalTransform, MapPendingMovements, MapSpawnRules, MobBrainMode, MobBrainState, MobMarker,
    MobMotion, MobMotionState, MoveIntent, MoveIntentQueue, NetEntityId, PendingMovement,
    PlayerIndex, RuntimeState,
};
use super::util::sample_player_motion_at;
use crate::api::{ClientIntent, PlayerEvent};
use crate::bridge::{ClientIntentMsg, EnterMsg, InboundEvent, LeaveMsg};
use crate::motion::EntityMotionSpeedTable;
use crate::outbox::PlayerOutbox;
use crate::types::MapInstanceKey;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::{LocalDistMeters, LocalPos, LocalPosExt, LocalRotation, LocalSize};
use zohar_domain::entity::mob::spawn::{
    Direction, FacingStrategy, SpawnArea, SpawnRuleDef, SpawnTemplate,
};
use zohar_domain::entity::mob::{MobBattleType, MobId, MobKind, MobPrototypeDef, MobRank};
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};
use zohar_domain::{BehaviorFlags, MapId, TerrainFlags};

fn test_configs(map_key: MapInstanceKey) -> (SharedConfig, MapConfig) {
    (
        SharedConfig {
            motion_speeds: Arc::new(EntityMotionSpeedTable::default()),
            mobs: Arc::new(HashMap::new()),
            wander: WanderConfig::default(),
            mob_chat: Arc::new(MobChatContent::default()),
        },
        MapConfig {
            map_key,
            empire: None,
            local_size: LocalSize::new(16_384.0, 16_384.0),
            navigator: None,
            spawn_rules: Vec::new(),
        },
    )
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
        bhv_flags,
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
        bhv_flags,
        empire: None,
    })
}

fn build_runtime_app(
    shared: SharedConfig,
    map: MapConfig,
    with_outbox: bool,
) -> (App, InboundSender<InboundEvent>) {
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(64);
    let mut app = App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    if with_outbox {
        app.add_plugins((
            ContentPlugin::new(shared, map),
            NetworkPlugin::new(inbound_rx),
            MapPlugin,
            SimulationPlugin,
            OutboxPlugin,
        ));
    } else {
        app.add_plugins((
            ContentPlugin::new(shared, map),
            NetworkPlugin::new(inbound_rx),
            MapPlugin,
            SimulationPlugin,
        ));
    }
    app.update();
    (app, inbound_tx)
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
    inbound_tx: &InboundSender<InboundEvent>,
    player_id: PlayerId,
    player_net_id: EntityId,
    initial_pos: LocalPos,
) -> Receiver<PlayerEvent> {
    enter_player_with_appearance(
        inbound_tx,
        player_id,
        player_net_id,
        initial_pos,
        PlayerAppearance::default(),
    )
}

fn enter_player_with_appearance(
    inbound_tx: &InboundSender<InboundEvent>,
    player_id: PlayerId,
    player_net_id: EntityId,
    initial_pos: LocalPos,
    appearance: PlayerAppearance,
) -> Receiver<PlayerEvent> {
    let (map_tx, map_rx) = tokio::sync::mpsc::channel(64);
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id,
                player_net_id,
                initial_pos,
                appearance,
                outbox: PlayerOutbox::new(map_tx),
            },
        })
        .expect("player enter");
    map_rx
}

fn attack_target(
    inbound_tx: &InboundSender<InboundEvent>,
    player_id: PlayerId,
    target: EntityId,
    attack_type: u8,
) {
    inbound_tx
        .send(InboundEvent::ClientIntent {
            msg: ClientIntentMsg {
                player_id,
                intent: ClientIntent::Attack {
                    target,
                    attack_type,
                },
            },
        })
        .expect("attack intent");
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
            PlayerEvent::EntityMove {
                entity_id, kind, ..
            } => Some((*entity_id, *kind)),
            _ => None,
        })
        .collect()
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

fn run_mob_ai(app: &mut App) {
    super::mob_brain::mob_brain_tick(app.world_mut());
    super::mob_chase::mob_chase_tick(app.world_mut());
}

fn east_rot() -> u8 {
    super::util::rotation_from_delta(LocalPos::new(1.0, 1.0), LocalPos::new(2.0, 1.0), 0)
}

fn north_rot() -> u8 {
    super::util::rotation_from_delta(LocalPos::new(1.0, 1.0), LocalPos::new(1.0, 2.0), 0)
}

fn set_stationary_mob(app: &mut App, mob_entity: Entity, pos: LocalPos, rot: u8, now_ms: u64) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    entity.get_mut::<LocalTransform>().expect("transform").pos = pos;
    entity.get_mut::<LocalTransform>().expect("transform").rot = rot;
    entity.get_mut::<MobMotion>().expect("mob motion").0 = MobMotionState {
        segment_start_pos: pos,
        segment_end_pos: pos,
        segment_start_at_ms: now_ms,
        segment_end_at_ms: now_ms,
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
    let mut brain = entity.get_mut::<MobBrainState>().expect("brain");
    brain.mode = MobBrainMode::Chasing;
    brain.target = Some(target);
    brain.target_locked_at_ms = now_ms;
    brain.next_attack_at_ms = next_attack_at_ms;
    brain.attack_windup_until_ms = 0;
    brain.next_chase_rethink_at_ms = now_ms;
}

fn set_mob_returning(app: &mut App, mob_entity: Entity, now_ms: u64) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    let mut brain = entity.get_mut::<MobBrainState>().expect("brain");
    brain.mode = MobBrainMode::Returning;
    brain.target = None;
    brain.target_locked_at_ms = 0;
    brain.next_attack_at_ms = 0;
    brain.attack_windup_until_ms = 0;
    brain.next_chase_rethink_at_ms = now_ms;
}

fn set_mob_idle(app: &mut App, mob_entity: Entity, now_ms: u64) {
    let mut entity = app.world_mut().entity_mut(mob_entity);
    let mut brain = entity.get_mut::<MobBrainState>().expect("brain");
    brain.mode = MobBrainMode::Idle;
    brain.target = None;
    brain.target_locked_at_ms = 0;
    brain.next_attack_at_ms = 0;
    brain.attack_windup_until_ms = 0;
    brain.next_chase_rethink_at_ms = 0;
    brain.next_wander_decision_at_ms = now_ms;
    brain.wander_wait_until_ms = None;
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
        segment_start_ts: 100,
        segment_end_ts: 200,
        last_client_ts: 101,
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
        segment_start_ts: 100,
        segment_end_ts: 200,
        last_client_ts: 101,
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
        segment_start_ts: 100,
        segment_end_ts: 200,
        last_client_ts: 175,
    };

    let pos = sample_player_motion_at(LocalPos::new(7.5, 0.0), &mut motion, 150);
    assert!((pos.x - 7.5).abs() < 0.01);
    assert!((pos.y - 0.0).abs() < 0.01);
}

#[test]
fn simulation_plugin_preloads_map_spawns() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6_400.0, 6_400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 3,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        test_mob_proto(
            mob_id,
            MobKind::Monster,
            "test_mob",
            MobRank::Pawn,
            1,
            100,
            100,
            BehaviorFlags::empty(),
        ),
    );

    let (mut app, _inbound_tx) = build_runtime_app(shared, map, false);
    let map_entity = app
        .world()
        .resource::<RuntimeState>()
        .map_entity
        .expect("map entity initialized");
    let spawn_rules = app
        .world()
        .entity(map_entity)
        .get::<MapSpawnRules>()
        .expect("spawn rules attached");
    assert_eq!(spawn_rules.rules.len(), 1);
    assert_eq!(spawn_rules.rules[0].active_instances, 3);
    assert_eq!(spawn_rules.rules[0].entities.len(), 3);

    let world = app.world_mut();
    let mut mob_query = world.query::<&MobMarker>();
    assert_eq!(mob_query.iter(world).count(), 3);
}

#[test]
fn player_count_follows_enter_leave() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (shared, map) = test_configs(map_key);
    let (mut app, inbound_tx) = build_runtime_app(shared, map, false);

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_001);
    let _rx = enter_player(
        &inbound_tx,
        player_id,
        player_net_id,
        LocalPos::new(6_400.0, 6_400.0),
    );
    advance_tick(&mut app);
    assert_eq!(app.world().resource::<PlayerCount>().0, 1);
    assert!(map_has_players(app.world_mut()));

    inbound_tx
        .send(InboundEvent::PlayerLeave {
            msg: LeaveMsg {
                player_id,
                player_net_id,
            },
        })
        .expect("player leave");
    advance_tick(&mut app);
    assert_eq!(app.world().resource::<PlayerCount>().0, 0);
    assert!(!map_has_players(app.world_mut()));
}

#[test]
fn startup_ready_signal_fires_after_map_bootstrap() {
    let map_id = MapId::new(41);
    let (shared, map) = test_configs(MapInstanceKey::shared(1, map_id));
    let (_inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (startup_tx, startup_rx) = tokio::sync::oneshot::channel();

    let mut app = App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.insert_resource(StartupReadySignal::new(startup_tx));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    assert!(startup_rx.blocking_recv().is_ok());
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
    {
        let mut ent = app.world_mut().entity_mut(player_entity);
        ent.get_mut::<MoveIntentQueue>()
            .expect("move queue")
            .0
            .push(MoveIntent {
                kind: MovementKind::Move,
                arg: 0,
                rot: 0,
                target: LocalPos::new(5.0, 0.0),
                ts: 1_000,
            });
    }

    advance_tick(&mut app);

    let transform = app
        .world()
        .entity(player_entity)
        .get::<LocalTransform>()
        .expect("transform");
    assert_eq!(transform.pos, LocalPos::new(5.0, 0.0));
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
    {
        let mut ent = app.world_mut().entity_mut(player_entity);
        ent.get_mut::<MoveIntentQueue>()
            .expect("move queue")
            .0
            .push(MoveIntent {
                kind: MovementKind::Move,
                arg: 0,
                rot: 0,
                target: LocalPos::new(12.5, 9.5),
                ts: 1_000,
            });
    }

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
    assert_eq!(brain.target, Some(player_net_id));
    assert!(matches!(
        brain.mode,
        MobBrainMode::Attacking | MobBrainMode::Chasing
    ));
}

#[test]
fn mob_chase_routes_around_navigation_blockers_and_emits_wait() {
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
    assert_eq!(movement.kind, MovementKind::Wait);
    assert_eq!(movement.rot, east_rot());
    assert!(
        navigator.segment_clear(LocalPos::new(1.0, 1.0), movement.new_pos),
        "terrain-aware chase should only emit a clear first segment"
    );
    assert!(
        (movement.new_pos.y - 1.0).abs() > 0.01,
        "routed chase should deviate off the blocked straight line"
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
    let follow_distance = super::mob_chase::mob_follow_distance_m(1.5);
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
    let follow_distance = super::mob_chase::mob_follow_distance_m(2.5);
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
            .map(|brain| brain.mode),
        Some(MobBrainMode::Attacking)
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms.saturating_add(2_000);
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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

    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms.saturating_add(100);
    run_mob_ai(&mut app);

    assert!(pending_movements(&app).is_empty());
    assert_eq!(
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.mode),
        Some(MobBrainMode::Attacking)
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
        .map(|brain| brain.attack_windup_until_ms)
        .expect("brain windup");
    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    app.world_mut()
        .entity_mut(player_entity)
        .get_mut::<LocalTransform>()
        .expect("player transform")
        .pos = LocalPos::new(12.0, 1.0);

    clear_pending_movements(&mut app);
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = windup_until_ms.saturating_add(1);
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
            .map(|brain| brain.mode),
        Some(MobBrainMode::Chasing)
    );
}

#[test]
fn mid_walk_chase_rethink_issues_wait_from_sampled_current_position_to_full_goal() {
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
    clear_pending_movements(&mut app);
    {
        let mut entity = app.world_mut().entity_mut(mob_entity);
        entity.get_mut::<LocalTransform>().expect("transform").pos = LocalPos::new(1.0, 1.0);
        entity.get_mut::<LocalTransform>().expect("transform").rot = east_rot();
        entity.get_mut::<MobMotion>().expect("mob motion").0 = MobMotionState {
            segment_start_pos: LocalPos::new(1.0, 1.0),
            segment_end_pos: LocalPos::new(5.0, 1.0),
            segment_start_at_ms: 800,
            segment_end_at_ms: 1_600,
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
    let follow_distance = super::mob_chase::mob_follow_distance_m(1.5);
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = now_ms;
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
        app.world()
            .entity(mob_entity)
            .get::<MobBrainState>()
            .map(|brain| brain.mode),
        Some(MobBrainMode::Returning)
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
        assert_eq!(brain.target, Some(player_net_id));
    }
}
