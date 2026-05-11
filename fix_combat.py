import re

with open("crates/zohar-sim/src/runtime/combat.rs", "r") as f:
    content = f.read()

replacement = """
pub(crate) fn process_mob_attack_windup(world: &mut World) {
    let now = world.resource::<RuntimeState>().sim_now;
    let now_ts = world.resource::<RuntimeState>().packet_now();

    // Collect entities that are ready to strike
    let mut to_execute = Vec::new();
    let mut query = world.query::<(Entity, &crate::runtime::state::LocalTransform, &crate::runtime::state::MobAttackWindup)>();
    for (entity, transform, windup) in query.iter(world) {
        if now >= windup.execute_at {
            to_execute.push((entity, transform.pos, windup.target_entity, windup.max_distance_m));
        }
    }

    for (entity, pos, target_entity_id, max_distance_m) in to_execute {
        // Remove the component
        world
            .entity_mut(entity)
            .remove::<crate::runtime::state::MobAttackWindup>();

        let target_entity = crate::runtime::spatial::net_entity(world, target_entity_id);
        if let Some(target_entity) = target_entity {
            // Check distance
            let target_pos = crate::runtime::spatial::player_position(world, target_entity_id, now_ts);

            let in_range = if let Some(target_pos) = target_pos {
                crate::runtime::rules::movement::distance(pos, target_pos) <= max_distance_m
            } else {
                false
            };

            if in_range {
                world
                    .resource_mut::<AttackCommandBuffer>()
                    .0
                    .push(AttackCommand::MobBasicAttack {
                        attacker_entity: entity,
                        victim_entity: target_entity,
                    });
            }
        }
    }
}
"""

content = re.sub(
    r'pub\(crate\) fn process_mob_attack_windup\(world: &mut World\) \{\n    let now = world\.resource::<RuntimeState>\(\)\.sim_now;\n\n    // Collect entities that are ready to strike\n    let mut to_execute = Vec::new\(\);\n    let mut query = world\.query::<\(Entity, &crate::runtime::state::MobAttackWindup\)>\(\);\n    for \(entity, windup\) in query\.iter\(world\) \{\n        if now >= windup\.execute_at \{\n            to_execute\.push\(\(entity, windup\.target_entity\)\);\n        \}\n    \}\n\n    for \(entity, target_entity_id\) in to_execute \{\n        // Remove the component\n        world\n            \.entity_mut\(entity\)\n            \.remove::<crate::runtime::state::MobAttackWindup>\(\);\n\n        let target_entity = crate::runtime::spatial::net_entity\(world, target_entity_id\);\n        if let Some\(target_entity\) = target_entity \{\n            world\n                \.resource_mut::<AttackCommandBuffer>\(\)\n                \.0\n                \.push\(AttackCommand::MobBasicAttack \{\n                    attacker_entity: entity,\n                    victim_entity: target_entity,\n                \}\);\n        \}\n    \}\n\}',
    replacement.strip('\n') + '\n',
    content
)

with open("crates/zohar-sim/src/runtime/combat.rs", "w") as f:
    f.write(content)
