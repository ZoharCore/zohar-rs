import re

with open("crates/zohar-sim/src/runtime/mob.rs", "r") as f:
    content = f.read()

content = content.replace("#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]", "#[derive(Component, Debug, Clone, Copy, PartialEq)]")

with open("crates/zohar-sim/src/runtime/mob.rs", "w") as f:
    f.write(content)

with open("crates/zohar-sim/src/runtime/action/apply.rs", "r") as f:
    content = f.read()

content = content.replace("crate::runtime::mob::ai::LEGACY_ATTACK_THRESHOLD_RATIO", "1.15")

with open("crates/zohar-sim/src/runtime/action/apply.rs", "w") as f:
    f.write(content)
