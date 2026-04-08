use bevy::prelude::*;
use std::collections::HashSet;
use zohar_domain::Empire;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::MovementKind;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementAnimation};
use zohar_map_port::{MovementEvent, PlayerEvent};

use crate::replication::{InterestConfig, VisibilityDiff};
use tracing::warn;

use super::state::{
    LocalTransform, MapPendingLocalChats, MapPendingMovementAnimations, MapPendingMovements,
    MapReplication, MapSpatial, NetEntityId, NetEntityIndex, PendingLocalChat, PendingMovement,
    PendingMovementAnimation, PlayerAppearanceComp, PlayerCount, PlayerIndex, PlayerMarker,
    PlayerMovementAnimation, PlayerOutboxComp, RuntimeState, SharedConfig,
};
use super::util::{
    format_talking_message, movement_kind_priority, obfuscate_cross_empire_talking_body,
    resolve_cross_empire_preserve_pct,
};
use crate::runtime::spawn_events::make_entity_spawn_payload;

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
    movement_animations: Vec<PendingMovementAnimation>,
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
        let movement_animations = {
            let mut map_ent = world.entity_mut(map_entity);
            map_ent
                .get_mut::<MapPendingMovementAnimations>()
                .map(|mut animations| std::mem::take(&mut animations.0))
                .unwrap_or_default()
        };

        Some(Self {
            movements,
            movement_animations,
            local_chats,
        })
    }
}

pub(crate) fn aoi_reconcile(world: &mut World) {
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

    maintain_spatial(world, map_entity);

    let interest_config = InterestConfig::default();
    for observer in collect_visibility_observers(world) {
        reconcile_one_observer(world, &shared, map_entity, observer, interest_config);
    }

    world.resource_mut::<RuntimeState>().is_dirty = false;
}

pub(crate) fn bootstrap_observer_snapshot(
    world: &mut World,
    observer_player_id: PlayerId,
    observer_net_id: EntityId,
    observer_pos: LocalPos,
) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let shared = world.resource::<SharedConfig>().clone();
    maintain_spatial(world, map_entity);
    reconcile_one_observer(
        world,
        &shared,
        map_entity,
        VisibilityObserver {
            player_id: observer_player_id,
            net_id: observer_net_id,
            pos: observer_pos,
        },
        InterestConfig::default(),
    );
}

fn maintain_spatial(world: &mut World, map_entity: Entity) {
    if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
        spatial.0.maintain();
    }
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

fn reconcile_one_observer(
    world: &mut World,
    shared: &SharedConfig,
    map_entity: Entity,
    observer: VisibilityObserver,
    interest_config: InterestConfig,
) {
    let Some(spawn_candidates) = visibility_candidates(
        world,
        map_entity,
        observer.pos,
        interest_config.spawn_radius,
        observer.net_id,
    ) else {
        return;
    };
    let Some(retain_candidates) = visibility_candidates(
        world,
        map_entity,
        observer.pos,
        interest_config.despawn_radius,
        observer.net_id,
    ) else {
        return;
    };

    let diff = reconcile_visibility(
        world,
        map_entity,
        observer,
        &spawn_candidates,
        &retain_candidates,
    );
    queue_visibility_diff(world, shared, map_entity, observer, diff);
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
    map_entity: Entity,
    observer: VisibilityObserver,
    diff: VisibilityDiff,
) {
    let observer_entity = {
        let player_index = world.resource::<PlayerIndex>();
        player_index.0.get(&observer.player_id).copied()
    };
    let Some(observer_entity) = observer_entity else {
        rollback_entered_visibility(world, map_entity, observer.net_id, &diff.entered);
        return;
    };

    let mut entered_payloads = Vec::with_capacity(diff.entered.len());
    let mut failed_targets = Vec::new();
    for target_id in diff.entered {
        if let Some((show, details)) = make_entity_spawn_payload(world, shared, target_id) {
            entered_payloads.push((show, details));
        } else {
            failed_targets.push(target_id);
        }
    }

    rollback_entered_visibility(world, map_entity, observer.net_id, &failed_targets);

    let entered_payloads: Vec<_> = entered_payloads
        .into_iter()
        .map(|(show, details)| {
            let movement_animation = world
                .resource::<NetEntityIndex>()
                .0
                .get(&show.entity_id)
                .copied()
                .and_then(|target_entity| {
                    world
                        .entity(target_entity)
                        .get::<PlayerMovementAnimation>()
                        .map(|animation| animation.0)
                });
            (show, details, movement_animation)
        })
        .collect();

    let mut observer_ent = world.entity_mut(observer_entity);
    let Some(mut observer_outbox) = observer_ent.get_mut::<PlayerOutboxComp>() else {
        let unsent_targets: Vec<_> = entered_payloads
            .iter()
            .map(|(show, _, _)| show.entity_id)
            .collect();
        drop(observer_ent);
        rollback_entered_visibility(world, map_entity, observer.net_id, &unsent_targets);
        return;
    };

    for (show, details, movement_animation) in entered_payloads {
        let entity_id = show.entity_id;
        observer_outbox
            .0
            .push_reliable(PlayerEvent::EntitySpawn { show, details });
        if movement_animation == Some(MovementAnimation::Walk) {
            observer_outbox
                .0
                .push_reliable(PlayerEvent::SetEntityMovementAnimation {
                    entity_id,
                    animation: MovementAnimation::Walk,
                });
        }
    }
    for target_id in diff.left {
        observer_outbox.0.push_reliable(PlayerEvent::EntityDespawn {
            entity_id: target_id,
        });
    }
}

fn rollback_entered_visibility(
    world: &mut World,
    map_entity: Entity,
    observer_net_id: EntityId,
    target_ids: &[EntityId],
) {
    if target_ids.is_empty() {
        return;
    }

    let mut map_ent = world.entity_mut(map_entity);
    let Some(mut replication) = map_ent.get_mut::<MapReplication>() else {
        return;
    };

    for &target_id in target_ids {
        if replication.0.remove_visibility(observer_net_id, target_id) {
            warn!(
                observer = ?observer_net_id,
                target = ?target_id,
                "Rolled back visibility edge after spawn payload could not be queued"
            );
        }
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

pub(crate) fn replication_flush(world: &mut World) {
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
    flush_pending_movement_animations(world, map_entity, pending.movement_animations);
    flush_pending_local_chats(world, map_entity, pending.local_chats);
}

fn flush_pending_movements(
    world: &mut World,
    map_entity: Entity,
    pending_movements: Vec<PendingMovement>,
) {
    for movement in pending_movements {
        let include_subject =
            movement.mover_player_id.is_some() && matches!(movement.kind, MovementKind::Attack);
        let recipients =
            observer_recipients(world, map_entity, movement.entity_id, include_subject);
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
    let movement_event = MovementEvent {
        entity_id: movement.entity_id,
        kind: movement.kind,
        arg: movement.arg,
        facing: movement.rot,
        position: movement.new_pos,
        client_ts: movement.ts,
        duration: movement.duration,
    };

    if movement.reliable {
        outbox
            .0
            .push_reliable(PlayerEvent::EntityMove(movement_event));
    } else if movement.mover_player_id.is_some()
        && matches!(movement.kind, MovementKind::Move | MovementKind::Wait)
    {
        outbox
            .0
            .set_latest_movement_with_priority(movement_event, true);
    } else {
        outbox.0.push_remote_movement(movement_event);
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

fn flush_pending_movement_animations(
    world: &mut World,
    map_entity: Entity,
    pending_animations: Vec<PendingMovementAnimation>,
) {
    for pending in pending_animations {
        let recipients = observer_recipients(world, map_entity, pending.entity_id, true);
        for recipient in recipients {
            let mut recipient_ent = world.entity_mut(recipient.entity);
            let Some(mut outbox) = recipient_ent.get_mut::<PlayerOutboxComp>() else {
                continue;
            };
            outbox
                .0
                .push_reliable(PlayerEvent::SetEntityMovementAnimation {
                    entity_id: pending.entity_id,
                    animation: pending.animation,
                });
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
        // TODO: only broadcast local speaking packets
        channel: pending_chat.channel,
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
