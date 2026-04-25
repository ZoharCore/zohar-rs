use bevy::ecs::component::Mutable;
use bevy::prelude::*;
use zohar_gameplay::stats::game::{
    ActorStatsRuntime, ResourceApplication, ResourceApplicationResult, StatWriteError,
};

use super::facts::{ActorRef, ActorResourceChanged, FrameFacts};
use super::state::{MobStatsComp, PlayerStatsComp};

pub(crate) fn apply_actor_resource(
    world: &mut World,
    actor: ActorRef,
    application: ResourceApplication,
) -> Result<Option<ResourceApplicationResult>, StatWriteError> {
    if world.entity(actor.entity).contains::<PlayerStatsComp>() {
        return apply_resource::<PlayerStatsComp>(world, actor, application);
    }
    if world.entity(actor.entity).contains::<MobStatsComp>() {
        return apply_resource::<MobStatsComp>(world, actor, application);
    }
    Ok(None)
}

trait StatsComp {
    fn runtime(&mut self) -> &mut ActorStatsRuntime;
}

impl StatsComp for MobStatsComp {
    fn runtime(&mut self) -> &mut ActorStatsRuntime {
        &mut self.0
    }
}

impl StatsComp for PlayerStatsComp {
    fn runtime(&mut self) -> &mut ActorStatsRuntime {
        &mut self.0
    }
}

fn apply_resource<T: Component<Mutability = Mutable> + StatsComp>(
    world: &mut World,
    actor: ActorRef,
    application: ResourceApplication,
) -> Result<Option<ResourceApplicationResult>, StatWriteError> {
    let Some(result) = apply_resource_inner::<T>(world, actor.entity, application)? else {
        return Ok(None);
    };

    if !result.is_noop() {
        world
            .resource_mut::<FrameFacts>()
            .resources
            .changed
            .push(ActorResourceChanged {
                actor,
                stat: result.stat,
                previous: result.previous,
                current: result.current,
            });
    }

    Ok(Some(result))
}

fn apply_resource_inner<T: Component<Mutability = Mutable> + StatsComp>(
    world: &mut World,
    entity: Entity,
    application: ResourceApplication,
) -> Result<Option<ResourceApplicationResult>, StatWriteError> {
    let mut entity_ref = world.entity_mut(entity);
    let Some(mut stats) = entity_ref.get_mut::<T>() else {
        return Ok(None);
    };
    stats
        .runtime()
        .with_api_mut(|api| api.apply_resource(application))
        .map(Some)
}
