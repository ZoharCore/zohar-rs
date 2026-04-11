use super::{InGamePhaseEffects, try_send_client_intent};
use crate::handlers::ingame::InGameCtx;
use zohar_map_port::{
    ClientIntent, CoreStatAllocationIntent, CoreStatKind, PlayerProgressionIntent,
};

#[derive(clap::Subcommand, Debug, Clone, PartialEq, Eq)]
pub(in crate::handlers::ingame::chat) enum StatsCommand {
    #[command(
        name = "stat",
        about = "Spend one stat point on `st`, `ht`, `dx`, or `iq`."
    )]
    Increase {
        #[arg(value_enum, value_name = "CORE_STAT")]
        stat: CoreStatArg,
    },

    #[command(
        name = "stat-",
        about = "Refund one invested point from `st`, `ht`, `dx`, or `iq`."
    )]
    Decrease {
        #[arg(value_enum, value_name = "CORE_STAT")]
        stat: CoreStatArg,
    },
}

impl StatsCommand {
    pub(super) fn execute(self, state: &mut InGameCtx<'_>) -> InGamePhaseEffects {
        let (intent, command_name) = match self {
            Self::Increase { stat } => (core_stat_intent(stat, 1), "stat"),
            Self::Decrease { stat } => (core_stat_intent(stat, -1), "stat-"),
        };

        try_send_client_intent(state, intent, command_name)
    }
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::handlers::ingame::chat) enum CoreStatArg {
    St,
    Ht,
    Dx,
    Iq,
}

fn core_stat_intent(stat: CoreStatArg, delta: i8) -> ClientIntent {
    ClientIntent::Progression(PlayerProgressionIntent::CoreStat(
        CoreStatAllocationIntent {
            stat: match stat {
                CoreStatArg::St => CoreStatKind::St,
                CoreStatArg::Ht => CoreStatKind::Ht,
                CoreStatArg::Dx => CoreStatKind::Dx,
                CoreStatArg::Iq => CoreStatKind::Iq,
            },
            delta,
        },
    ))
}
