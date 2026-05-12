use bevy::prelude::*;
use rand::RngExt;
use zohar_domain::entity::mob::MobCombatStats;
use zohar_gameplay::combat::{
    CombatStats, DamageRolls, HitFlags, NormalHitInput, normal_hit_damage,
};
use zohar_gameplay::stats::game::{ActorStatsRuntime, ResourceApplication, Stat};

use super::actor_stats::apply_actor_resource;
use super::facts::{ActorDamaged, ActorHpDepleted, ActorRef, FrameFacts};
use super::state::{
    MobRef, MobStatsComp, NetEntityId, PlayerStatsComp, RuntimeState, SharedConfig,
};

// Temporary until equipment-backed weapon damage is wired into player combat snapshots.
const PLACEHOLDER_PLAYER_DAMAGE_MIN: i32 = 8;
const PLACEHOLDER_PLAYER_DAMAGE_MAX: i32 = 12;

#[derive(Resource, Default)]
pub(crate) struct AttackCommandBuffer(pub(crate) Vec<AttackCommand>);

#[derive(Debug, Clone, Copy)]
pub(crate) enum AttackCommand {
    PlayerBasicAttack {
        attacker_entity: Entity,
        victim_entity: Entity,
    },
    MobBasicAttack {
        attacker_entity: Entity,
        victim_entity: Entity,
    },
}

#[derive(Debug, Clone, Copy)]
struct ResolvedHit {
    damage: i32,
    flags: HitFlags,
}

#[derive(Debug, Clone, Copy)]
struct CombatOutcome {
    attacker: CombatantSnapshot,
    victim: CombatantSnapshot,
    hit: ResolvedHit,
}

#[derive(Debug, Clone, Copy)]
struct CombatantSnapshot {
    actor: ActorRef,
    stats: CombatStats,
    damage_roll: DamageRoll,
}

#[derive(Debug, Clone, Copy)]
enum DamageRoll {
    PlaceholderPlayerWeapon,
    MobPrototype(MobCombatStats),
}

#[derive(Debug, Clone, Copy)]
struct AppliedDamage {
    killed: bool,
}

pub(crate) fn process_attack_commands(world: &mut World) {
    let commands = {
        let mut buffer = world.resource_mut::<AttackCommandBuffer>();
        std::mem::take(&mut buffer.0)
    };

    for command in commands {
        process_attack_command(world, command);
    }
}

fn process_attack_command(world: &mut World, command: AttackCommand) {
    if try_finalize_dying_victim(world, command) {
        return;
    }

    let Some(outcome) = resolve_attack_command(world, command) else {
        return;
    };
    let Some(applied) = apply_combat_outcome(world, outcome) else {
        return;
    };

    if applied.killed {
        world
            .resource_mut::<FrameFacts>()
            .combat
            .hp_depleted
            .push(ActorHpDepleted {
                victim: outcome.victim.actor,
                killer: Some(outcome.attacker.actor),
            });
    }
}

fn try_finalize_dying_victim(world: &mut World, command: AttackCommand) -> bool {
    let (attacker_entity, victim_entity) = match command {
        AttackCommand::PlayerBasicAttack {
            attacker_entity,
            victim_entity,
        }
        | AttackCommand::MobBasicAttack {
            attacker_entity,
            victim_entity,
        } => (attacker_entity, victim_entity),
    };

    if !crate::runtime::actor_life::actor_can_act(world, attacker_entity)
        || !crate::runtime::actor_life::actor_is_dying(world, victim_entity)
    {
        return false;
    }

    let Some(victim) = combatant_snapshot(world, victim_entity) else {
        return false;
    };
    crate::runtime::actor_life::finalize_dying_actor_death(world, victim.actor)
}

fn resolve_normal_hit(input: NormalHitInput) -> ResolvedHit {
    let outcome = normal_hit_damage(input);
    ResolvedHit {
        damage: outcome.damage,
        flags: HitFlags::NORMAL,
    }
}

fn resolve_attack_command(world: &mut World, command: AttackCommand) -> Option<CombatOutcome> {
    let (attacker_entity, victim_entity) = match command {
        AttackCommand::PlayerBasicAttack {
            attacker_entity,
            victim_entity,
        }
        | AttackCommand::MobBasicAttack {
            attacker_entity,
            victim_entity,
        } => (attacker_entity, victim_entity),
    };
    if !crate::runtime::actor_life::actor_can_act(world, attacker_entity)
        || !crate::runtime::actor_life::actor_can_take_combat_damage(world, victim_entity)
    {
        return None;
    }

    let attacker = combatant_snapshot(world, attacker_entity)?;
    let victim = combatant_snapshot(world, victim_entity)?;
    let hit = resolve_normal_hit(normal_hit_input(world, attacker, victim));

    Some(CombatOutcome {
        attacker,
        victim,
        hit,
    })
}

fn combatant_snapshot(world: &World, entity: Entity) -> Option<CombatantSnapshot> {
    let id = world.entity(entity).get::<NetEntityId>()?.net_id;
    if let Some(stats) = world.entity(entity).get::<PlayerStatsComp>() {
        return Some(CombatantSnapshot {
            actor: ActorRef::new(entity, id),
            stats: combat_stats_from_runtime(&stats.0),
            damage_roll: DamageRoll::PlaceholderPlayerWeapon,
        });
    }

    if let Some(stats) = world.entity(entity).get::<MobStatsComp>() {
        return Some(CombatantSnapshot {
            actor: ActorRef::new(entity, id),
            stats: combat_stats_from_runtime(&stats.0),
            damage_roll: DamageRoll::MobPrototype(mob_combat_stats(world, entity)?),
        });
    }

    None
}

fn mob_combat_stats(world: &World, mob_entity: Entity) -> Option<MobCombatStats> {
    let mob_id = world.entity(mob_entity).get::<MobRef>()?.mob_id;
    Some(world.resource::<SharedConfig>().mobs.get(&mob_id)?.combat)
}

fn normal_hit_input(
    world: &mut World,
    attacker: CombatantSnapshot,
    victim: CombatantSnapshot,
) -> NormalHitInput {
    let rolls = match attacker.damage_roll {
        DamageRoll::PlaceholderPlayerWeapon => roll_damage(
            world,
            PLACEHOLDER_PLAYER_DAMAGE_MIN,
            PLACEHOLDER_PLAYER_DAMAGE_MAX,
        ),
        DamageRoll::MobPrototype(combat) => roll_damage(
            world,
            combat.damage_min.max(0),
            combat.damage_max.max(combat.damage_min).max(0),
        ),
    };

    let mut input = NormalHitInput::unmodified(attacker.stats, victim.stats, rolls);
    if let DamageRoll::MobPrototype(combat) = attacker.damage_roll {
        input.damage_multiplier = combat.damage_multiplier.max(0.0);
    }
    input
}

fn apply_combat_outcome(world: &mut World, outcome: CombatOutcome) -> Option<AppliedDamage> {
    let applied = apply_resource_damage(world, outcome.victim.actor, outcome.hit.damage)?;

    world
        .resource_mut::<FrameFacts>()
        .combat
        .damaged
        .push(ActorDamaged {
            attacker: outcome.attacker.actor,
            victim: outcome.victim.actor,
            damage: outcome.hit.damage,
            flags: outcome.hit.flags,
        });

    Some(applied)
}

fn apply_resource_damage(
    world: &mut World,
    victim: ActorRef,
    damage: i32,
) -> Option<AppliedDamage> {
    let result = apply_actor_resource(world, victim, ResourceApplication::damage(damage))
        .ok()
        .flatten()?;
    Some(AppliedDamage {
        killed: result.stat == Stat::Hp && result.previous > 0 && result.current <= 0,
    })
}

fn combat_stats_from_runtime(runtime: &ActorStatsRuntime) -> CombatStats {
    CombatStats {
        level: runtime.read_limited(Stat::Level),
        dx: runtime.read_limited(Stat::Dx),
        attack_grade: runtime.read_limited(Stat::AttGrade),
        defence_grade: runtime.read_limited(Stat::DefGrade),
    }
}

fn roll_damage(world: &mut World, damage_min: i32, damage_max: i32) -> DamageRolls {
    let (lower, upper) = if damage_min <= damage_max {
        (damage_min, damage_max)
    } else {
        (damage_max, damage_min)
    };
    let mut runtime = world.resource_mut::<RuntimeState>();
    DamageRolls {
        weapon_or_mob_damage: runtime.rng.random_range(lower..=upper),
        low_damage_fallback: runtime.rng.random_range(1..=5),
    }
}
