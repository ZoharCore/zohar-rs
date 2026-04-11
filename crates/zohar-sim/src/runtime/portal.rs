use bevy::prelude::*;
use std::collections::HashMap;
use zohar_domain::coords::{LocalPos, WorldPos};
use zohar_domain::entity::EntityId;
use zohar_domain::entity::mob::{MobKind, PortalBehavior};
use zohar_map_port::{ClientTimestamp, PlayerEvent, PortalDestination};

use super::state::{
    LocalTransform, MAX_MOVE_PACKET_STEP_M, MapSpatial, NetEntityIndex, PlayerMarker, PlayerMotion,
    PlayerOutboxComp, PortalPollState, RuntimeState, SharedConfig, SimDuration,
};
use crate::runtime::mob::MobRef;
use crate::runtime::util::sample_player_visual_position_at;

const PORTAL_OVERLAP_RADIUS_M: f32 = 3.0;
const PORTAL_POLL_INTERVAL: SimDuration = SimDuration::from_millis(200);

pub(crate) fn process_portal_entries(world: &mut World) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };

    let sim_now = world.resource::<RuntimeState>().sim_now;
    let packet_now = world.resource::<RuntimeState>().packet_now();

    let previous_overlaps = {
        let mut poll_state = world.resource_mut::<PortalPollState>();
        if sim_now < poll_state.next_poll_at {
            return;
        }

        poll_state.next_poll_at = sim_now.saturating_add(PORTAL_POLL_INTERVAL);
        std::mem::take(&mut poll_state.overlaps)
    };

    let current_overlaps = {
        let portals = collect_portals(world);
        collect_current_player_portal_overlaps(world, map_entity, &portals, packet_now)
    };

    // trigger on rising edge only
    let new_overlaps = current_overlaps
        .iter()
        .filter(|(player, overlap)| previous_overlaps.get(player) != Some(&overlap.portal_entity));

    for (player, overlap) in new_overlaps {
        if let Some(mut outbox) = world.entity_mut(*player).get_mut::<PlayerOutboxComp>() {
            outbox.0.push_reliable(PlayerEvent::PortalEntered {
                destination: overlap.destination,
            });
        }
    }

    world.resource_mut::<PortalPollState>().overlaps = current_overlaps
        .into_iter()
        .map(|(player_entity, overlap)| (player_entity, overlap.portal_entity))
        .collect();
}

#[derive(Clone)]
struct PortalEntry {
    entity: Entity,
    pos: LocalPos,
    destination: PortalDestination,
}

#[derive(Clone)]
struct PortalOverlap {
    portal_entity: Entity,
    destination: PortalDestination,
}

fn collect_portals(world: &mut World) -> Vec<PortalEntry> {
    let mobs = world.resource::<SharedConfig>().mobs.clone();
    let mut query = world.query::<(Entity, &LocalTransform, &MobRef)>();
    query
        .iter(world)
        .filter_map(|(portal_entity, transform, mob_ref)| {
            let proto = mobs.get(&mob_ref.mob_id)?;
            let MobKind::Portal(portal_behavior) = proto.mob_kind else {
                return None;
            };
            Some(PortalEntry {
                entity: portal_entity,
                pos: transform.pos,
                destination: parse_portal_destination(portal_behavior, &proto.name)?,
            })
        })
        .collect()
}

fn collect_current_player_portal_overlaps(
    world: &World,
    map_entity: Entity,
    portals: &[PortalEntry],
    packet_now: ClientTimestamp,
) -> HashMap<Entity, PortalOverlap> {
    let Some(spatial) = world.entity(map_entity).get::<MapSpatial>() else {
        return HashMap::new();
    };

    let mut best_by_player: HashMap<Entity, (PortalOverlap, f32)> = HashMap::new();

    for portal in portals {
        for entity_id in spatial
            .0
            .query_in_radius(portal.pos, MAX_MOVE_PACKET_STEP_M + PORTAL_OVERLAP_RADIUS_M)
        {
            let Some(player_entity) = resolve_player_entity(world, entity_id) else {
                continue;
            };
            let Some(player_pos) = sample_player_position(world, player_entity, packet_now) else {
                continue;
            };

            let dist_sq = distance_sq(player_pos, portal.pos);
            if dist_sq > PORTAL_OVERLAP_RADIUS_M * PORTAL_OVERLAP_RADIUS_M {
                continue;
            }

            let candidate = (
                PortalOverlap {
                    portal_entity: portal.entity,
                    destination: portal.destination,
                },
                dist_sq,
            );
            if candidate_is_nearer(best_by_player.get(&player_entity), &candidate) {
                best_by_player.insert(player_entity, candidate);
            }
        }
    }

    best_by_player
        .into_iter()
        .map(|(player_entity, (overlap, _dist))| (player_entity, overlap))
        .collect()
}

fn resolve_player_entity(world: &World, entity_id: EntityId) -> Option<Entity> {
    let entity = world
        .resource::<NetEntityIndex>()
        .0
        .get(&entity_id)
        .copied()?;
    world.entity(entity).get::<PlayerMarker>()?;
    Some(entity)
}

fn sample_player_position(
    world: &World,
    player_entity: Entity,
    packet_now: ClientTimestamp,
) -> Option<LocalPos> {
    let transform = world.entity(player_entity).get::<LocalTransform>()?;
    let motion = world
        .entity(player_entity)
        .get::<PlayerMotion>()
        .map(|motion| motion.0);

    Some(
        motion
            .map(|motion| sample_player_visual_position_at(motion, packet_now))
            .unwrap_or(transform.pos),
    )
}

fn distance_sq(a: LocalPos, b: LocalPos) -> f32 {
    let delta = a - b;
    delta.dot(delta)
}

fn candidate_is_nearer(
    current: Option<&(PortalOverlap, f32)>,
    candidate: &(PortalOverlap, f32),
) -> bool {
    let Some((current_overlap, current_dist)) = current else {
        return true;
    };

    candidate.1.total_cmp(current_dist).is_lt()
        || (candidate.1 == *current_dist
            && candidate.0.portal_entity.index() < current_overlap.portal_entity.index())
}

fn parse_portal_destination(behavior: PortalBehavior, name: &str) -> Option<PortalDestination> {
    let mut parts = name.split_whitespace().rev();
    let y = parts.next()?.parse::<f32>().ok()?;
    let x = parts.next()?.parse::<f32>().ok()?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }

    Some(match behavior {
        PortalBehavior::MapTransfer => PortalDestination::MapTransfer {
            world_pos: WorldPos::new(x, y),
        },
        PortalBehavior::LocalReposition => PortalDestination::LocalReposition {
            local_pos: LocalPos::new(x, y),
        },
    })
}
