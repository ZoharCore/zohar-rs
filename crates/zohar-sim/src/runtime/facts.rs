use bevy::prelude::*;
use zohar_domain::entity::EntityId;
use zohar_gameplay::combat::HitFlags;
use zohar_gameplay::stats::game::{PlayerStaminaTimerCommand, Stat};

/// One-tick handoff between simulation mutation and outward effects.
///
/// Systems before projection may mutate actors and append typed facts here. They must not
/// encode map-port or wire packets directly. Projection is the boundary that translates these
/// facts into `PlayerEvent`s, then clears the resource before the next tick.
#[derive(Resource, Default)]
pub(crate) struct FrameFacts {
    pub(crate) combat: CombatFacts,
    pub(crate) resources: ResourceFacts,
    pub(crate) life: LifeFacts,
    pub(crate) cleanup: CleanupFacts,
    pub(crate) persistence: PersistenceFacts,
    pub(crate) projection: ProjectionFacts,
}

#[derive(Debug, Default)]
pub(crate) struct CombatFacts {
    pub(crate) damaged: Vec<ActorDamaged>,
    pub(crate) hp_depleted: Vec<ActorHpDepleted>,
}

#[derive(Debug, Default)]
pub(crate) struct ResourceFacts {
    pub(crate) changed: Vec<ActorResourceChanged>,
}

#[derive(Debug, Default)]
pub(crate) struct LifeFacts {
    pub(crate) dying_started: Vec<ActorDyingStarted>,
    pub(crate) death_finalized: Vec<ActorDeathFinalized>,
    pub(crate) cleanup_due: Vec<ActorCleanupDue>,
}

#[derive(Debug, Default)]
pub(crate) struct CleanupFacts {
    pub(crate) despawned: Vec<ActorDespawned>,
}

#[derive(Debug, Default)]
pub(crate) struct PersistenceFacts {
    pub(crate) player_dirty: Vec<Entity>,
}

impl PersistenceFacts {
    pub(crate) fn mark_player_dirty(&mut self, player: Entity) {
        if !self.player_dirty.contains(&player) {
            self.player_dirty.push(player);
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ProjectionFacts {
    pub(crate) stamina_timer_changed: Vec<PlayerStaminaTimerChanged>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorDamaged {
    pub(crate) attacker: ActorRef,
    pub(crate) victim: ActorRef,
    pub(crate) damage: i32,
    pub(crate) flags: HitFlags,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorResourceChanged {
    pub(crate) actor: ActorRef,
    pub(crate) stat: Stat,
    pub(crate) previous: i32,
    pub(crate) current: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorHpDepleted {
    pub(crate) victim: ActorRef,
    pub(crate) killer: Option<ActorRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorDyingStarted {
    pub(crate) actor: ActorRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorDeathFinalized {
    pub(crate) actor: ActorRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorCleanupDue {
    pub(crate) actor: ActorRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActorDespawned {
    pub(crate) actor_id: EntityId,
    pub(crate) recipients: Vec<Entity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlayerStaminaTimerChanged {
    pub(crate) player: ActorRef,
    pub(crate) command: PlayerStaminaTimerCommand,
    pub(crate) current_stamina: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActorRef {
    pub(crate) entity: Entity,
    pub(crate) id: EntityId,
}

impl ActorRef {
    pub(crate) const fn new(entity: Entity, id: EntityId) -> Self {
        Self { entity, id }
    }
}

pub(crate) fn reset_frame_facts(world: &mut World) {
    *world.resource_mut::<FrameFacts>() = FrameFacts::default();
}
