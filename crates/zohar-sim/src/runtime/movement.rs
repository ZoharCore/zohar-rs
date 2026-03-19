use super::players::player_entities_on_map;
use super::state::{
    ChatIntentQueue, LocalTransform, MapConfig, MapPendingLocalChats, MapPendingMovements,
    MapSpatial, MoveIntent, MoveIntentQueue, NetEntityId, PendingLocalChat, PendingMovement,
    PlayerAppearanceComp, PlayerMarker, PlayerMotion, RuntimeState, SharedConfig,
};
use super::util::{calculate_move_duration_ms, sample_player_motion_at, sanitize_packet_target};
use bevy::prelude::*;
use tracing::debug;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::PlayerId;
use zohar_domain::entity::{EntityId, MovementKind};

pub(super) fn process_intents(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let map_size = world.resource::<MapConfig>().local_size;
    let player_entities = player_entities_on_map(world);

    for player_entity in player_entities {
        if !world.entities().contains(player_entity) {
            continue;
        }

        let (player_id, appearance_empire, player_name, mover_net_id) = {
            let e = world.entity(player_entity);
            let Some(player) = e.get::<PlayerMarker>() else {
                continue;
            };
            let Some(appearance) = e.get::<PlayerAppearanceComp>() else {
                continue;
            };
            let Some(net_id) = e.get::<NetEntityId>() else {
                continue;
            };
            (
                player.player_id,
                appearance.0.empire,
                appearance.0.name.clone(),
                net_id.net_id,
            )
        };

        let move_intents = {
            let mut ent = world.entity_mut(player_entity);
            let Some(mut move_queue) = ent.get_mut::<MoveIntentQueue>() else {
                continue;
            };
            std::mem::take(&mut move_queue.0)
        };
        let chat_intents = {
            let mut ent = world.entity_mut(player_entity);
            let Some(mut chat_queue) = ent.get_mut::<ChatIntentQueue>() else {
                continue;
            };
            std::mem::take(&mut chat_queue.0)
        };

        for intent in move_intents {
            apply_move_intent(
                world,
                &shared,
                map_size,
                player_entity,
                player_id,
                mover_net_id,
                intent,
            );
        }

        enqueue_local_chat_intents(
            world,
            player_id,
            mover_net_id,
            appearance_empire,
            &player_name,
            chat_intents,
        );
    }
}

fn enqueue_local_chat_intents(
    world: &mut World,
    speaker_player_id: PlayerId,
    speaker_entity_id: EntityId,
    speaker_empire: zohar_domain::Empire,
    speaker_name: &str,
    chat_intents: Vec<super::state::ChatIntent>,
) {
    if chat_intents.is_empty() {
        return;
    }

    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let mut map_ent = world.entity_mut(map_entity);
    let Some(mut pending_chats) = map_ent.get_mut::<MapPendingLocalChats>() else {
        return;
    };
    let speaker_name = speaker_name.to_string();

    for chat in chat_intents {
        pending_chats.0.push(PendingLocalChat {
            speaker_player_id,
            speaker_entity_id,
            speaker_empire,
            speaker_name: speaker_name.clone(),
            message: chat.message,
        });
    }
}

fn apply_move_intent(
    world: &mut World,
    shared: &SharedConfig,
    map_size: zohar_domain::coords::LocalSize,
    player_entity: Entity,
    player_id: PlayerId,
    mover_net_id: EntityId,
    intent: MoveIntent,
) {
    let mut player_query = world.query::<(
        &mut LocalTransform,
        &mut PlayerMotion,
        &PlayerAppearanceComp,
    )>();
    let (_old_pos, new_pos, duration) = {
        let Ok((mut transform, mut motion, appearance)) =
            player_query.get_mut(world, player_entity)
        else {
            return;
        };

        let old_pos = sample_player_motion_at(transform.pos, &mut motion.0, intent.ts);
        let requested_pos = sanitize_packet_target(old_pos, intent.target);
        let new_pos = sanitize_player_target_to_map(requested_pos, map_size);
        transform.pos = new_pos;
        transform.rot = intent.rot;

        let duration = if intent.kind == MovementKind::Move {
            calculate_move_duration_ms(&shared.motion_speeds, &appearance.0, old_pos, new_pos)
        } else {
            0
        };

        if intent.kind == MovementKind::Move && duration > 0 {
            motion.0.segment_start_pos = old_pos;
            motion.0.segment_end_pos = new_pos;
            motion.0.segment_start_ts = intent.ts;
            motion.0.segment_end_ts = intent.ts.saturating_add(duration);
        } else {
            motion.0.segment_start_pos = new_pos;
            motion.0.segment_end_pos = new_pos;
            motion.0.segment_start_ts = intent.ts;
            motion.0.segment_end_ts = intent.ts;
        }
        motion.0.last_client_ts = intent.ts;

        debug!(
            player = ?mover_net_id,
            kind = ?intent.kind,
            requested_pos = ?intent.target,
            old_pos = ?old_pos,
            new_pos = ?new_pos,
            rot = intent.rot,
            ts = intent.ts,
            duration,
            "Applied player movement intent"
        );

        (old_pos, new_pos, duration)
    };

    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
        spatial.0.update_position(mover_net_id, new_pos);
    }

    if let Some(mut pending) = world
        .entity_mut(map_entity)
        .get_mut::<MapPendingMovements>()
    {
        pending.0.push(PendingMovement {
            mover_player_id: Some(player_id),
            entity_id: mover_net_id,
            new_pos,
            kind: intent.kind,
            reliable: false,
            arg: intent.arg,
            rot: intent.rot,
            ts: intent.ts,
            duration,
        });
    }

    world.resource_mut::<RuntimeState>().is_dirty = true;
}

fn sanitize_player_target_to_map(
    requested: LocalPos,
    map_size: zohar_domain::coords::LocalSize,
) -> LocalPos {
    LocalPos::new(
        clamp_axis_to_map(requested.x, map_size.width),
        clamp_axis_to_map(requested.y, map_size.height),
    )
}

fn clamp_axis_to_map(coord: f32, max_exclusive: f32) -> f32 {
    if !coord.is_finite() {
        return 0.0;
    }
    if !max_exclusive.is_finite() || max_exclusive <= 0.0 {
        return 0.0;
    }
    coord.clamp(0.0, max_exclusive - 0.001)
}
