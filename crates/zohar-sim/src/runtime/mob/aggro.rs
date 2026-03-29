use bevy::prelude::*;

use super::query::mob_pack_members;
use super::state::{MAX_MOB_STIMULI_PER_TICK, MobAggro, MobAggroQueue, MobRef};

#[derive(Resource, Default)]
pub(crate) struct MobAggroDispatchBuffer(pub(crate) Vec<MobAggroDispatch>);

#[derive(Debug, Clone, Copy)]
pub(crate) struct MobAggroDispatch {
    pub(crate) attacked_mob_entity: Entity,
    pub(crate) aggro: MobAggro,
}

pub(crate) fn route_mob_aggro(world: &mut World) {
    let dispatches = {
        let mut buffer = world.resource_mut::<MobAggroDispatchBuffer>();
        std::mem::take(&mut buffer.0)
    };

    for dispatch in dispatches {
        if !world.entities().contains(dispatch.attacked_mob_entity)
            || !world
                .entity(dispatch.attacked_mob_entity)
                .contains::<MobRef>()
        {
            continue;
        }

        for mob_entity in mob_pack_members(world, dispatch.attacked_mob_entity) {
            queue_mob_aggro(world, mob_entity, dispatch.aggro);
        }
    }
}

fn queue_mob_aggro(world: &mut World, mob_entity: Entity, aggro: MobAggro) {
    let mut entity = world.entity_mut(mob_entity);
    let Some(mut queue) = entity.get_mut::<MobAggroQueue>() else {
        return;
    };
    queue.0.push(aggro);
    if queue.0.len() > MAX_MOB_STIMULI_PER_TICK {
        let overflow = queue.0.len() - MAX_MOB_STIMULI_PER_TICK;
        queue.0.drain(0..overflow);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::state::{MobAggroQueue, MobPackId};

    #[test]
    fn route_mob_aggro_fans_out_to_pack_members() {
        let mut world = World::new();
        world.init_resource::<MobAggroDispatchBuffer>();

        let attacked = world
            .spawn((
                MobRef {
                    mob_id: zohar_domain::entity::mob::MobId::new(1),
                },
                MobPackId { pack_id: 9 },
                MobAggroQueue::default(),
            ))
            .id();
        let ally = world
            .spawn((
                MobRef {
                    mob_id: zohar_domain::entity::mob::MobId::new(2),
                },
                MobPackId { pack_id: 9 },
                MobAggroQueue::default(),
            ))
            .id();
        let outsider = world
            .spawn((
                MobRef {
                    mob_id: zohar_domain::entity::mob::MobId::new(3),
                },
                MobPackId { pack_id: 10 },
                MobAggroQueue::default(),
            ))
            .id();

        world
            .resource_mut::<MobAggroDispatchBuffer>()
            .0
            .push(MobAggroDispatch {
                attacked_mob_entity: attacked,
                aggro: MobAggro::ProvokedBy {
                    attacker: zohar_domain::entity::EntityId(77),
                },
            });

        route_mob_aggro(&mut world);

        assert_eq!(
            world
                .entity(attacked)
                .get::<MobAggroQueue>()
                .unwrap()
                .0
                .len(),
            1
        );
        assert_eq!(
            world.entity(ally).get::<MobAggroQueue>().unwrap().0.len(),
            1
        );
        assert!(
            world
                .entity(outsider)
                .get::<MobAggroQueue>()
                .unwrap()
                .0
                .is_empty()
        );
    }
}
