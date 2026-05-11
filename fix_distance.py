import re

with open("crates/zohar-sim/src/runtime/mob.rs", "r") as f:
    content = f.read()

content = content.replace(
    "pub(crate) target_entity: EntityId,",
    "pub(crate) target_entity: EntityId,\n    pub(crate) max_distance_m: f32,"
)

with open("crates/zohar-sim/src/runtime/mob.rs", "w") as f:
    f.write(content)

with open("crates/zohar-sim/src/runtime/action/apply.rs", "r") as f:
    content = f.read()

replacement = """
    // Calculate max allowed distance at execution time
    let attack_range_m = proto.map(|p| crate::runtime::rules::combat::effective_attack_range_m(p.attack_range, p.battle_type)).unwrap_or(1.5);
    let attack_threshold_m = attack_range_m.max(0.0) * crate::runtime::mob::ai::LEGACY_ATTACK_THRESHOLD_RATIO;

    // Add a small buffer for movement extrapolation discrepancies
    let max_distance_m = attack_threshold_m + 1.0;

    let target_entity_id = world
        .entity(target_entity)
        .get::<crate::runtime::state::NetEntityId>()
        .map(|n| n.net_id);

    if let Some(target_entity_id) = target_entity_id {
        world
            .entity_mut(mob_entity)
            .insert(crate::runtime::state::MobAttackWindup {
                execute_at,
                target_entity: target_entity_id,
                max_distance_m,
            });
"""

content = re.sub(
    r'    let target_entity_id = world\n        \.entity\(target_entity\)\n        \.get::<crate::runtime::state::NetEntityId>\(\)\n        \.map\(\|n\| n\.net_id\);\n\n    if let Some\(target_entity_id\) = target_entity_id \{\n        world\n            \.entity_mut\(mob_entity\)\n            \.insert\(crate::runtime::state::MobAttackWindup \{\n                execute_at,\n                target_entity: target_entity_id,\n            \}\);',
    replacement.strip('\n') + '\n',
    content
)

with open("crates/zohar-sim/src/runtime/action/apply.rs", "w") as f:
    f.write(content)
