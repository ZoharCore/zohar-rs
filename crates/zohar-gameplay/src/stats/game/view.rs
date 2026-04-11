use std::cmp::Ord;

use super::Stat;
use super::actor::ActorStatState;
use super::source::ActorStatSource;

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatValueView {
    Limited,
    WireCompatible,
}

pub fn read_stat_value<Source, Detail>(
    state: &ActorStatState<Source, Detail>,
    source: Option<&ActorStatSource>,
    stat: Stat,
    view: StatValueView,
) -> i32
where
    Source: Ord + Copy,
{
    match view {
        StatValueView::Limited => {
            let value = read_effective_value(state, stat);
            apply_limit(source, stat, value)
        }
        StatValueView::WireCompatible => read_wire_compatible_value(state, stat),
    }
}

fn read_effective_value<Source, Detail>(state: &ActorStatState<Source, Detail>, stat: Stat) -> i32
where
    Source: Ord + Copy,
{
    match stat {
        Stat::Hp => state.resources().hp,
        Stat::Sp => state.resources().sp,
        Stat::Stamina => state.resources().stamina,
        Stat::HpRecovery => state.resources().hp_recovery,
        Stat::SpRecovery => state.resources().sp_recovery,
        _ => state.computed().get(stat),
    }
}

fn read_wire_compatible_value<Source, Detail>(
    state: &ActorStatState<Source, Detail>,
    stat: Stat,
) -> i32
where
    Source: Ord + Copy,
{
    if matches!(stat, Stat::Exp | Stat::NextExp) {
        let progression = state.progression();
        return match stat {
            Stat::Exp => clamp_u32_to_i32(progression.exp_in_level),
            Stat::NextExp => clamp_u32_to_i32(progression.next_exp_in_level),
            _ => unreachable!("guarded by progression stat match"),
        };
    }

    if matches!(stat, Stat::Level | Stat::Exp | Stat::NextExp | Stat::Gold) {
        return state.base().get(stat);
    }

    read_effective_value(state, stat)
}

fn apply_limit(source: Option<&ActorStatSource>, stat: Stat, value: i32) -> i32 {
    let Some(source) = source else {
        return value;
    };

    let Some(max) = source.view_limit(stat) else {
        return value;
    };

    clamp_optional(value, Some(0), Some(max))
}

fn clamp_optional(value: i32, min: Option<i32>, max: Option<i32>) -> i32 {
    let lower = min.unwrap_or(i32::MIN);
    let upper = max.unwrap_or(i32::MAX);
    value.clamp(lower, upper)
}

fn clamp_u32_to_i32(value: u32) -> i32 {
    value.min(i32::MAX as u32) as i32
}
