use bevy::prelude::*;
use zohar_domain::entity::EntityId;
use zohar_gameplay::combat::HitFlags;
use zohar_gameplay::stats::game::Stat;
use zohar_map_port::{DamageInfoFlags, PlayerEvent};

use super::state::{
    MapReplication, MobStatsComp, NetEntityId, NetEntityIndex, PlayerMarker, PlayerOutboxComp,
    PlayerStatsComp, PlayerTargetComp, RuntimeState,
};

pub(crate) fn select_target(
    world: &mut World,
    map_entity: Entity,
    player_entity: Entity,
    player_net_id: EntityId,
    target_id: EntityId,
) {
    if target_id == EntityId(0)
        || !target_is_selectable(world, map_entity, player_net_id, target_id)
    {
        set_selected_target(world, player_entity, None);
        push_health_bar(world, player_entity, EntityId(0), 0);
        return;
    }

    set_selected_target(world, player_entity, Some(target_id));
    if let Some(hp_pct) = health_bar_percent_for_entity_id(world, target_id) {
        push_health_bar(world, player_entity, target_id, hp_pct);
    }
}

pub(crate) fn broadcast_entity_health_bar_to_targeters(world: &mut World, target_id: EntityId) {
    let Some(map_entity) = world.resource::<RuntimeState>().map_entity else {
        return;
    };
    let Some(hp_pct) = health_bar_percent_for_entity_id(world, target_id) else {
        return;
    };

    let mut query = world.query::<(Entity, &NetEntityId, &PlayerTargetComp, &PlayerMarker)>();
    let candidates = query
        .iter(world)
        .filter_map(|(player_entity, net_id, selected, _)| {
            (selected.selected == Some(target_id)).then_some((player_entity, net_id.net_id))
        })
        .collect::<Vec<_>>();

    for (player_entity, player_net_id) in candidates {
        if !target_is_visible(world, map_entity, player_net_id, target_id) {
            continue;
        }
        push_health_bar(world, player_entity, target_id, hp_pct);
    }
}

pub(crate) fn send_damage_info_to_selected_target(
    world: &mut World,
    player_entity: Entity,
    target_id: EntityId,
    damage: i32,
    flags: HitFlags,
) {
    let selected = world
        .entity(player_entity)
        .get::<PlayerTargetComp>()
        .and_then(|target| target.selected);
    if selected == Some(target_id) {
        push_damage_info(world, player_entity, target_id, damage, flags);
    }
}

pub(crate) fn send_damage_info_to_player(
    world: &mut World,
    player_entity: Entity,
    target_id: EntityId,
    damage: i32,
    flags: HitFlags,
) {
    push_damage_info(world, player_entity, target_id, damage, flags);
}

fn target_is_selectable(
    world: &World,
    map_entity: Entity,
    player_net_id: EntityId,
    target_id: EntityId,
) -> bool {
    world
        .resource::<NetEntityIndex>()
        .0
        .contains_key(&target_id)
        && target_id != player_net_id
        && target_is_visible(world, map_entity, player_net_id, target_id)
}

fn target_is_visible(
    world: &World,
    map_entity: Entity,
    player_net_id: EntityId,
    target_id: EntityId,
) -> bool {
    world
        .entity(map_entity)
        .get::<MapReplication>()
        .is_some_and(|replication| replication.0.is_visible(player_net_id, target_id))
}

fn set_selected_target(world: &mut World, player_entity: Entity, selected: Option<EntityId>) {
    if let Some(mut target) = world
        .entity_mut(player_entity)
        .get_mut::<PlayerTargetComp>()
    {
        target.selected = selected;
    }
}

fn push_health_bar(world: &mut World, player_entity: Entity, entity_id: EntityId, hp_pct: u8) {
    if let Some(mut outbox) = world
        .entity_mut(player_entity)
        .get_mut::<PlayerOutboxComp>()
    {
        outbox.0.push_reliable(PlayerEvent::SyncEntityHealthBar {
            entity_id,
            hp_pct: hp_pct.min(100),
        });
    }
}

fn push_damage_info(
    world: &mut World,
    player_entity: Entity,
    entity_id: EntityId,
    damage: i32,
    flags: HitFlags,
) {
    if let Some(mut outbox) = world
        .entity_mut(player_entity)
        .get_mut::<PlayerOutboxComp>()
    {
        outbox.0.push_reliable(PlayerEvent::DamageInfo {
            entity_id,
            flags: damage_info_flags(flags),
            damage,
        });
    }
}

fn damage_info_flags(flags: HitFlags) -> DamageInfoFlags {
    DamageInfoFlags(flags.bits())
}

fn health_bar_percent_for_entity_id(world: &World, entity_id: EntityId) -> Option<u8> {
    let entity = world
        .resource::<NetEntityIndex>()
        .0
        .get(&entity_id)
        .copied()?;
    Some(health_bar_percent_for_entity(world, entity))
}

fn health_bar_percent_for_entity(world: &World, entity: Entity) -> u8 {
    if world.entity(entity).contains::<PlayerStatsComp>() {
        return 0;
    }

    let Some(stats) = world.entity(entity).get::<MobStatsComp>() else {
        return 0;
    };
    let max_hp = stats.0.read_limited(Stat::MaxHp);
    if max_hp <= 0 {
        return 0;
    }

    let hp = stats.0.read_limited(Stat::Hp);
    ((i64::from(hp).clamp(0, i64::from(max_hp)) * 100) / i64::from(max_hp)).clamp(0, 100) as u8
}
