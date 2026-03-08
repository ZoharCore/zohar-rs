use super::*;
use crate::chat::MobChatContent;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Receiver;

use super::players::map_has_players;
use super::state::{
    ChatIntent, ChatIntentQueue, LocalTransform, MapPendingMovements, MapSpawnRules, MobMarker,
    MoveIntent, MoveIntentQueue, NetEntityId, PlayerIndex, RuntimeState, WanderState,
};
use super::util::sample_player_motion_at;
use crate::api::{ClientIntent, PlayerEvent};
use crate::bridge::{ClientIntentMsg, EnterMsg, InboundEvent, LeaveMsg, MapEventSender};
use crate::motion::EntityMotionSpeedTable;
use crate::outbox::PlayerOutbox;
use crate::types::MapInstanceKey;
use zohar_domain::appearance::PlayerAppearance;
use zohar_domain::coords::{LocalPos, LocalSize};
use zohar_domain::entity::mob::spawn::{
    Direction, FacingStrategy, SpawnArea, SpawnRuleDef, SpawnTemplate,
};
use zohar_domain::entity::mob::{MobId, MobKind, MobPrototypeDef, MobRank};
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};
use zohar_domain::{BehaviorFlags, MapId};

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
            spawn_rules: Vec::new(),
        },
    )
}

fn test_wander_config(step_min_m: f32, step_max_m: f32) -> WanderConfig {
    WanderConfig {
        decision_pause_idle_min: Duration::ZERO,
        decision_pause_idle_max: Duration::ZERO,
        post_move_pause_min: Duration::ZERO,
        post_move_pause_max: Duration::ZERO,
        wander_chance_denominator: 1,
        step_min_m,
        step_max_m,
    }
}

fn advance_tick(app: &mut bevy::prelude::App) {
    run_pre_update(app);
    run_fixed_first(app);
    run_fixed_update(app);
    run_fixed_post_update(app);
}

fn run_pre_update(app: &mut bevy::prelude::App) {
    let _ = app.world_mut().try_run_schedule(bevy::prelude::PreUpdate);
}

fn run_fixed_first(app: &mut bevy::prelude::App) {
    app.world_mut().run_schedule(bevy::prelude::FixedFirst);
}

fn run_fixed_update(app: &mut bevy::prelude::App) {
    app.world_mut().run_schedule(bevy::prelude::FixedUpdate);
}

fn run_fixed_post_update(app: &mut bevy::prelude::App) {
    app.world_mut().run_schedule(bevy::prelude::FixedPostUpdate);
}

fn drain_player_events(rx: &mut Receiver<PlayerEvent>) -> Vec<PlayerEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

fn first_local_chat(events: &[PlayerEvent]) -> Option<(Option<EntityId>, Vec<u8>)> {
    events.iter().find_map(|event| match event {
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            message,
            ..
        } if *kind == 0 => Some((*sender_entity_id, message.clone())),
        _ => None,
    })
}

fn first_spawn(events: &[PlayerEvent]) -> Option<EntityId> {
    events.iter().find_map(|event| match event {
        PlayerEvent::EntitySpawn { show, .. } => Some(show.entity_id),
        _ => None,
    })
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

#[test]
fn sample_player_motion_at_interpolates_within_segment() {
    let mut motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(0.0, 0.0),
        segment_end_pos: LocalPos::new(10.0, 0.0),
        segment_start_ts: 1_000,
        segment_end_ts: 2_000,
        last_client_ts: 1_000,
    };

    let sampled = sample_player_motion_at(LocalPos::new(0.0, 0.0), &mut motion, 1_500);
    assert!((sampled.x - 5.0).abs() < 0.001);
    assert!((sampled.y - 0.0).abs() < 0.001);
}

#[test]
fn sample_player_motion_at_clamps_to_segment_end_after_overshoot() {
    let mut motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(0.0, 0.0),
        segment_end_pos: LocalPos::new(3.0, 4.0),
        segment_start_ts: 5_000,
        segment_end_ts: 5_500,
        last_client_ts: 5_000,
    };

    let sampled = sample_player_motion_at(LocalPos::new(0.0, 0.0), &mut motion, 5_800);
    assert!((sampled.x - 3.0).abs() < 0.001);
    assert!((sampled.y - 4.0).abs() < 0.001);
}

#[test]
fn sample_player_motion_at_keeps_current_pos_for_stale_ts() {
    let mut motion = super::state::PlayerMotionState {
        segment_start_pos: LocalPos::new(1.0, 1.0),
        segment_end_pos: LocalPos::new(9.0, 1.0),
        segment_start_ts: 10_000,
        segment_end_ts: 11_000,
        last_client_ts: 10_500,
    };

    let current = LocalPos::new(4.0, 1.0);
    let sampled = sample_player_motion_at(current, &mut motion, 10_400);
    assert!((sampled.x - current.x).abs() < 0.001);
    assert!((sampled.y - current.y).abs() < 0.001);
}

#[test]
fn simulation_plugin_preloads_map_spawns() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 3,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

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
    assert!(spawn_rules.scheduled_spawns.is_empty());

    let mob_count = {
        let world = app.world_mut();
        let mut mob_query = world.query::<&super::state::MobMarker>();
        mob_query.iter(world).count()
    };
    assert_eq!(mob_count, 3);
}

#[test]
fn simulation_plugin_preloads_full_group_for_single_group_instance() {
    let map_id = MapId::new(41);
    let leader_id = MobId::new(20025);
    let pony_id = MobId::new(20029);
    let horse_id = MobId::new(20030);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Group(Arc::from([
            leader_id, pony_id, pony_id, horse_id, horse_id,
        ])),
        area: SpawnArea::new(LocalPos::new(714.0, 566.0), LocalSize::new(1.0, 1.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));

    for mob_id in [leader_id, pony_id, horse_id] {
        Arc::make_mut(&mut shared.mobs).insert(
            mob_id,
            Arc::new(MobPrototypeDef {
                mob_id,
                mob_kind: MobKind::Npc,
                name: format!("mob_{mob_id:?}"),
                rank: MobRank::Pawn,
                level: 1,
                move_speed: 100,
                attack_speed: 100,
                bhv_flags: BehaviorFlags::empty(),
                empire: None,
            }),
        );
    }

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

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
    assert_eq!(spawn_rules.rules[0].active_instances, 1);
    assert_eq!(spawn_rules.rules[0].entities.len(), 5);
    assert!(spawn_rules.scheduled_spawns.is_empty());

    let mut counts = HashMap::<MobId, usize>::new();
    let world = app.world_mut();
    let mut mob_query = world.query::<&super::state::MobRef>();
    for mob_ref in mob_query.iter(world) {
        *counts.entry(mob_ref.mob_id).or_default() += 1;
    }

    assert_eq!(counts.get(&leader_id), Some(&1));
    assert_eq!(counts.get(&pony_id), Some(&2));
    assert_eq!(counts.get(&horse_id), Some(&2));
}

#[test]
fn grouped_spawn_near_map_edge_keeps_all_members_in_bounds() {
    let map_id = MapId::new(41);
    let leader_id = MobId::new(20025);
    let follower_id = MobId::new(20029);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(4.0, 4.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Group(Arc::from([leader_id, follower_id, follower_id])),
        area: SpawnArea::new(LocalPos::new(0.0, 0.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));

    for mob_id in [leader_id, follower_id] {
        Arc::make_mut(&mut shared.mobs).insert(
            mob_id,
            Arc::new(MobPrototypeDef {
                mob_id,
                mob_kind: MobKind::Npc,
                name: format!("mob_{mob_id:?}"),
                rank: MobRank::Pawn,
                level: 1,
                move_speed: 100,
                attack_speed: 100,
                bhv_flags: BehaviorFlags::empty(),
                empire: None,
            }),
        );
    }

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let world = app.world_mut();
    let mut mob_query = world.query::<(&super::state::MobRef, &LocalTransform)>();
    let positions: Vec<(MobId, LocalPos)> = mob_query
        .iter(world)
        .map(|(mob_ref, transform)| (mob_ref.mob_id, transform.pos))
        .collect();

    assert_eq!(positions.len(), 3, "all grouped members should spawn");
    for (_, pos) in &positions {
        assert!(
            pos.x >= 0.0 && pos.x < 4.0 && pos.y >= 0.0 && pos.y < 4.0,
            "grouped spawn member must stay within map bounds: {pos:?}"
        );
    }
    assert!(
        positions
            .iter()
            .all(|(_, pos)| *pos == LocalPos::new(0.0, 0.0)),
        "zero-area edge spawn should keep grouped members inside the authored spawn area"
    );
}

#[test]
fn simulation_plugin_applies_fixed_spawn_facing_rotation() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Fixed(Direction::East),
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let expected = super::util::degrees_to_protocol_rot(Direction::East.to_angle());
    let actual = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.rot)
            .expect("preloaded mob rotation")
    };
    assert_eq!(actual, expected);
}

#[test]
fn monster_wander_advances_without_players_at_idle_rate() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(5.0, 5.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let before = {
        let world = app.world_mut();
        let mut q = world.query::<(&super::state::MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.pos)
            .expect("preloaded mob position")
    };

    for _ in 0..3 {
        advance_tick(&mut app);
    }

    let after = {
        let world = app.world_mut();
        let mut q = world.query::<(&super::state::MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.pos)
            .expect("mob position after idle ticks")
    };

    assert!(
        (before.x - after.x).abs() > f32::EPSILON || (before.y - after.y).abs() > f32::EPSILON,
        "mob should wander even when there are no players"
    );
}

#[test]
fn monster_wander_emits_wait_with_duration() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(5.0, 5.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let map_entity = app
        .world()
        .resource::<RuntimeState>()
        .map_entity
        .expect("map entity initialized");

    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let pending = app
        .world()
        .entity(map_entity)
        .get::<MapPendingMovements>()
        .expect("map pending movements attached");
    assert_eq!(pending.0.len(), 1, "expected one initial wander packet");
    let movement = pending.0[0];
    assert_eq!(movement.kind, MovementKind::Wait);
    assert!(
        movement.duration > 0,
        "wander movement packet must carry travel duration"
    );
}

#[test]
fn monster_wander_does_not_emit_terminal_wait_packet() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = WanderConfig {
        post_move_pause_min: Duration::from_secs(10),
        post_move_pause_max: Duration::from_secs(10),
        ..test_wander_config(5.0, 5.0)
    };
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let map_entity = app
        .world()
        .resource::<RuntimeState>()
        .map_entity
        .expect("map entity initialized");

    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let wait_at_ms = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &WanderState)>();
        q.iter(world)
            .next()
            .and_then(|(_, wander)| wander.0.pending_wait_at_ms)
            .expect("mob should have a pending wait deadline after wander start")
    };

    {
        let pending = app
            .world()
            .entity(map_entity)
            .get::<MapPendingMovements>()
            .expect("map pending movements attached");
        assert_eq!(pending.0.len(), 1, "expected one initial wander packet");
        assert_eq!(pending.0[0].kind, MovementKind::Wait);
        assert!(pending.0[0].duration > 0);
    }

    {
        let mut map_ent = app.world_mut().entity_mut(map_entity);
        map_ent
            .get_mut::<MapPendingMovements>()
            .expect("map pending movements attached")
            .0
            .clear();
    }

    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = wait_at_ms;
    run_fixed_update(&mut app);

    let pending_after = app
        .world()
        .entity(map_entity)
        .get::<MapPendingMovements>()
        .expect("map pending movements attached");
    assert!(
        pending_after.0.is_empty(),
        "wander wait expiry should not emit an extra terminal wait packet"
    );

    let wander_pending = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &WanderState)>();
        q.iter(world)
            .next()
            .map(|(_, wander)| wander.0.pending_wait_at_ms)
    };
    assert_eq!(
        wander_pending,
        Some(None),
        "mob pending wait should be cleared after wait expiry"
    );
}

#[test]
fn movable_npc_gets_wander_state_and_moves() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(20_041);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(3.0, 3.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Npc,
            name: "beggar".to_string(),
            rank: MobRank::King,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let (before, has_wander_state) = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform, Option<&WanderState>)>();
        q.iter(world)
            .next()
            .map(|(_, transform, wander)| (transform.pos, wander.is_some()))
            .expect("npc position")
    };
    assert!(has_wander_state, "movable NPC should get WanderState");

    for _ in 0..3 {
        advance_tick(&mut app);
    }

    let after = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.pos)
            .expect("npc position after idle ticks")
    };

    assert!(
        (before.x - after.x).abs() > f32::EPSILON || (before.y - after.y).abs() > f32::EPSILON,
        "movable NPC should wander"
    );
}

#[test]
fn nomove_npc_does_not_get_wander_state_or_move() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(20_349);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(3.0, 3.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Npc,
            name: "stable boy".to_string(),
            rank: MobRank::King,
            level: 70,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::NO_MOVE,
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let (before, has_wander_state) = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform, Option<&WanderState>)>();
        q.iter(world)
            .next()
            .map(|(_, transform, wander)| (transform.pos, wander.is_some()))
            .expect("npc position")
    };
    assert!(!has_wander_state, "NOMOVE NPC should not get WanderState");

    for _ in 0..3 {
        advance_tick(&mut app);
    }

    let after = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.pos)
            .expect("npc position after idle ticks")
    };

    assert_eq!(before, after, "NOMOVE NPC must stay fixed");
}

#[test]
fn idle_wander_can_leave_original_spawn_bounds() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(20_029);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(7.0, 7.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Npc,
            name: "pony".to_string(),
            rank: MobRank::King,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();
    app.world_mut().resource_mut::<RuntimeState>().rng = SmallRng::seed_from_u64(0xC0FFEE);

    let spawn_pos = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.pos)
            .expect("npc position after spawn")
    };

    let mut max_distance = 0.0f32;
    for step in 0..12 {
        run_pre_update(&mut app);
        run_fixed_first(&mut app);
        app.world_mut().resource_mut::<RuntimeState>().sim_time_ms = 10_000 * (step + 1);
        run_fixed_update(&mut app);
        run_fixed_post_update(&mut app);
        let pos = {
            let world = app.world_mut();
            let mut q = world.query::<(&MobMarker, &LocalTransform)>();
            q.iter(world)
                .next()
                .map(|(_, transform)| transform.pos)
                .expect("npc position after wander tick")
        };
        max_distance = max_distance.max((pos - spawn_pos).length());
    }

    assert!(
        max_distance > 10.0,
        "wander should be able to leave the original zero-area spawn bounds, max_distance={max_distance}"
    );
}

#[test]
fn invalid_idle_wander_sample_skips_movement_and_reschedules() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.local_size = LocalSize::new(4.0, 4.0);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(1.0, 1.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(7.0, 7.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let before = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform)>();
        q.iter(world)
            .next()
            .map(|(_, transform)| transform.pos)
            .expect("mob position")
    };

    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let after = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &LocalTransform, &WanderState)>();
        q.iter(world)
            .next()
            .map(|(_, transform, wander)| (transform.pos, wander.0))
            .expect("mob position after invalid wander")
    };
    assert_eq!(
        before, after.0,
        "invalid wander sample must not move the mob"
    );
    assert!(
        after.1.pending_wait_at_ms.is_none(),
        "invalid wander sample must not start a movement wait"
    );
    assert!(
        after.1.next_decision_at_ms >= app.world().resource::<RuntimeState>().sim_time_ms,
        "invalid wander sample must reschedule the next decision"
    );

    let map_entity = app
        .world()
        .resource::<RuntimeState>()
        .map_entity
        .expect("map entity initialized");
    let pending = app
        .world()
        .entity(map_entity)
        .get::<MapPendingMovements>()
        .expect("map pending movements attached");
    assert!(
        pending.0.is_empty(),
        "invalid wander sample must not enqueue movement packets"
    );
}

#[test]
fn zero_distance_idle_wander_reschedules_without_packet() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    shared.wander = test_wander_config(0.0, 0.0);
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );

    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    run_pre_update(&mut app);
    run_fixed_first(&mut app);
    run_fixed_update(&mut app);

    let map_entity = app
        .world()
        .resource::<RuntimeState>()
        .map_entity
        .expect("map entity initialized");
    let pending = app
        .world()
        .entity(map_entity)
        .get::<MapPendingMovements>()
        .expect("map pending movements attached");
    assert!(
        pending.0.is_empty(),
        "zero-distance idle wander must not emit movement packets"
    );

    let wander = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &WanderState)>();
        q.iter(world)
            .next()
            .map(|(_, wander)| wander.0)
            .expect("wander state")
    };
    assert!(wander.pending_wait_at_ms.is_none());
    assert!(
        wander.next_decision_at_ms >= app.world().resource::<RuntimeState>().sim_time_ms,
        "zero-distance idle wander must reschedule the next decision"
    );
}

#[test]
fn wander_validator_checks_midpoint_and_endpoint_against_map_bounds() {
    let map_size = LocalSize::new(10.0, 10.0);
    let current = LocalPos::new(5.0, 5.0);

    assert!(super::wander::is_wander_target_allowed(
        map_size,
        current,
        LocalPos::new(8.0, 8.0),
    ));
    assert!(!super::wander::is_wander_target_allowed(
        map_size,
        current,
        LocalPos::new(10.0, 8.0),
    ));
    assert!(!super::wander::is_wander_target_allowed(
        map_size,
        current,
        LocalPos::new(8.0, -0.1),
    ));
}

#[test]
fn player_count_follows_enter_leave() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5001);
    let (map_tx, _map_rx) = tokio::sync::mpsc::channel(8);
    let msg = EnterMsg {
        player_id,
        player_net_id,
        initial_pos: LocalPos::new(6400.0, 6400.0),
        appearance: PlayerAppearance::default(),
        outbox: PlayerOutbox::new(map_tx),
    };
    inbound_tx
        .send(InboundEvent::PlayerEnter { msg })
        .expect("send enter event");
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
        .expect("send leave event");
    advance_tick(&mut app);
    assert_eq!(app.world().resource::<PlayerCount>().0, 0);
    assert!(!map_has_players(app.world_mut()));
}

#[test]
fn fixed_schedule_order_is_first_update_post() {
    #[derive(bevy::prelude::Resource, Default)]
    struct Trace(Vec<&'static str>);

    fn first(mut trace: bevy::prelude::ResMut<Trace>) {
        trace.0.push("first");
    }
    fn sim(mut trace: bevy::prelude::ResMut<Trace>) {
        trace.0.push("sim");
    }
    fn post(mut trace: bevy::prelude::ResMut<Trace>) {
        trace.0.push("post");
    }

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.init_resource::<Trace>();
    app.add_systems(bevy::prelude::FixedFirst, first);
    app.add_systems(bevy::prelude::FixedUpdate, sim);
    app.add_systems(bevy::prelude::FixedPostUpdate, post);

    advance_tick(&mut app);

    let trace = &app.world().resource::<Trace>().0;
    assert_eq!(trace, &vec!["first", "sim", "post"]);
}

#[test]
fn startup_ready_signal_fires_after_map_bootstrap() {
    let map_id = MapId::new(41);
    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let (shared, map) = test_configs(MapInstanceKey::shared(1, map_id));
    let (startup_tx, startup_rx) = tokio::sync::oneshot::channel();

    let mut app = bevy::prelude::App::new();
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

    assert!(
        startup_rx.blocking_recv().is_ok(),
        "startup ready signal should be emitted after startup schedules run"
    );
}

#[test]
fn process_intents_consumes_directly_inserted_queue() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5001);
    let (map_tx, _map_rx) = tokio::sync::mpsc::channel(8);
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id,
                player_net_id,
                initial_pos: LocalPos::new(0.0, 0.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(map_tx),
            },
        })
        .expect("enter");
    advance_tick(&mut app);

    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    {
        let mut ent = app.world_mut().entity_mut(player_entity);
        let mut queue = ent.get_mut::<MoveIntentQueue>().expect("move queue exists");
        queue.0.push(MoveIntent {
            kind: MovementKind::Move,
            arg: 0,
            rot: 0,
            target: LocalPos::new(5.0, 0.0),
            ts: 1000,
        });
    }

    advance_tick(&mut app);

    let transform = app
        .world()
        .entity(player_entity)
        .get::<LocalTransform>()
        .expect("transform exists");
    assert!(transform.pos.x > 0.0);
    let queue = app
        .world()
        .entity(player_entity)
        .get::<MoveIntentQueue>()
        .expect("queue exists");
    assert!(queue.0.is_empty(), "intent queue should be consumed");
}

#[test]
fn player_chat_events_include_sender_and_name_prefix() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (map_tx, mut map_rx) = tokio::sync::mpsc::channel(32);
    let player_id = PlayerId::from(1);
    let player_net_id = EntityId(5_001);
    let mut appearance = PlayerAppearance::default();
    appearance.name = "alice".to_string();

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id,
                player_net_id,
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance,
                outbox: PlayerOutbox::new(map_tx),
            },
        })
        .expect("enter");
    advance_tick(&mut app);

    let player_entity = app.world().resource::<PlayerIndex>().0[&player_id];
    {
        let mut ent = app.world_mut().entity_mut(player_entity);
        let mut queue = ent.get_mut::<ChatIntentQueue>().expect("chat queue exists");
        queue.0.push(ChatIntent {
            message: b"hello\0".to_vec(),
        });
    }

    advance_tick(&mut app);
    let events = drain_player_events(&mut map_rx);

    let Some((sender_entity_id, message)) = events.into_iter().find_map(|event| match event {
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            message,
            ..
        } if kind == 0 => Some((sender_entity_id, message)),
        _ => None,
    }) else {
        panic!("missing talking chat event");
    };

    assert_eq!(sender_entity_id, Some(player_net_id));
    assert_eq!(message, b"alice : hello\0");
}

#[test]
fn same_empire_talking_preserves_raw_payload_for_observers() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx_alice, mut rx_alice) = tokio::sync::mpsc::channel(32);
    let (tx_bob, mut rx_bob) = tokio::sync::mpsc::channel(32);
    let alice_id = PlayerId::from(1);
    let bob_id = PlayerId::from(2);
    let alice_net_id = EntityId(5_201);

    let mut alice_appearance = PlayerAppearance::default();
    alice_appearance.name = "alice".to_string();
    alice_appearance.empire = zohar_domain::Empire::Red;

    let mut bob_appearance = PlayerAppearance::default();
    bob_appearance.name = "bob".to_string();
    bob_appearance.empire = zohar_domain::Empire::Red;

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: alice_id,
                player_net_id: alice_net_id,
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: alice_appearance,
                outbox: PlayerOutbox::new(tx_alice),
            },
        })
        .expect("alice enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: bob_id,
                player_net_id: EntityId(5_202),
                initial_pos: LocalPos::new(6401.0, 6400.0),
                appearance: bob_appearance,
                outbox: PlayerOutbox::new(tx_bob),
            },
        })
        .expect("bob enter");

    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_alice);
    let _ = drain_player_events(&mut rx_bob);

    let alice_entity = app.world().resource::<PlayerIndex>().0[&alice_id];
    {
        let mut ent = app.world_mut().entity_mut(alice_entity);
        let mut queue = ent.get_mut::<ChatIntentQueue>().expect("chat queue exists");
        queue.0.push(ChatIntent {
            message: b"hello :-)\0".to_vec(),
        });
    }
    advance_tick(&mut app);
    let events_alice = drain_player_events(&mut rx_alice);
    let events_bob = drain_player_events(&mut rx_bob);

    let Some((sender_alice, msg_alice)) = events_alice.into_iter().find_map(|event| match event {
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            message,
            ..
        } if kind == 0 => Some((sender_entity_id, message)),
        _ => None,
    }) else {
        panic!("missing self talking chat event");
    };
    let Some((sender_bob, msg_bob)) = events_bob.into_iter().find_map(|event| match event {
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            message,
            ..
        } if kind == 0 => Some((sender_entity_id, message)),
        _ => None,
    }) else {
        panic!("missing observer talking chat event");
    };

    assert_eq!(sender_alice, Some(alice_net_id));
    assert_eq!(sender_bob, Some(alice_net_id));
    assert_eq!(msg_alice, b"alice : hello :-)\0");
    assert_eq!(msg_bob, msg_alice);
}

#[test]
fn cross_empire_talking_obfuscates_payload_for_observers() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();
    app.world_mut().resource_mut::<RuntimeState>().rng = SmallRng::seed_from_u64(0xC0FFEE);

    let (tx_alice, mut rx_alice) = tokio::sync::mpsc::channel(32);
    let (tx_bob, mut rx_bob) = tokio::sync::mpsc::channel(32);
    let alice_id = PlayerId::from(1);
    let bob_id = PlayerId::from(2);
    let alice_net_id = EntityId(5_301);

    let mut alice_appearance = PlayerAppearance::default();
    alice_appearance.name = "alice".to_string();
    alice_appearance.empire = zohar_domain::Empire::Red;

    let mut bob_appearance = PlayerAppearance::default();
    bob_appearance.name = "bob".to_string();
    bob_appearance.empire = zohar_domain::Empire::Blue;

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: alice_id,
                player_net_id: alice_net_id,
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: alice_appearance,
                outbox: PlayerOutbox::new(tx_alice),
            },
        })
        .expect("alice enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: bob_id,
                player_net_id: EntityId(5_302),
                initial_pos: LocalPos::new(6401.0, 6400.0),
                appearance: bob_appearance,
                outbox: PlayerOutbox::new(tx_bob),
            },
        })
        .expect("bob enter");

    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_alice);
    let _ = drain_player_events(&mut rx_bob);

    let alice_entity = app.world().resource::<PlayerIndex>().0[&alice_id];
    {
        let mut ent = app.world_mut().entity_mut(alice_entity);
        let mut queue = ent.get_mut::<ChatIntentQueue>().expect("chat queue exists");
        queue.0.push(ChatIntent {
            message: b"hello friend\0".to_vec(),
        });
    }
    advance_tick(&mut app);
    let events_alice = drain_player_events(&mut rx_alice);
    let events_bob = drain_player_events(&mut rx_bob);

    let Some((sender_alice, msg_alice)) = events_alice.into_iter().find_map(|event| match event {
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            message,
            ..
        } if kind == 0 => Some((sender_entity_id, message)),
        _ => None,
    }) else {
        panic!("missing self talking chat event");
    };
    let Some((sender_bob, msg_bob)) = events_bob.into_iter().find_map(|event| match event {
        PlayerEvent::Chat {
            kind,
            sender_entity_id,
            message,
            ..
        } if kind == 0 => Some((sender_entity_id, message)),
        _ => None,
    }) else {
        panic!("missing observer talking chat event");
    };

    assert_eq!(sender_alice, Some(alice_net_id));
    assert_eq!(sender_bob, Some(alice_net_id));
    assert_eq!(msg_alice, b"alice : hello friend\0");
    assert_ne!(msg_bob, msg_alice);
    assert!(msg_bob.starts_with(b"alice : "));
    assert_eq!(msg_bob.last().copied(), Some(0));
}

#[test]
fn local_talking_chat_only_reaches_visible_observers() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx_alice, mut rx_alice) = tokio::sync::mpsc::channel(32);
    let (tx_bob, mut rx_bob) = tokio::sync::mpsc::channel(32);
    let (tx_cara, mut rx_cara) = tokio::sync::mpsc::channel(32);

    let mut alice = PlayerAppearance::default();
    alice.name = "alice".to_string();
    alice.empire = zohar_domain::Empire::Red;

    let mut bob = PlayerAppearance::default();
    bob.name = "bob".to_string();
    bob.empire = zohar_domain::Empire::Red;

    let mut cara = PlayerAppearance::default();
    cara.name = "cara".to_string();
    cara.empire = zohar_domain::Empire::Red;

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(5_401),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: alice,
                outbox: PlayerOutbox::new(tx_alice),
            },
        })
        .expect("alice enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(2),
                player_net_id: EntityId(5_402),
                initial_pos: LocalPos::new(6401.0, 6400.0),
                appearance: bob,
                outbox: PlayerOutbox::new(tx_bob),
            },
        })
        .expect("bob enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(3),
                player_net_id: EntityId(5_403),
                initial_pos: LocalPos::new(6600.0, 6400.0),
                appearance: cara,
                outbox: PlayerOutbox::new(tx_cara),
            },
        })
        .expect("cara enter");

    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_alice);
    let _ = drain_player_events(&mut rx_bob);
    let _ = drain_player_events(&mut rx_cara);

    let alice_entity = app.world().resource::<PlayerIndex>().0[&PlayerId::from(1)];
    {
        let mut ent = app.world_mut().entity_mut(alice_entity);
        let mut queue = ent.get_mut::<ChatIntentQueue>().expect("chat queue exists");
        queue.0.push(ChatIntent {
            message: b"hello local\0".to_vec(),
        });
    }

    advance_tick(&mut app);

    let alice_chat = first_local_chat(&drain_player_events(&mut rx_alice));
    let bob_chat = first_local_chat(&drain_player_events(&mut rx_bob));
    let cara_chat = first_local_chat(&drain_player_events(&mut rx_cara));

    assert_eq!(
        alice_chat,
        Some((Some(EntityId(5_401)), b"alice : hello local\0".to_vec()))
    );
    assert_eq!(
        bob_chat,
        Some((Some(EntityId(5_401)), b"alice : hello local\0".to_vec()))
    );
    assert_eq!(
        cara_chat, None,
        "distant players must not receive local talk"
    );
}

#[test]
fn global_shout_remains_senderless_and_empire_scoped() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx_red_a, mut rx_red_a) = tokio::sync::mpsc::channel(32);
    let (tx_red_b, mut rx_red_b) = tokio::sync::mpsc::channel(32);
    let (tx_blue, mut rx_blue) = tokio::sync::mpsc::channel(32);

    let mut red_a = PlayerAppearance::default();
    red_a.name = "alice".to_string();
    red_a.empire = zohar_domain::Empire::Red;

    let mut red_b = PlayerAppearance::default();
    red_b.name = "eve".to_string();
    red_b.empire = zohar_domain::Empire::Red;

    let mut blue = PlayerAppearance::default();
    blue.name = "bob".to_string();
    blue.empire = zohar_domain::Empire::Blue;

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(6_101),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: red_a,
                outbox: PlayerOutbox::new(tx_red_a),
            },
        })
        .expect("red a enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(2),
                player_net_id: EntityId(6_102),
                initial_pos: LocalPos::new(6401.0, 6400.0),
                appearance: red_b,
                outbox: PlayerOutbox::new(tx_red_b),
            },
        })
        .expect("red b enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(3),
                player_net_id: EntityId(6_103),
                initial_pos: LocalPos::new(6402.0, 6400.0),
                appearance: blue,
                outbox: PlayerOutbox::new(tx_blue),
            },
        })
        .expect("blue enter");

    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_red_a);
    let _ = drain_player_events(&mut rx_red_b);
    let _ = drain_player_events(&mut rx_blue);

    inbound_tx
        .send(InboundEvent::GlobalShout {
            msg: crate::bridge::GlobalShoutMsg {
                from_player_name: "ann".to_string(),
                from_empire: zohar_domain::Empire::Red,
                message_bytes: b"wave\0".to_vec(),
            },
        })
        .expect("queue shout");
    advance_tick(&mut app);

    let events_red_a = drain_player_events(&mut rx_red_a);
    let events_red_b = drain_player_events(&mut rx_red_b);
    let events_blue = drain_player_events(&mut rx_blue);

    let Some((sender_a, empire_a, message_a)) =
        events_red_a.into_iter().find_map(|event| match event {
            PlayerEvent::Chat {
                kind,
                sender_entity_id,
                empire,
                message,
            } if kind == 6 => Some((sender_entity_id, empire, message)),
            _ => None,
        })
    else {
        panic!("missing shout for red recipient a");
    };
    let Some((sender_b, empire_b, message_b)) =
        events_red_b.into_iter().find_map(|event| match event {
            PlayerEvent::Chat {
                kind,
                sender_entity_id,
                empire,
                message,
            } if kind == 6 => Some((sender_entity_id, empire, message)),
            _ => None,
        })
    else {
        panic!("missing shout for red recipient b");
    };

    assert_eq!(sender_a, None);
    assert_eq!(sender_b, None);
    assert_eq!(empire_a, Some(zohar_domain::Empire::Red));
    assert_eq!(empire_b, Some(zohar_domain::Empire::Red));
    assert_eq!(message_a, b"ann : wave\0");
    assert_eq!(message_b, b"ann : wave\0");
    assert!(
        !events_blue.into_iter().any(|event| matches!(
            event,
            PlayerEvent::Chat { kind, .. } if kind == 6
        )),
        "blue recipient should not receive red shout"
    );
}

#[test]
fn monster_idle_chat_uses_strategy_and_sender_identity() {
    let map_id = MapId::new(41);
    let mob_id = MobId::new(101);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (mut shared, mut map) = test_configs(map_key);
    map.spawn_rules.push(Arc::new(SpawnRuleDef {
        template: SpawnTemplate::Mob(mob_id),
        area: SpawnArea::new(LocalPos::new(6400.0, 6400.0), LocalSize::new(0.0, 0.0)),
        facing: FacingStrategy::Random,
        max_count: 1,
        regen_time: Duration::from_secs(60),
    }));
    Arc::make_mut(&mut shared.mobs).insert(
        mob_id,
        Arc::new(MobPrototypeDef {
            mob_id,
            mob_kind: MobKind::Monster,
            name: "test_mob".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 100,
            attack_speed: 100,
            bhv_flags: BehaviorFlags::empty(),
            empire: None,
        }),
    );
    Arc::make_mut(&mut shared.mob_chat)
        .strategy_type_defaults
        .insert(
            ("idle".to_string(), MobKind::Monster),
            crate::MobChatStrategyInterval {
                interval_min_sec: 1,
                interval_max_sec: 1,
            },
        );
    Arc::make_mut(&mut shared.mob_chat)
        .lines_by_mob
        .entry(("idle".to_string(), mob_id))
        .or_default()
        .push(crate::MobChatLine {
            source_key: "idle.monster_chat_501_1".to_string(),
            text: "growl".to_string(),
        });

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let mob_net_id = {
        let world = app.world_mut();
        let mut q = world.query::<(&MobMarker, &NetEntityId)>();
        q.iter(world)
            .next()
            .map(|(_, net)| net.net_id)
            .expect("preloaded mob")
    };

    let (map_tx, mut map_rx) = tokio::sync::mpsc::channel(64);
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(5_101),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(map_tx),
            },
        })
        .expect("player enter");
    advance_tick(&mut app);

    let mut seen_idle_chat = None;
    for _ in 0..5 {
        app.world_mut()
            .resource_mut::<RuntimeState>()
            .packet_time_start = Instant::now() - Duration::from_secs(2);
        advance_tick(&mut app);
        for event in drain_player_events(&mut map_rx) {
            if let PlayerEvent::Chat {
                kind,
                sender_entity_id,
                message,
                ..
            } = event
            {
                if kind == 0 {
                    seen_idle_chat = Some((sender_entity_id, message));
                    break;
                }
            }
        }
        if seen_idle_chat.is_some() {
            break;
        }
    }

    let Some((sender_entity_id, message)) = seen_idle_chat else {
        panic!("expected idle monster chat event");
    };
    assert_eq!(sender_entity_id, Some(mob_net_id));
    assert_eq!(message, b"growl\0");
}

#[test]
fn player_enter_same_tick_does_not_duplicate_spawn_events() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx1, mut rx1) = tokio::sync::mpsc::channel(32);
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(32);
    let player_1 = PlayerId::from(1);
    let player_2 = PlayerId::from(2);
    let net_1 = EntityId(7_001);
    let net_2 = EntityId(7_002);

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: player_1,
                player_net_id: net_1,
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx1),
            },
        })
        .expect("queue first enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: player_2,
                player_net_id: net_2,
                initial_pos: LocalPos::new(6410.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx2),
            },
        })
        .expect("queue second enter");

    advance_tick(&mut app);

    let events_1 = drain_player_events(&mut rx1);
    let events_2 = drain_player_events(&mut rx2);

    let spawns_1: Vec<EntityId> = events_1
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntitySpawn { show, .. } => Some(show.entity_id),
            _ => None,
        })
        .collect();
    let spawns_2: Vec<EntityId> = events_2
        .iter()
        .filter_map(|event| match event {
            PlayerEvent::EntitySpawn { show, .. } => Some(show.entity_id),
            _ => None,
        })
        .collect();

    assert_eq!(
        spawns_1,
        vec![net_2],
        "player one should receive a single spawn for player two"
    );
    assert_eq!(
        spawns_2,
        vec![net_1],
        "player two should receive a single spawn for player one"
    );
}

#[test]
fn far_apart_players_do_not_spawn_each_other() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx1, mut rx1) = tokio::sync::mpsc::channel(32);
    let (tx2, mut rx2) = tokio::sync::mpsc::channel(32);

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(7_101),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx1),
            },
        })
        .expect("queue first enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(2),
                player_net_id: EntityId(7_102),
                initial_pos: LocalPos::new(6600.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx2),
            },
        })
        .expect("queue second enter");

    advance_tick(&mut app);

    assert!(
        first_spawn(&drain_player_events(&mut rx1)).is_none(),
        "player one should not receive a spawn for a distant player"
    );
    assert!(
        first_spawn(&drain_player_events(&mut rx2)).is_none(),
        "player two should not receive a spawn for a distant player"
    );
}

#[test]
fn player_movement_only_reaches_visible_observers() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx_alice, mut rx_alice) = tokio::sync::mpsc::channel(32);
    let (tx_bob, mut rx_bob) = tokio::sync::mpsc::channel(32);
    let (tx_cara, mut rx_cara) = tokio::sync::mpsc::channel(32);

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(7_201),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx_alice),
            },
        })
        .expect("alice enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(2),
                player_net_id: EntityId(7_202),
                initial_pos: LocalPos::new(6401.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx_bob),
            },
        })
        .expect("bob enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(3),
                player_net_id: EntityId(7_203),
                initial_pos: LocalPos::new(6600.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx_cara),
            },
        })
        .expect("cara enter");

    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_alice);
    let _ = drain_player_events(&mut rx_bob);
    let _ = drain_player_events(&mut rx_cara);

    let alice_entity = app.world().resource::<PlayerIndex>().0[&PlayerId::from(1)];
    {
        let mut ent = app.world_mut().entity_mut(alice_entity);
        let mut queue = ent.get_mut::<MoveIntentQueue>().expect("move queue exists");
        queue.0.push(MoveIntent {
            kind: MovementKind::Move,
            arg: 0,
            rot: 0,
            target: LocalPos::new(6405.0, 6400.0),
            ts: 1_000,
        });
    }

    advance_tick(&mut app);

    assert!(
        movement_events(&drain_player_events(&mut rx_alice)).is_empty(),
        "mover must not receive its own movement echo"
    );
    assert_eq!(
        movement_events(&drain_player_events(&mut rx_bob)),
        vec![(EntityId(7_201), MovementKind::Move)]
    );
    assert!(
        movement_events(&drain_player_events(&mut rx_cara)).is_empty(),
        "distant players must not receive movement updates"
    );
}

#[test]
fn same_tick_spawn_precedes_local_chat_for_new_observer() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx_alice, mut rx_alice) = tokio::sync::mpsc::channel(32);
    let (tx_bob, mut rx_bob) = tokio::sync::mpsc::channel(32);

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(2),
                player_net_id: EntityId(7_301),
                initial_pos: LocalPos::new(6505.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx_bob),
            },
        })
        .expect("bob enter");
    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_bob);

    let mut alice = PlayerAppearance::default();
    alice.name = "alice".to_string();

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(7_302),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: alice,
                outbox: PlayerOutbox::new(tx_alice),
            },
        })
        .expect("alice enter");
    inbound_tx
        .send(InboundEvent::ClientIntent {
            msg: ClientIntentMsg {
                player_id: PlayerId::from(1),
                intent: ClientIntent::Chat {
                    message: b"hello same tick\0".to_vec(),
                },
            },
        })
        .expect("queue chat");

    advance_tick(&mut app);

    let events_bob = drain_player_events(&mut rx_bob);
    assert!(
        matches!(events_bob.first(), Some(PlayerEvent::EntitySpawn { show, .. }) if show.entity_id == EntityId(7_302)),
        "spawn must be queued before local chat when visibility is created in the same tick"
    );
    assert!(
        matches!(events_bob.get(1), Some(PlayerEvent::Chat { kind, sender_entity_id, message, .. })
            if *kind == 0 && *sender_entity_id == Some(EntityId(7_302)) && message == b"alice : hello same tick\0"),
        "new observers should receive the same-tick local chat after the spawn"
    );
    assert_eq!(
        first_local_chat(&drain_player_events(&mut rx_alice)),
        Some((Some(EntityId(7_302)), b"alice : hello same tick\0".to_vec()))
    );
}

#[test]
fn same_tick_aoi_exit_sends_despawn_without_trailing_chat_or_movement() {
    let map_id = MapId::new(41);
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
        OutboxPlugin,
    ));
    app.update();

    let (tx_alice, mut rx_alice) = tokio::sync::mpsc::channel(32);
    let (tx_bob, mut rx_bob) = tokio::sync::mpsc::channel(32);

    let mut alice = PlayerAppearance::default();
    alice.name = "alice".to_string();

    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(7_401),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: alice,
                outbox: PlayerOutbox::new(tx_alice),
            },
        })
        .expect("alice enter");
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(2),
                player_net_id: EntityId(7_402),
                initial_pos: LocalPos::new(6505.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(tx_bob),
            },
        })
        .expect("bob enter");

    advance_tick(&mut app);
    let _ = drain_player_events(&mut rx_alice);
    let _ = drain_player_events(&mut rx_bob);

    let alice_entity = app.world().resource::<PlayerIndex>().0[&PlayerId::from(1)];
    {
        let mut ent = app.world_mut().entity_mut(alice_entity);
        {
            let mut move_queue = ent.get_mut::<MoveIntentQueue>().expect("move queue exists");
            move_queue.0.push(MoveIntent {
                kind: MovementKind::Move,
                arg: 0,
                rot: 0,
                target: LocalPos::new(6380.0, 6400.0),
                ts: 1_000,
            });
        }
        {
            let mut chat_queue = ent.get_mut::<ChatIntentQueue>().expect("chat queue exists");
            chat_queue.0.push(ChatIntent {
                message: b"bye\0".to_vec(),
            });
        }
    }

    advance_tick(&mut app);

    let events_bob = drain_player_events(&mut rx_bob);
    assert!(
        matches!(events_bob.as_slice(), [PlayerEvent::EntityDespawn { entity_id }] if *entity_id == EntityId(7_401)),
        "former observers should only receive the despawn when the subject exits AOI in the same tick"
    );
    let events_alice = drain_player_events(&mut rx_alice);
    assert_eq!(
        first_local_chat(&events_alice),
        Some((Some(EntityId(7_401)), b"alice : bye\0".to_vec()))
    );
    assert!(
        movement_events(&events_alice).is_empty(),
        "mover must not receive its own movement echo while leaving AOI"
    );
}

#[test]
fn fixed_timestep_switches_to_idle_rate_without_players() {
    let map_id = MapId::new(41);
    let (_runtime, inbound_rx) = MapEventSender::channel_pair(16);
    let (shared, map) = test_configs(MapInstanceKey::shared(1, map_id));

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();

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
    let map_key = MapInstanceKey::shared(1, map_id);
    let (inbound_tx, inbound_rx) = crossbeam_channel::bounded(16);
    let (shared, map) = test_configs(map_key);

    let mut app = bevy::prelude::App::new();
    app.add_plugins(bevy::prelude::MinimalPlugins);
    app.insert_resource(bevy::prelude::Time::<bevy::prelude::Fixed>::from_hz(25.0));
    app.add_plugins((
        ContentPlugin::new(shared, map),
        NetworkPlugin::new(inbound_rx),
        MapPlugin,
        SimulationPlugin,
    ));
    app.update();
    assert_eq!(
        app.world()
            .resource::<bevy::prelude::Time<bevy::prelude::Fixed>>()
            .timestep(),
        Duration::from_secs(1)
    );

    let (map_tx, _map_rx) = tokio::sync::mpsc::channel(8);
    inbound_tx
        .send(InboundEvent::PlayerEnter {
            msg: EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(5001),
                initial_pos: LocalPos::new(6400.0, 6400.0),
                appearance: PlayerAppearance::default(),
                outbox: PlayerOutbox::new(map_tx),
            },
        })
        .expect("send enter event");

    advance_tick(&mut app);
    assert_eq!(
        app.world()
            .resource::<bevy::prelude::Time<bevy::prelude::Fixed>>()
            .timestep(),
        Duration::from_millis(40)
    );
}
