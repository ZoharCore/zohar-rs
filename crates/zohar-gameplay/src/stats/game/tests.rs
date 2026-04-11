mod bootstrap;
mod formulas;
mod modifiers;
mod progression;
mod projection;
mod resources;
mod state;
mod sync;

use super::{
    ActorStatSource, CoreStatBlock, DeterministicGrowthVersion, MobStatSource, PlayerGrowthFormula,
    PlayerResourceFormula, PlayerStatSource, SourceSpeeds, Stat, StatModifierInstance,
    StatModifierLedger, default_mob_balance_rules, default_player_balance_rules,
};
use zohar_domain::entity::player::PlayerClass;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum TestModifierSourceKind {
    EquipmentSlot,
    Buff,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct TestModifierSource {
    kind: TestModifierSourceKind,
    id: u64,
}

impl TestModifierSource {
    const fn new(kind: TestModifierSourceKind, id: u64) -> Self {
        Self { kind, id }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum TestModifierDetail {
    None,
    EquipmentApply(u8),
}

type TestModifierInstance = StatModifierInstance<TestModifierSource, TestModifierDetail>;
type TestModifierLedger = StatModifierLedger<TestModifierSource, TestModifierDetail>;
type TestActorStatState = super::ActorStatState<TestModifierSource, TestModifierDetail>;

impl TestModifierInstance {
    const fn new(source: TestModifierSource, stat: Stat, amount: i32) -> Self {
        Self {
            source,
            stat,
            amount,
            detail: TestModifierDetail::None,
        }
    }

    const fn equipment_apply(self, apply_index: u8) -> Self {
        Self {
            detail: TestModifierDetail::EquipmentApply(apply_index),
            ..self
        }
    }
}

fn player_source(
    base_max_hp: i32,
    base_max_sp: i32,
    base_max_stamina: i32,
    hp_per_ht: i32,
    sp_per_iq: i32,
    stamina_per_ht: i32,
) -> ActorStatSource {
    player_source_for_class(
        base_max_hp,
        base_max_sp,
        base_max_stamina,
        hp_per_ht,
        sp_per_iq,
        stamina_per_ht,
        PlayerClass::Warrior,
    )
}

fn player_source_for_class(
    base_max_hp: i32,
    base_max_sp: i32,
    base_max_stamina: i32,
    hp_per_ht: i32,
    sp_per_iq: i32,
    stamina_per_ht: i32,
    class: PlayerClass,
) -> ActorStatSource {
    ActorStatSource::Player(PlayerStatSource {
        resources: PlayerResourceFormula {
            base_max_hp,
            base_max_sp,
            base_max_stamina,
            hp_per_ht,
            sp_per_iq,
            stamina_per_ht,
        },
        growth: PlayerGrowthFormula::zero(),
        balance: default_player_balance_rules(class),
        speeds: SourceSpeeds::default(),
    })
}

fn player_source_with_growth(
    base_max_hp: i32,
    base_max_sp: i32,
    base_max_stamina: i32,
    hp_per_ht: i32,
    sp_per_iq: i32,
    stamina_per_ht: i32,
    hp_per_level: (i32, i32),
    sp_per_level: (i32, i32),
    stamina_per_level: (i32, i32),
) -> ActorStatSource {
    let mut source = player_source(
        base_max_hp,
        base_max_sp,
        base_max_stamina,
        hp_per_ht,
        sp_per_iq,
        stamina_per_ht,
    );

    if let ActorStatSource::Player(player) = &mut source {
        player.growth = PlayerGrowthFormula {
            hp_per_level,
            sp_per_level,
            stamina_per_level,
            version: DeterministicGrowthVersion::V1,
        };
    }

    source
}

fn mob_source(
    level: i32,
    st: i32,
    ht: i32,
    dx: i32,
    iq: i32,
    max_hp: i32,
    def_grade_flat: i32,
    attack_speed: i32,
    move_speed: i32,
) -> ActorStatSource {
    ActorStatSource::Mob(MobStatSource {
        level,
        core: CoreStatBlock::new(st, ht, dx, iq),
        max_hp,
        def_grade_flat,
        balance: default_mob_balance_rules(),
        speeds: SourceSpeeds {
            attack_speed,
            move_speed,
            casting_speed: attack_speed,
        },
    })
}
