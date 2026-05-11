import re

with open("crates/zohar-sim/src/runtime/combat.rs", "r") as f:
    content = f.read()

# I need to add distance check to `process_mob_attack_windup`.
# But `process_mob_attack_windup` in `combat.rs` doesn't have an easy way to check the attack range since it needs the mob's proto.
# We could store the `max_distance` in `MobAttackWindup` when the attack is queued? Yes, that's much cleaner!
pass
