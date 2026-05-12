use bevy::prelude::*;
use zohar_gameplay::combat::HitFlags;
use zohar_gameplay::stats::game::{PlayerStaminaTimerCommand, Stat};
use zohar_map_port::{
    ChatChannel, PlayerEvent, ProjectileEffectKind, ProjectileTargetEvent, SpecialEffectType,
    StatUpdate,
};

use super::facts::{
    ActorSpecialEffect, ActorSpecialEffectKind, FrameFacts, PlayerStaminaTimerChanged,
    PointVisualEffect, ProjectileVisualEffectKind, reset_frame_facts,
};
use super::fanout::{ActorAudience, broadcast_actor_event, push_reliable};
use super::state::{MobStatsComp, PlayerMarker};

/// Project accumulated frame facts into map-port events.
///
/// This is the only runtime layer that should translate combat/lifecycle facts into client-facing
/// packets. Keeping that boundary narrow lets later reward, PvP, buff, or quest hooks observe
/// the same facts without being coupled to legacy wire concerns.
pub(crate) fn project_frame_facts(world: &mut World) {
    let damaged = world.resource::<FrameFacts>().combat.damaged.clone();
    for effect in damaged {
        if world
            .entity(effect.victim.entity)
            .contains::<MobStatsComp>()
        {
            crate::runtime::player::target::broadcast_entity_health_bar_to_targeters(
                world,
                effect.victim.id,
            );
        }
        if world
            .entity(effect.attacker.entity)
            .contains::<PlayerMarker>()
        {
            crate::runtime::player::target::send_damage_info_to_selected_target(
                world,
                effect.attacker.entity,
                effect.victim.id,
                effect.damage,
                effect.flags,
            );
        }
        if world
            .entity(effect.victim.entity)
            .contains::<PlayerMarker>()
        {
            crate::runtime::player::target::send_damage_info_to_player(
                world,
                effect.victim.entity,
                effect.victim.id,
                effect.damage,
                effect.flags,
            );
        }

        let mut special_effects = Vec::new();
        if effect.flags.contains(HitFlags::CRITICAL) {
            special_effects.push(ActorSpecialEffectKind::Critical);
        }
        if effect.flags.contains(HitFlags::PENETRATE) {
            special_effects.push(ActorSpecialEffectKind::Piercing);
        }
        for special_effect in special_effects {
            broadcast_special_effect(
                world,
                ActorSpecialEffect {
                    actor: effect.victim,
                    effect: special_effect,
                },
            );
        }
    }

    let dying_started = world.resource::<FrameFacts>().life.dying_started.clone();
    for effect in dying_started {
        broadcast_actor_event(
            world,
            effect.actor.id,
            ActorAudience::ViewAndSelf,
            |entity_id| PlayerEvent::EntityStunned { entity_id },
        );
    }

    let death_finalized = world.resource::<FrameFacts>().life.death_finalized.clone();
    for effect in death_finalized {
        broadcast_actor_event(
            world,
            effect.actor.id,
            ActorAudience::ViewAndSelf,
            |entity_id| PlayerEvent::EntityDead { entity_id },
        );
    }

    let projectile_effects = world
        .resource::<FrameFacts>()
        .visuals
        .projectile_effects
        .clone();
    for effect in projectile_effects {
        broadcast_actor_event(
            world,
            effect.start.id,
            ActorAudience::ViewAndSelf,
            |start_id| PlayerEvent::CreateProjectileEffect {
                effect: map_projectile_effect(effect.effect),
                start_entity_id: start_id,
                end_entity_id: effect.end.id,
            },
        );
    }

    let projectile_targets = world
        .resource::<FrameFacts>()
        .visuals
        .projectile_targets
        .clone();
    for projectile_target in projectile_targets {
        let Some(target_pos) = world
            .entity(projectile_target.target.entity)
            .get::<super::state::LocalTransform>()
            .map(|transform| transform.pos)
        else {
            continue;
        };
        broadcast_actor_event(
            world,
            projectile_target.caster.id,
            ActorAudience::ViewAndSelf,
            |shooter_entity_id| {
                PlayerEvent::SetProjectileTarget(ProjectileTargetEvent {
                    caster_entity_id: shooter_entity_id,
                    target_entity_id: projectile_target.target.id,
                    target_pos,
                    append: false,
                })
            },
        );
    }

    let point_effects = world.resource::<FrameFacts>().visuals.point_effects.clone();
    for effect in point_effects {
        broadcast_point_visual_effect(world, effect);
    }

    let special_effects = world
        .resource::<FrameFacts>()
        .visuals
        .special_effects
        .clone();
    for effect in special_effects {
        broadcast_special_effect(world, effect);
    }

    let despawned = world.resource::<FrameFacts>().cleanup.despawned.clone();
    for effect in despawned {
        for player_entity in effect.recipients {
            push_reliable(
                world,
                player_entity,
                PlayerEvent::EntityDespawn {
                    entity_id: effect.actor_id,
                },
            );
        }
    }

    let stamina_timers = world
        .resource::<FrameFacts>()
        .projection
        .stamina_timer_changed
        .clone();
    for fact in stamina_timers {
        push_reliable(
            world,
            fact.player.entity,
            PlayerEvent::Chat {
                channel: ChatChannel::Command,
                message: stamina_timer_command(fact).into_bytes(),
                sender_entity_id: None,
                empire: None,
            },
        );
    }

    reset_frame_facts(world);
}

fn broadcast_point_visual_effect(world: &mut World, effect: PointVisualEffect) {
    let actor = effect.actor();
    broadcast_actor_event(world, actor.id, ActorAudience::Observers, |entity_id| {
        PlayerEvent::SetEntityStats {
            entity_id,
            stats: vec![point_visual_stat_update(effect)],
        }
    });
}

fn broadcast_special_effect(world: &mut World, effect: ActorSpecialEffect) {
    let special = effect.effect;
    broadcast_actor_event(
        world,
        effect.actor.id,
        ActorAudience::ViewAndSelf,
        |entity_id| PlayerEvent::SpecialEffect {
            entity_id,
            effect: map_special_effect(special),
        },
    );
}

const fn map_projectile_effect(effect: ProjectileVisualEffectKind) -> ProjectileEffectKind {
    match effect {
        ProjectileVisualEffectKind::Experience => ProjectileEffectKind::Exp,
    }
}

const fn map_special_effect(effect: ActorSpecialEffectKind) -> SpecialEffectType {
    match effect {
        ActorSpecialEffectKind::Critical => SpecialEffectType::Critical,
        ActorSpecialEffectKind::Piercing => SpecialEffectType::Penetrate,
    }
}

const fn point_visual_stat_update(effect: PointVisualEffect) -> StatUpdate {
    match effect {
        PointVisualEffect::LevelStep { .. } => StatUpdate {
            stat: Stat::LevelStep,
            absolute: 0, // client ignores value, only plays an animation
        },
        PointVisualEffect::LevelUp { level, .. } => StatUpdate {
            stat: Stat::Level,
            absolute: level, // client updates the level on the nameplate text and plays animation
        },
    }
}

fn stamina_timer_command(fact: PlayerStaminaTimerChanged) -> String {
    match fact.command {
        PlayerStaminaTimerCommand::Start { consume_per_sec } => {
            format!(
                "StartStaminaConsume {consume_per_sec} {}\0",
                fact.current_stamina
            )
        }
        PlayerStaminaTimerCommand::Stop => {
            format!("StopStaminaConsume {}\0", fact.current_stamina)
        }
    }
}
