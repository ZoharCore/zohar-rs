use bevy::prelude::*;
use rand::RngExt;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::mob::MobCombatStats;
use zohar_gameplay::combat::{
    CombatStats, DamageRolls, FlyTiming, HitFlags, NormalHitInput, normal_hit_damage,
    projectile_travel_ms,
};
use zohar_gameplay::stats::game::{ActorStatsRuntime, ResourceApplication, Stat};

use super::actor_stats::apply_actor_resource;
use super::facts::{ActorDamaged, ActorHpDepleted, ActorRef, FrameFacts};
use super::state::{
    LocalTransform, MobRef, MobStatsComp, NetEntityId, PlayerStatsComp, RuntimeState, SharedConfig,
    SimDuration, SimInstant,
};

// Temporary until equipment-backed weapon damage is wired into player combat snapshots.
const PLACEHOLDER_PLAYER_DAMAGE_MIN: i32 = 8;
const PLACEHOLDER_PLAYER_DAMAGE_MAX: i32 = 12;

#[derive(Resource, Default)]
pub(crate) struct AttackCommandBuffer(pub(crate) Vec<AttackCommand>);

#[derive(Resource, Default)]
pub(crate) struct DelayedAttackCommandBuffer(pub(crate) Vec<DelayedAttackCommand>);

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

impl AttackCommand {
    fn entities(self) -> AttackEntities {
        let (attacker, victim) = match self {
            Self::PlayerBasicAttack {
                attacker_entity,
                victim_entity,
            }
            | Self::MobBasicAttack {
                attacker_entity,
                victim_entity,
            } => (attacker_entity, victim_entity),
        };
        AttackEntities { attacker, victim }
    }

    fn live_entities(self, world: &World) -> Option<AttackEntities> {
        let entities = self.entities();
        entities.exist_in(world).then_some(entities)
    }
}

#[derive(Debug, Clone, Copy)]
struct AttackEntities {
    attacker: Entity,
    victim: Entity,
}

impl AttackEntities {
    fn exist_in(self, world: &World) -> bool {
        world.entities().contains(self.attacker) && world.entities().contains(self.victim)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DelayedAttackCommand {
    pub(crate) due_at: SimInstant,
    pub(crate) kind: DelayedAttackKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum DelayedAttackKind {
    Resolve(AttackCommand),
    ReleaseProjectile {
        command: AttackCommand,
        flight: FlyTiming,
    },
}

impl DelayedAttackCommand {
    fn resolve(now: SimInstant, delay: SimDuration, command: AttackCommand) -> Self {
        Self {
            due_at: now.saturating_add(delay),
            kind: DelayedAttackKind::Resolve(command),
        }
    }

    fn release_projectile(
        now: SimInstant,
        delay: SimDuration,
        command: AttackCommand,
        flight: FlyTiming,
    ) -> Self {
        Self {
            due_at: now.saturating_add(delay),
            kind: DelayedAttackKind::ReleaseProjectile { command, flight },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AttackDamageSchedule {
    Immediate,
    Delayed(SimDuration),
    Projectile {
        release_delay: SimDuration,
        flight: FlyTiming,
    },
}

pub(crate) fn schedule_attack_damage(
    world: &mut World,
    now: SimInstant,
    command: AttackCommand,
    schedule: AttackDamageSchedule,
) {
    match schedule {
        AttackDamageSchedule::Immediate => {
            world.resource_mut::<AttackCommandBuffer>().0.push(command)
        }
        AttackDamageSchedule::Delayed(delay) => world
            .resource_mut::<DelayedAttackCommandBuffer>()
            .0
            .push(DelayedAttackCommand::resolve(now, delay, command)),
        AttackDamageSchedule::Projectile {
            release_delay,
            flight,
        } => world.resource_mut::<DelayedAttackCommandBuffer>().0.push(
            DelayedAttackCommand::release_projectile(now, release_delay, command, flight),
        ),
    }
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
    release_due_attack_commands(world);

    let commands = {
        let mut buffer = world.resource_mut::<AttackCommandBuffer>();
        std::mem::take(&mut buffer.0)
    };

    for command in commands {
        process_attack_command(world, command);
    }
}

fn release_due_attack_commands(world: &mut World) {
    let now = world.resource::<RuntimeState>().sim_now;
    let (due, pending) = {
        let mut delayed = world.resource_mut::<DelayedAttackCommandBuffer>();
        std::mem::take(&mut delayed.0)
            .into_iter()
            .partition::<Vec<_>, _>(|scheduled| scheduled.due_at <= now)
    };

    if !pending.is_empty() {
        world.resource_mut::<DelayedAttackCommandBuffer>().0 = pending;
    }
    for scheduled in due {
        match scheduled.kind {
            DelayedAttackKind::Resolve(command) => {
                enqueue_live_attack_command(world, command);
            }
            DelayedAttackKind::ReleaseProjectile { command, flight } => {
                let Some(entities) = command.live_entities(world) else {
                    continue;
                };
                let travel = projectile_travel_delay(world, entities, flight);
                let impact = scheduled.due_at.saturating_add(travel);
                if impact <= now {
                    enqueue_live_attack_command(world, command);
                } else {
                    world.resource_mut::<DelayedAttackCommandBuffer>().0.push(
                        DelayedAttackCommand {
                            due_at: impact,
                            kind: DelayedAttackKind::Resolve(command),
                        },
                    );
                }
            }
        }
    }
}

fn enqueue_live_attack_command(world: &mut World, command: AttackCommand) {
    if command.live_entities(world).is_some() {
        world.resource_mut::<AttackCommandBuffer>().0.push(command);
    }
}

fn projectile_travel_delay(
    world: &World,
    entities: AttackEntities,
    flight: FlyTiming,
) -> SimDuration {
    let distance_m = actor_distance(world, entities).unwrap_or(0.0);
    SimDuration::from_millis(u64::from(projectile_travel_ms(distance_m, flight)))
}

fn actor_distance(world: &World, entities: AttackEntities) -> Option<f32> {
    let a = world.entity(entities.attacker).get::<LocalTransform>()?.pos;
    let b = world.entity(entities.victim).get::<LocalTransform>()?.pos;
    Some(distance(a, b))
}

fn distance(a: LocalPos, b: LocalPos) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx.hypot(dy)
}

fn process_attack_command(world: &mut World, command: AttackCommand) {
    let Some(entities) = command.live_entities(world) else {
        return;
    };
    if try_finalize_dying_victim(world, entities) {
        return;
    }

    let Some(outcome) = resolve_attack_command(world, entities) else {
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

fn try_finalize_dying_victim(world: &mut World, entities: AttackEntities) -> bool {
    if !crate::runtime::actor_life::actor_can_act(world, entities.attacker)
        || !crate::runtime::actor_life::actor_is_dying(world, entities.victim)
    {
        return false;
    }

    let Some(victim) = combatant_snapshot(world, entities.victim) else {
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

fn resolve_attack_command(world: &mut World, entities: AttackEntities) -> Option<CombatOutcome> {
    if !crate::runtime::actor_life::actor_can_act(world, entities.attacker)
        || !crate::runtime::actor_life::actor_can_take_combat_damage(world, entities.victim)
    {
        return None;
    }

    let attacker = combatant_snapshot(world, entities.attacker)?;
    let victim = combatant_snapshot(world, entities.victim)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> World {
        let mut world = World::new();
        world.init_resource::<AttackCommandBuffer>();
        world.init_resource::<DelayedAttackCommandBuffer>();
        world.init_resource::<RuntimeState>();
        world
    }

    fn mob_attack(attacker_entity: Entity, victim_entity: Entity) -> AttackCommand {
        AttackCommand::MobBasicAttack {
            attacker_entity,
            victim_entity,
        }
    }

    #[test]
    fn stale_immediate_attack_command_is_dropped() {
        let mut world = test_world();
        let attacker = world.spawn_empty().id();
        let victim = world.spawn_empty().id();
        let _ = world.despawn(victim);
        world
            .resource_mut::<AttackCommandBuffer>()
            .0
            .push(mob_attack(attacker, victim));

        process_attack_commands(&mut world);

        assert!(world.resource::<AttackCommandBuffer>().0.is_empty());
        assert!(world.resource::<DelayedAttackCommandBuffer>().0.is_empty());
    }

    #[test]
    fn stale_delayed_attack_command_is_dropped() {
        let mut world = test_world();
        let attacker = world.spawn_empty().id();
        let victim = world.spawn_empty().id();
        let command = mob_attack(attacker, victim);
        schedule_attack_damage(
            &mut world,
            SimInstant::ZERO,
            command,
            AttackDamageSchedule::Delayed(SimDuration::ZERO),
        );
        let _ = world.despawn(victim);

        process_attack_commands(&mut world);

        assert!(world.resource::<AttackCommandBuffer>().0.is_empty());
        assert!(world.resource::<DelayedAttackCommandBuffer>().0.is_empty());
    }

    #[test]
    fn stale_projectile_release_command_is_dropped() {
        let mut world = test_world();
        let attacker = world.spawn_empty().id();
        let victim = world.spawn_empty().id();
        let command = mob_attack(attacker, victim);
        schedule_attack_damage(
            &mut world,
            SimInstant::ZERO,
            command,
            AttackDamageSchedule::Projectile {
                release_delay: SimDuration::ZERO,
                flight: FlyTiming::FALLBACK_PROJECTILE,
            },
        );
        let _ = world.despawn(attacker);

        process_attack_commands(&mut world);

        assert!(world.resource::<AttackCommandBuffer>().0.is_empty());
        assert!(world.resource::<DelayedAttackCommandBuffer>().0.is_empty());
    }
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
