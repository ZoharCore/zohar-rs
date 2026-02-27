use bevy::prelude::*;
use std::collections::HashSet;
use zohar_domain::Empire;
use zohar_domain::MobKind;
use zohar_domain::appearance::{EntityDetails, EntityKind, PlayerAppearance, ShowEntity};
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::MovementKind;
use zohar_domain::entity::player::PlayerId;

use crate::api::PlayerEvent;
use crate::replication::{InterestConfig, VisibilityDiff};

use super::state::{
    LocalTransform, MapEmpire, MapPendingLocalChats, MapPendingMovements, MapReplication,
    MapSpatial, MobRef, NetEntityId, NetEntityIndex, PendingLocalChat, PendingMovement,
    PlayerAppearanceComp, PlayerCount, PlayerIndex, PlayerMarker, PlayerOutboxComp, RuntimeState,
    SharedConfig,
};
use super::util::{
    format_talking_message, movement_kind_priority, obfuscate_cross_empire_talking_body,
    resolve_cross_empire_preserve_pct,
};

#[derive(Clone, Copy)]
struct ObserverRecipient {
    entity: Entity,
    empire: Empire,
}

#[derive(Clone, Copy)]
struct VisibilityObserver {
    player_id: PlayerId,
    net_id: EntityId,
    pos: LocalPos,
}

#[derive(Default)]
struct PendingReplicationFlush {
    movements: Vec<PendingMovement>,
    local_chats: Vec<PendingLocalChat>,
}

impl PendingReplicationFlush {
    fn drain(world: &mut World, map_entity: Entity) -> Option<Self> {
        let movements = {
            let mut map_ent = world.entity_mut(map_entity);
            let mut pending_movements = map_ent.get_mut::<MapPendingMovements>()?;
            std::mem::take(&mut pending_movements.0)
        };
        let local_chats = {
            let mut map_ent = world.entity_mut(map_entity);
            map_ent
                .get_mut::<MapPendingLocalChats>()
                .map(|mut chats| std::mem::take(&mut chats.0))
                .unwrap_or_default()
        };

        Some(Self {
            movements,
            local_chats,
        })
    }
}

pub(super) fn aoi_reconcile(world: &mut World) {
    if !world.resource::<RuntimeState>().is_dirty {
        return;
    }
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    if world.resource::<PlayerCount>().0 == 0 {
        world.resource_mut::<RuntimeState>().is_dirty = false;
        return;
    }

    {
        if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
            spatial.0.maintain();
        }
    }

    let interest_config = InterestConfig::default();
    for observer in collect_visibility_observers(world) {
        let Some(spawn_candidates) = visibility_candidates(
            world,
            map_entity,
            observer.pos,
            interest_config.spawn_radius,
            observer.net_id,
        ) else {
            continue;
        };
        let Some(retain_candidates) = visibility_candidates(
            world,
            map_entity,
            observer.pos,
            interest_config.despawn_radius,
            observer.net_id,
        ) else {
            continue;
        };

        let diff = reconcile_visibility(
            world,
            map_entity,
            observer,
            &spawn_candidates,
            &retain_candidates,
        );
        queue_visibility_diff(world, &shared, observer.player_id, diff);
    }

    world.resource_mut::<RuntimeState>().is_dirty = false;
}

fn collect_visibility_observers(world: &mut World) -> Vec<VisibilityObserver> {
    let mut query = world.query::<(&PlayerMarker, &NetEntityId, &LocalTransform)>();
    query
        .iter(world)
        .map(|(marker, net_id, transform)| VisibilityObserver {
            player_id: marker.player_id,
            net_id: net_id.net_id,
            pos: transform.pos,
        })
        .collect()
}

fn visibility_candidates(
    world: &World,
    map_entity: Entity,
    observer_pos: LocalPos,
    radius: f32,
    observer_net_id: EntityId,
) -> Option<HashSet<EntityId>> {
    let spatial = world.entity(map_entity).get::<MapSpatial>()?;
    Some(
        spatial
            .0
            .query_in_radius(observer_pos, radius)
            .filter(|entity_id| *entity_id != observer_net_id)
            .collect(),
    )
}

fn reconcile_visibility(
    world: &mut World,
    map_entity: Entity,
    observer: VisibilityObserver,
    spawn_candidates: &HashSet<EntityId>,
    retain_candidates: &HashSet<EntityId>,
) -> VisibilityDiff {
    let mut map_ent = world.entity_mut(map_entity);
    let mut replication = map_ent
        .get_mut::<MapReplication>()
        .expect("map replication must exist during AOI reconcile");
    replication
        .0
        .reconcile_observer(observer.net_id, spawn_candidates, retain_candidates)
}

fn queue_visibility_diff(
    world: &mut World,
    shared: &SharedConfig,
    observer_player_id: PlayerId,
    diff: VisibilityDiff,
) {
    let observer_entity = {
        let player_index = world.resource::<PlayerIndex>();
        player_index.0.get(&observer_player_id).copied()
    };
    let Some(observer_entity) = observer_entity else {
        return;
    };

    let mut entered_payloads = Vec::with_capacity(diff.entered.len());
    for target_id in diff.entered {
        if let Some((show, details)) = make_entity_spawn_payload(world, shared, target_id) {
            entered_payloads.push((show, details));
        }
    }

    let mut observer_ent = world.entity_mut(observer_entity);
    let Some(mut observer_outbox) = observer_ent.get_mut::<PlayerOutboxComp>() else {
        return;
    };

    for (show, details) in entered_payloads {
        observer_outbox
            .0
            .push_reliable(PlayerEvent::EntitySpawn { show, details });
    }
    for target_id in diff.left {
        observer_outbox.0.push_reliable(PlayerEvent::EntityDespawn {
            entity_id: target_id,
        });
    }
}

fn observer_recipients(
    world: &World,
    map_entity: Entity,
    subject_entity_id: EntityId,
    include_subject: bool,
) -> Vec<ObserverRecipient> {
    let mut recipient_ids = world
        .entity(map_entity)
        .get::<MapReplication>()
        .map(|replication| replication.0.observers_for(subject_entity_id))
        .unwrap_or_default();

    if include_subject {
        recipient_ids.push(subject_entity_id);
    }

    recipient_ids.sort_unstable_by_key(|entity_id| entity_id.0);
    recipient_ids.dedup();

    let net_index = world.resource::<NetEntityIndex>();
    recipient_ids
        .into_iter()
        .filter_map(|recipient_id| {
            let entity = net_index.0.get(&recipient_id).copied()?;
            let empire = world
                .entity(entity)
                .get::<PlayerAppearanceComp>()
                .map(|appearance| appearance.0.empire)?;
            Some(ObserverRecipient { entity, empire })
        })
        .collect()
}

pub(super) fn replication_flush(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let Some(mut pending) = PendingReplicationFlush::drain(world, map_entity) else {
        return;
    };

    pending.movements.sort_unstable_by_key(|movement| {
        (
            movement.entity_id.0,
            movement.ts,
            movement_kind_priority(movement.kind),
        )
    });

    flush_pending_movements(world, map_entity, pending.movements);
    flush_pending_local_chats(world, map_entity, pending.local_chats);
}

fn flush_pending_movements(
    world: &mut World,
    map_entity: Entity,
    pending_movements: Vec<PendingMovement>,
) {
    for movement in pending_movements {
        let recipients = observer_recipients(world, map_entity, movement.entity_id, false);
        for recipient in recipients {
            push_movement_event(world, recipient.entity, movement);
        }
    }
}

fn push_movement_event(world: &mut World, recipient_entity: Entity, movement: PendingMovement) {
    let mut recipient_ent = world.entity_mut(recipient_entity);
    let Some(mut outbox) = recipient_ent.get_mut::<PlayerOutboxComp>() else {
        return;
    };

    match movement.kind {
        MovementKind::Move | MovementKind::Wait => {
            outbox.0.set_latest_movement_with_priority(
                movement.entity_id,
                movement.kind,
                movement.arg,
                movement.rot,
                movement.new_pos.x,
                movement.new_pos.y,
                movement.ts,
                movement.duration,
                movement.mover_player_id.is_some(),
            );
        }
        MovementKind::Attack | MovementKind::Combo => {
            outbox.0.push_reliable(PlayerEvent::EntityMove {
                entity_id: movement.entity_id,
                kind: movement.kind,
                arg: movement.arg,
                rot: movement.rot,
                x: movement.new_pos.x,
                y: movement.new_pos.y,
                ts: movement.ts,
                duration: movement.duration,
            });
        }
    }
}

fn flush_pending_local_chats(
    world: &mut World,
    map_entity: Entity,
    pending_local_chats: Vec<PendingLocalChat>,
) {
    let preserve_pct = resolve_cross_empire_preserve_pct();
    for pending_chat in pending_local_chats {
        let recipients =
            observer_recipients(world, map_entity, pending_chat.speaker_entity_id, true);
        for recipient in recipients {
            push_local_chat_event(world, recipient, &pending_chat, preserve_pct);
        }
    }
}

fn push_local_chat_event(
    world: &mut World,
    recipient: ObserverRecipient,
    pending_chat: &PendingLocalChat,
    preserve_pct: u8,
) {
    let message = local_chat_message_for(world, pending_chat, recipient.empire, preserve_pct);
    let mut recipient_ent = world.entity_mut(recipient.entity);
    let Some(mut outbox) = recipient_ent.get_mut::<PlayerOutboxComp>() else {
        return;
    };

    outbox.0.push_reliable(PlayerEvent::Chat {
        kind: 0,
        sender_entity_id: Some(pending_chat.speaker_entity_id),
        empire: Some(pending_chat.speaker_empire),
        message,
    });
}

fn local_chat_message_for(
    world: &mut World,
    pending_chat: &PendingLocalChat,
    recipient_empire: Empire,
    preserve_pct: u8,
) -> Vec<u8> {
    let mut message = format_talking_message(&pending_chat.speaker_name, &pending_chat.message);
    let message_body_start = pending_chat.speaker_name.len() + 3;
    let message_body_end = message.len().saturating_sub(1);

    if recipient_empire != pending_chat.speaker_empire && message_body_start < message_body_end {
        let mut state = world.resource_mut::<RuntimeState>();
        obfuscate_cross_empire_talking_body(
            &mut state.rng,
            pending_chat.speaker_empire,
            &mut message[message_body_start..message_body_end],
            preserve_pct,
        );
    }

    message
}

pub(super) fn make_entity_spawn_payload(
    world: &World,
    shared: &SharedConfig,
    target_id: EntityId,
) -> Option<(ShowEntity, Option<EntityDetails>)> {
    let target_entity = world
        .resource::<NetEntityIndex>()
        .0
        .get(&target_id)
        .copied()?;
    let target_ref = world.entity(target_entity);

    if let (Some(_player), Some(net_id), Some(transform), Some(appearance)) = (
        target_ref.get::<PlayerMarker>(),
        target_ref.get::<NetEntityId>(),
        target_ref.get::<LocalTransform>(),
        target_ref.get::<super::state::PlayerAppearanceComp>(),
    ) {
        return Some((
            make_player_show(net_id.net_id, transform.pos, &appearance.0, transform.rot),
            Some(make_player_details(net_id.net_id, &appearance.0)),
        ));
    }

    let mob_ref = target_ref.get::<MobRef>()?;
    let transform = target_ref.get::<LocalTransform>()?;
    let net_id = target_ref.get::<NetEntityId>()?;
    let proto = shared.mobs.get(&mob_ref.mob_id)?;

    let show = ShowEntity {
        entity_id: net_id.net_id,
        angle: transform.rot as f32 * 5.0,
        pos: transform.pos,
        kind: EntityKind::Mob {
            mob_id: mob_ref.mob_id,
            mob_kind: proto.mob_kind,
        },
        move_speed: proto.move_speed,
        attack_speed: proto.attack_speed,
        state_flags: 0,
        buff_flags: 0,
    };

    let details = if proto.mob_kind == MobKind::Npc {
        let map_empire = world
            .resource::<RuntimeState>()
            .map_entity
            .and_then(|map_entity| world.entity(map_entity).get::<MapEmpire>())
            .and_then(|emp| emp.0);

        Some(EntityDetails {
            entity_id: net_id.net_id,
            name: format!("[{}] {}", mob_ref.mob_id.get(), proto.name),
            body_part: 0,
            wep_part: 0,
            hair_part: 0,
            empire: proto.empire.or(map_empire),
            guild_id: 0,
            level: 0,
            rank_pts: 0,
            pvp_mode: 0,
            mount_id: 0,
        })
    } else {
        None
    };

    Some((show, details))
}

fn make_player_show(
    net_id: EntityId,
    pos: LocalPos,
    appearance: &PlayerAppearance,
    rot: u8,
) -> ShowEntity {
    ShowEntity {
        entity_id: net_id,
        angle: rot as f32 * 5.0,
        pos,
        kind: EntityKind::Player {
            class: appearance.class,
            gender: appearance.gender,
        },
        move_speed: appearance.move_speed,
        attack_speed: appearance.attack_speed,
        state_flags: 0,
        buff_flags: 0,
    }
}

fn make_player_details(net_id: EntityId, appearance: &PlayerAppearance) -> EntityDetails {
    EntityDetails {
        entity_id: net_id,
        name: appearance.name.clone(),
        body_part: appearance.body_part,
        wep_part: 0,
        hair_part: 0,
        empire: Some(appearance.empire),
        guild_id: appearance.guild_id,
        level: appearance.level,
        rank_pts: 0,
        pvp_mode: 0,
        mount_id: 0,
    }
}
