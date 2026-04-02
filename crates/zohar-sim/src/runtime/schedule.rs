use super::resources::PlayerCount;
use super::resources::RuntimeState;
use super::time::SimInstant;
use bevy::prelude::*;
use std::time::{Duration, Instant};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimSet {
    DrainInbound,
    SyncTickRate,
    Sense,
    Think,
    Act,
    Ambient,
    AoiReconcile,
    ReplicationFlush,
    OutboxFlush,
}

const ACTIVE_SIM_TIMESTEP: Duration = Duration::from_millis(40);
const IDLE_SIM_TIMESTEP: Duration = Duration::from_secs(1);

pub(crate) fn sample_sim_now(packet_time_start: Instant) -> SimInstant {
    SimInstant::from_elapsed(packet_time_start.elapsed())
}

pub(crate) fn advance_sim_time(mut state: ResMut<RuntimeState>) {
    state.sim_now = sample_sim_now(state.packet_time_start);
}

pub(crate) fn sync_fixed_tick_rate(
    player_count: Res<PlayerCount>,
    mut fixed_time: ResMut<Time<Fixed>>,
) {
    let previous = fixed_time.timestep();
    let target = if player_count.0 > 0 {
        ACTIVE_SIM_TIMESTEP
    } else {
        IDLE_SIM_TIMESTEP
    };

    if previous == target {
        return;
    }

    if previous == IDLE_SIM_TIMESTEP && target == ACTIVE_SIM_TIMESTEP {
        let overstep = fixed_time.overstep();
        fixed_time.discard_overstep(overstep);
    }
    fixed_time.set_timestep(target);
}

pub(crate) fn has_active_players(player_count: Res<PlayerCount>) -> bool {
    player_count.0 > 0
}
