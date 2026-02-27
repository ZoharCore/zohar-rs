use bevy::prelude::*;
use rand::RngExt;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::MovementKind;

use super::state::{
    LocalTransform, MapPendingMovements, MapSpatial, MobRef, NetEntityId, NetEntityIndex,
    PendingMovement, RuntimeState, SharedConfig, WanderState,
};
use super::util::{
    calculate_mob_move_duration_ms, packet_time_ms, random_idle_decision_delay,
    random_post_move_delay, rotation_from_delta,
};

pub(super) fn monster_wander(world: &mut World) {
    let shared = world.resource::<SharedConfig>().clone();
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let (now_ms, now_ts) = {
        let state = world.resource::<RuntimeState>();
        (state.sim_time_ms, packet_time_ms(state.packet_time_start))
    };

    let mob_entities: Vec<zohar_domain::entity::EntityId> = {
        let mut all_mobs = world.query::<(&MobRef, &NetEntityId)>();
        all_mobs
            .iter(world)
            .map(|(_, net_entity_id)| net_entity_id.net_id)
            .collect()
    };

    let mut dirty_map = false;

    for mob_net_id in mob_entities {
        let mob_entity = {
            let net_index = world.resource::<NetEntityIndex>();
            net_index.0.get(&mob_net_id).copied()
        };
        let Some(mob_entity) = mob_entity else {
            continue;
        };

        let (mob_id, old_pos, current_rot, mut wander) = {
            let mob_ref = world.entity(mob_entity).get::<MobRef>();
            let transform = world.entity(mob_entity).get::<LocalTransform>();
            let wander_state = world.entity(mob_entity).get::<WanderState>();
            let (Some(mob_ref), Some(transform), Some(wander_state)) =
                (mob_ref, transform, wander_state)
            else {
                continue;
            };
            (mob_ref.mob_id, transform.pos, transform.rot, wander_state.0)
        };

        if let Some(wait_at_ms) = wander.pending_wait_at_ms {
            if now_ms >= wait_at_ms {
                wander.pending_wait_at_ms = None;
                if let Some(mut ws) = world.entity_mut(mob_entity).get_mut::<WanderState>() {
                    ws.0 = wander;
                }
            }
            continue;
        }

        if now_ms < wander.next_decision_at_ms {
            continue;
        }

        let should_wander = {
            let mut state = world.resource_mut::<RuntimeState>();
            let chance_denom = shared.monster_wander.wander_chance_denominator.max(1);
            state.rng.random_range(0..chance_denom) == 0
        };

        if !should_wander {
            let mut state = world.resource_mut::<RuntimeState>();
            wander.next_decision_at_ms = now_ms.saturating_add(random_idle_decision_delay(
                &mut state.rng,
                &shared.monster_wander,
            ));
            if let Some(mut ws) = world.entity_mut(mob_entity).get_mut::<WanderState>() {
                ws.0 = wander;
            }
            continue;
        }

        let (new_pos, rot) = {
            let mut state = world.resource_mut::<RuntimeState>();
            let heading_rad = state.rng.random_range(0.0..std::f32::consts::TAU);
            let distance = state.rng.random_range(
                shared
                    .monster_wander
                    .step_min_m
                    .min(shared.monster_wander.step_max_m)
                    ..=shared
                        .monster_wander
                        .step_min_m
                        .max(shared.monster_wander.step_max_m),
            );
            let new_pos = LocalPos::new(
                old_pos.x + heading_rad.cos() * distance,
                old_pos.y + heading_rad.sin() * distance,
            );
            let rot = rotation_from_delta(old_pos, new_pos, current_rot);
            (new_pos, rot)
        };

        let Some(proto) = shared.mobs.get(&mob_id) else {
            continue;
        };

        let duration = calculate_mob_move_duration_ms(
            &shared.motion_speeds,
            mob_id,
            proto.move_speed,
            old_pos,
            new_pos,
        );

        if duration == 0 {
            let mut state = world.resource_mut::<RuntimeState>();
            wander.next_decision_at_ms = now_ms.saturating_add(random_idle_decision_delay(
                &mut state.rng,
                &shared.monster_wander,
            ));
            if let Some(mut ws) = world.entity_mut(mob_entity).get_mut::<WanderState>() {
                ws.0 = wander;
            }
            continue;
        }

        let (wait_at_ms, next_decision_at_ms) = {
            let mut state = world.resource_mut::<RuntimeState>();
            let wait_at = now_ms.saturating_add(duration as u64);
            let next_decision = wait_at.saturating_add(random_post_move_delay(
                &mut state.rng,
                &shared.monster_wander,
            ));
            (wait_at, next_decision)
        };

        wander.pending_wait_at_ms = Some(wait_at_ms);
        wander.next_decision_at_ms = next_decision_at_ms;

        if let Some(mut spatial) = world.entity_mut(map_entity).get_mut::<MapSpatial>() {
            spatial.0.update_position(mob_net_id, new_pos);
        }
        if let Some(mut transform) = world.entity_mut(mob_entity).get_mut::<LocalTransform>() {
            transform.pos = new_pos;
            transform.rot = rot;
        }
        if let Some(mut ws) = world.entity_mut(mob_entity).get_mut::<WanderState>() {
            ws.0 = wander;
        }
        if let Some(mut pending) = world
            .entity_mut(map_entity)
            .get_mut::<MapPendingMovements>()
        {
            pending.0.push(PendingMovement {
                mover_player_id: None,
                entity_id: mob_net_id,
                new_pos,
                kind: MovementKind::Wait,
                arg: 0,
                rot,
                ts: now_ts,
                duration,
            });
        }
        dirty_map = true;
    }

    if dirty_map {
        world.resource_mut::<RuntimeState>().is_dirty = true;
    }
}
