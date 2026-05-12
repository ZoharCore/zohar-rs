use bevy::prelude::*;
use zohar_domain::entity::EntityId;

use super::facts::{ActorCleanupDue, ActorDeathFinalized, ActorDyingStarted, ActorRef, FrameFacts};
use super::state::{
    MobAggroQueue, MobBrainMode, MobBrainState, MobMarker, MobMotion, NetEntityId, PlayerMarker,
    RuntimeState, SimDuration, SimInstant,
};

const DYING_TO_DEAD_DELAY: SimDuration = SimDuration::from_millis(3_000);
const PLAYER_RESTART_HERE_DELAY: SimDuration = SimDuration::from_millis(10_000);
const PLAYER_RESTART_TOWN_DELAY: SimDuration = SimDuration::from_millis(7_000);
const PLAYER_FORCED_RESPAWN_DELAY: SimDuration = SimDuration::from_millis(180_000);
const MOB_CORPSE_MIN_DELAY: SimDuration = SimDuration::from_millis(6_000);
const MOB_CORPSE_JITTER_MS: u64 = 4_000;

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[cfg_attr(feature = "admin-brp", reflect(Component))]
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorLifeComp {
    pub(crate) phase: ActorLifePhase,
}

#[cfg_attr(feature = "admin-brp", derive(Reflect))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorLifePhase {
    #[default]
    Alive,
    Dying {
        entered_at: SimInstant,
        dead_at: SimInstant,
    },
    Dead {
        entered_at: SimInstant,
        restart_here_allowed_at: Option<SimInstant>,
        restart_town_allowed_at: Option<SimInstant>,
        forced_respawn_at: Option<SimInstant>,
        cleanup_at: Option<SimInstant>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartReadiness {
    Ready,
    Waiting { retry_after: SimDuration },
}

impl ActorLifeComp {
    pub(crate) const fn alive() -> Self {
        Self {
            phase: ActorLifePhase::Alive,
        }
    }

    pub(crate) const fn is_alive(&self) -> bool {
        matches!(self.phase, ActorLifePhase::Alive)
    }

    pub(crate) const fn is_dead(&self) -> bool {
        matches!(self.phase, ActorLifePhase::Dead { .. })
    }

    pub(crate) const fn is_dying(&self) -> bool {
        matches!(self.phase, ActorLifePhase::Dying { .. })
    }

    pub(crate) const fn can_act(&self) -> bool {
        self.is_alive()
    }

    pub(crate) const fn can_be_combat_target(&self) -> bool {
        !self.is_dead()
    }

    pub(crate) const fn can_take_combat_damage(&self) -> bool {
        self.is_alive()
    }

    pub(crate) const fn can_regenerate(&self) -> bool {
        self.is_alive()
    }

    pub(crate) fn restart_here_readiness(&self, now: SimInstant) -> Option<RestartReadiness> {
        restart_readiness(now, self.restart_here_allowed_at())
    }

    pub(crate) fn restart_town_readiness(&self, now: SimInstant) -> Option<RestartReadiness> {
        restart_readiness(now, self.restart_town_allowed_at())
    }

    pub(crate) fn forced_respawn_due(&self, now: SimInstant) -> bool {
        matches!(
            self.phase,
            ActorLifePhase::Dead {
                forced_respawn_at: Some(at),
                ..
            } if at <= now
        )
    }

    pub(crate) fn restart(&mut self) -> bool {
        if !self.is_dead() {
            return false;
        }
        self.phase = ActorLifePhase::Alive;
        true
    }

    fn restart_here_allowed_at(&self) -> Option<SimInstant> {
        match self.phase {
            ActorLifePhase::Dead {
                restart_here_allowed_at,
                ..
            } => restart_here_allowed_at,
            _ => None,
        }
    }

    fn restart_town_allowed_at(&self) -> Option<SimInstant> {
        match self.phase {
            ActorLifePhase::Dead {
                restart_town_allowed_at,
                ..
            } => restart_town_allowed_at,
            _ => None,
        }
    }

    fn begin_dying(&mut self, now: SimInstant) -> bool {
        if !self.is_alive() {
            return false;
        }
        self.phase = ActorLifePhase::Dying {
            entered_at: now,
            dead_at: now.saturating_add(DYING_TO_DEAD_DELAY),
        };
        true
    }
}

fn restart_readiness(now: SimInstant, allowed_at: Option<SimInstant>) -> Option<RestartReadiness> {
    let allowed_at = allowed_at?;
    if allowed_at <= now {
        Some(RestartReadiness::Ready)
    } else {
        Some(RestartReadiness::Waiting {
            retry_after: allowed_at.saturating_sub(now),
        })
    }
}

pub(crate) fn actor_can_act(world: &World, entity: Entity) -> bool {
    world
        .entity(entity)
        .get::<ActorLifeComp>()
        .is_none_or(ActorLifeComp::can_act)
}

pub(crate) fn actor_can_take_combat_damage(world: &World, entity: Entity) -> bool {
    world
        .entity(entity)
        .get::<ActorLifeComp>()
        .is_none_or(ActorLifeComp::can_take_combat_damage)
}

pub(crate) fn actor_is_dying(world: &World, entity: Entity) -> bool {
    world
        .entity(entity)
        .get::<ActorLifeComp>()
        .is_some_and(ActorLifeComp::is_dying)
}

pub(crate) fn actor_can_be_combat_target(world: &World, entity: Entity) -> bool {
    world
        .entity(entity)
        .get::<ActorLifeComp>()
        .is_none_or(ActorLifeComp::can_be_combat_target)
}

pub(crate) fn process_life_events(world: &mut World) {
    let victims = {
        let effects = world.resource::<FrameFacts>();
        effects
            .combat
            .hp_depleted
            .iter()
            .map(|effect| {
                let _ = effect.killer;
                effect.victim
            })
            .collect::<Vec<_>>()
    };

    for victim in victims {
        begin_actor_dying(world, victim);
    }
}

fn begin_actor_dying(world: &mut World, actor: ActorRef) -> bool {
    let now = world.resource::<RuntimeState>().sim_now;

    if !world.entities().contains(actor.entity)
        || world
            .entity(actor.entity)
            .get::<NetEntityId>()
            .is_none_or(|net| net.net_id != actor.id)
    {
        return false;
    }

    let began_dying = {
        let mut entity_ref = world.entity_mut(actor.entity);
        let Some(mut life) = entity_ref.get_mut::<ActorLifeComp>() else {
            return false;
        };
        life.begin_dying(now)
    };
    if !began_dying {
        return false;
    }

    world
        .resource_mut::<FrameFacts>()
        .life
        .dying_started
        .push(ActorDyingStarted { actor });
    stop_actor_activity_for_dying(world, actor.entity, now);
    true
}

pub(crate) fn finalize_dying_actor_death(world: &mut World, actor: ActorRef) -> bool {
    let now = world.resource::<RuntimeState>().sim_now;

    if !world.entities().contains(actor.entity)
        || world
            .entity(actor.entity)
            .get::<NetEntityId>()
            .is_none_or(|net| net.net_id != actor.id)
    {
        return false;
    }

    let is_player = world.entity(actor.entity).get::<PlayerMarker>().is_some();
    let is_mob = world.entity(actor.entity).get::<MobMarker>().is_some();

    let finalized = {
        let mut entity_ref = world.entity_mut(actor.entity);
        let Some(mut life) = entity_ref.get_mut::<ActorLifeComp>() else {
            return false;
        };
        if !life.is_dying() {
            return false;
        }
        life.phase = death_phase(now, actor.id, is_player, is_mob);
        true
    };

    if finalized {
        world
            .resource_mut::<FrameFacts>()
            .life
            .death_finalized
            .push(ActorDeathFinalized { actor });
    }
    finalized
}

fn stop_actor_activity_for_dying(world: &mut World, entity: Entity, now: SimInstant) {
    if !world.entities().contains(entity) {
        return;
    }

    let mut entity_ref = world.entity_mut(entity);
    if let Some(mut brain) = entity_ref.get_mut::<MobBrainState>() {
        brain.mode = MobBrainMode::Idle;
        brain.target = None;
        brain.attack_windup_until = SimInstant::ZERO;
        brain.next_rethink_at = SimInstant::ZERO;
    }
    if let Some(mut aggro) = entity_ref.get_mut::<MobAggroQueue>() {
        aggro.0.clear();
    }
    if let Some(mut motion) = entity_ref.get_mut::<MobMotion>() {
        let pos = motion.0.segment_end_pos;
        motion.0.segment_start_pos = pos;
        motion.0.segment_end_pos = pos;
        motion.0.segment_start_at = now;
        motion.0.segment_end_at = now;
    }
}

pub(crate) fn process_actor_lifecycle(world: &mut World) {
    let now = world.resource::<RuntimeState>().sim_now;
    let mut newly_dead = Vec::new();
    let mut cleanup = Vec::new();

    {
        let mut query = world.query::<(
            Entity,
            &NetEntityId,
            &mut ActorLifeComp,
            Option<&PlayerMarker>,
            Option<&MobMarker>,
        )>();

        for (entity, net_id, mut life, player, mob) in query.iter_mut(world) {
            match life.phase {
                ActorLifePhase::Dying { dead_at, .. } if now >= dead_at => {
                    life.phase = death_phase(now, net_id.net_id, player.is_some(), mob.is_some());
                    newly_dead.push((entity, net_id.net_id));
                }
                ActorLifePhase::Dead {
                    cleanup_at: Some(cleanup_at),
                    ..
                } if mob.is_some() && now >= cleanup_at => {
                    cleanup.push((entity, net_id.net_id));
                }
                _ => {}
            }
        }
    }

    for (entity, entity_id) in newly_dead {
        world
            .resource_mut::<FrameFacts>()
            .life
            .death_finalized
            .push(ActorDeathFinalized {
                actor: ActorRef::new(entity, entity_id),
            });
    }
    for (entity, entity_id) in cleanup {
        world
            .resource_mut::<FrameFacts>()
            .life
            .cleanup_due
            .push(ActorCleanupDue {
                actor: ActorRef::new(entity, entity_id),
            });
    }
}

fn death_phase(
    now: SimInstant,
    entity_id: EntityId,
    is_player: bool,
    is_mob: bool,
) -> ActorLifePhase {
    ActorLifePhase::Dead {
        entered_at: now,
        restart_here_allowed_at: is_player.then_some(now.saturating_add(PLAYER_RESTART_HERE_DELAY)),
        restart_town_allowed_at: is_player.then_some(now.saturating_add(PLAYER_RESTART_TOWN_DELAY)),
        forced_respawn_at: is_player.then_some(now.saturating_add(PLAYER_FORCED_RESPAWN_DELAY)),
        cleanup_at: is_mob.then_some(mob_cleanup_at(now, entity_id)),
    }
}

fn mob_cleanup_at(now: SimInstant, entity_id: EntityId) -> SimInstant {
    let jitter = u64::from(entity_id.0) % MOB_CORPSE_JITTER_MS;
    now.saturating_add(MOB_CORPSE_MIN_DELAY)
        .saturating_add(SimDuration::from_millis(jitter))
}
