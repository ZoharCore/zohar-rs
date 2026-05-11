use bevy::prelude::*;
use tracing::warn;
use zohar_gameplay::stats::game::{PlayerMobExpRewardOutcome, PlayerProgressionState, Stat};

use super::facts::{
    FrameFacts, PointVisualEffect, ProjectileVisualEffect, ProjectileVisualEffectKind,
};
use super::player::persistence::mark_player_dirty;
use super::state::{
    MobRef, NetEntityId, PlayerAppearanceComp, PlayerMarker, PlayerProgressionComp,
    PlayerStatsComp, SharedConfig,
};

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct LastHitMobDeathRewardClaim {
    killer: super::facts::ActorRef,
}

pub(crate) fn record_mob_death_reward_claims(world: &mut World) {
    let deaths = world.resource::<FrameFacts>().combat.hp_depleted.clone();

    for death in deaths {
        let Some(killer) = death.killer else {
            continue;
        };
        if !world.entities().contains(death.victim.entity)
            || !world.entities().contains(killer.entity)
            || !world.entity(killer.entity).contains::<PlayerMarker>()
        {
            continue;
        }

        if !world.entity(death.victim.entity).contains::<MobRef>() {
            continue;
        }

        let Some(mob_reward) = mob_exp_reward(world, death.victim.entity) else {
            continue;
        };

        if mob_reward.base_exp <= 0 {
            continue;
        }

        world
            .entity_mut(death.victim.entity)
            .insert(LastHitMobDeathRewardClaim { killer });
    }
}

pub(crate) fn grant_mob_death_rewards(world: &mut World) {
    let deaths = world.resource::<FrameFacts>().life.death_finalized.clone();

    for death in deaths {
        if !world.entities().contains(death.actor.entity) {
            continue;
        }

        let Some(claim) = world
            .entity(death.actor.entity)
            .get::<LastHitMobDeathRewardClaim>()
            .copied()
        else {
            continue;
        };
        let killer = claim.killer;
        if !world.entities().contains(killer.entity)
            || !world.entity(killer.entity).contains::<PlayerMarker>()
            || world
                .entity(killer.entity)
                .get::<NetEntityId>()
                .is_none_or(|net| net.net_id != killer.id)
        {
            let _ = world
                .entity_mut(death.actor.entity)
                .remove::<LastHitMobDeathRewardClaim>();
            continue;
        }

        let Some(mob_reward) = mob_exp_reward(world, death.actor.entity) else {
            continue;
        };
        match apply_player_exp_reward(world, killer.entity, mob_reward) {
            Ok(Some(applied)) => {
                mark_player_dirty(world, killer.entity);
                enqueue_exp_reward_visuals(world, death.actor, killer, applied);
            }
            Ok(None) => {}
            Err(error) => warn!(?error, "failed to apply mob death exp reward"),
        }

        let _ = world
            .entity_mut(death.actor.entity)
            .remove::<LastHitMobDeathRewardClaim>();
    }
}

fn enqueue_exp_reward_visuals(
    world: &mut World,
    source: super::facts::ActorRef,
    recipient: super::facts::ActorRef,
    applied: AppliedExpReward,
) {
    let mut facts = world.resource_mut::<FrameFacts>();
    if applied.show_stat_point_step {
        facts
            .visuals
            .point_effects
            .push(PointVisualEffect::LevelStep { actor: recipient });
    }
    if let Some(level) = applied.level_up {
        facts
            .visuals
            .point_effects
            .push(PointVisualEffect::LevelUp {
                actor: recipient,
                level,
            });
    }
    facts
        .visuals
        .projectile_effects
        .push(ProjectileVisualEffect {
            effect: ProjectileVisualEffectKind::Experience,
            start: source,
            end: recipient,
        });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RewardError {
    MissingPlayerState,
    InvalidStatWrite {
        stat: Stat,
        source: zohar_gameplay::stats::game::StatWriteError,
    },
}

impl From<(Stat, zohar_gameplay::stats::game::StatWriteError)> for RewardError {
    fn from((stat, source): (Stat, zohar_gameplay::stats::game::StatWriteError)) -> Self {
        Self::InvalidStatWrite { stat, source }
    }
}

fn map_stat_write(
    stat: Stat,
    result: Result<impl Sized, zohar_gameplay::stats::game::StatWriteError>,
) -> Result<(), RewardError> {
    result.map(|_| ()).map_err(|error| (stat, error).into())
}

fn apply_reward_stats(
    stats: &mut PlayerStatsComp,
    outcome: &PlayerMobExpRewardOutcome,
) -> Result<(), RewardError> {
    let current_stat_points = stats.0.read_packet(Stat::StatPoints);
    stats.0.with_api_mut(|api| {
        api.set_player_progression(outcome.progression);
        if outcome.stat_points_gained > 0 {
            map_stat_write(
                Stat::StatPoints,
                api.set_stored_stat(
                    Stat::StatPoints,
                    current_stat_points.saturating_add(outcome.stat_points_gained),
                ),
            )?;
        }
        Ok::<(), RewardError>(())
    })?;

    if outcome.level_steps_gained > 0 {
        stats.0.normalize();
        stats.0.with_api_mut(|api| {
            let max_hp = api.read_limited(Stat::MaxHp);
            let max_sp = api.read_limited(Stat::MaxSp);
            let max_stamina = api.read_limited(Stat::MaxStamina);
            map_stat_write(Stat::Hp, api.set_resource(Stat::Hp, max_hp))?;
            map_stat_write(Stat::Sp, api.set_resource(Stat::Sp, max_sp))?;
            map_stat_write(Stat::Stamina, api.set_resource(Stat::Stamina, max_stamina))?;
            Ok::<(), RewardError>(())
        })?;
    }

    Ok(())
}

fn apply_player_exp_reward(
    world: &mut World,
    player_entity: Entity,
    mob_reward: MobExpReward,
) -> Result<Option<AppliedExpReward>, RewardError> {
    let shared = world.resource::<SharedConfig>().clone();
    let mut query = world.query::<(
        &mut PlayerProgressionComp,
        &mut PlayerStatsComp,
        &mut PlayerAppearanceComp,
    )>();
    let Ok((mut progression, mut stats, mut appearance)) = query.get_mut(world, player_entity)
    else {
        return Err(RewardError::MissingPlayerState);
    };

    let current = PlayerProgressionState::new(
        progression.level,
        progression.exp_in_level.clamp(0, i64::from(u32::MAX)) as u32,
        stats.0.read_packet(Stat::NextExp).clamp(0, i32::MAX) as u32,
    );
    let Some(outcome) =
        shared
            .player_stats
            .apply_mob_exp_reward(current, mob_reward.level, mob_reward.base_exp)
    else {
        return Ok(None);
    };

    progression.level = outcome.progression.level;
    progression.exp_in_level = i64::from(outcome.progression.exp_in_level);
    apply_reward_stats(&mut stats, &outcome)?;

    if let Some(update) = stats.0.normalize().public_state {
        appearance.0.level = update.stats.level;
        appearance.0.move_speed = update.stats.move_speed;
        appearance.0.attack_speed = update.stats.attack_speed;
    }

    Ok(Some(AppliedExpReward {
        show_stat_point_step: outcome.stat_points_gained > 0,
        level_up: outcome.level_up,
    }))
}

#[derive(Debug, Clone, Copy)]
struct MobExpReward {
    base_exp: i64,
    level: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AppliedExpReward {
    show_stat_point_step: bool,
    level_up: Option<i32>,
}

fn mob_exp_reward(world: &World, mob_entity: Entity) -> Option<MobExpReward> {
    let mob_id = world.entity(mob_entity).get::<MobRef>()?.mob_id;
    let proto = world.resource::<SharedConfig>().mobs.get(&mob_id)?;
    Some(MobExpReward {
        base_exp: proto.rewards.experience,
        level: proto.level.min(i32::MAX as u32) as i32,
    })
}
