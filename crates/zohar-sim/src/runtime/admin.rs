use bevy::ecs::world::DeferredWorld;
use bevy::prelude::*;
use bevy::remote::RemotePlugin;
#[cfg(not(test))]
use bevy::remote::http::RemoteHttpPlugin;
use tracing::warn;
use zohar_domain::entity::player::PlayerId;
use zohar_map_port::{ClientIntent, ClientIntentMsg, PlayerEvent};

use super::ingress::handle_client_intent;
use super::state::PlayerIndex;
use super::state::PlayerOutboxComp;

pub(crate) struct AdminPlugin;

impl Plugin for AdminPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RemotePlugin::default())
            .add_observer(handle_admin_command);
        #[cfg(not(test))]
        app.add_plugins(RemoteHttpPlugin::default());
    }
}

#[derive(Event, Reflect, Debug, Clone)]
#[reflect(Event)]
pub enum AdminCommand {
    PushPlayerEvent {
        player_id: PlayerId,
        event: PlayerEvent,
    },
    PushClientIntent {
        player_id: PlayerId,
        intent: ClientIntent,
    },
}

fn handle_admin_command(trigger: On<AdminCommand>, mut world: DeferredWorld) {
    let command = trigger.event().clone();
    match command {
        AdminCommand::PushPlayerEvent { player_id, event } => {
            push_player_event(world.reborrow(), player_id, event);
        }
        AdminCommand::PushClientIntent { player_id, intent } => {
            handle_client_intent(world.reborrow(), ClientIntentMsg { player_id, intent });
        }
    }
}

fn push_player_event(mut world: DeferredWorld, player_id: PlayerId, event: PlayerEvent) {
    let Some(player_entity) = world.resource::<PlayerIndex>().0.get(&player_id).copied() else {
        warn!(player_id = ?player_id, "Ignoring admin push for unknown player");
        return;
    };

    let mut player_entity_mut = world.entity_mut(player_entity);
    let Some(mut outbox) = player_entity_mut.get_mut::<PlayerOutboxComp>() else {
        warn!(
            player_id = ?player_id,
            entity = ?player_entity,
            "Ignoring admin push for player entity without outbox"
        );
        return;
    };

    outbox.0.push_reliable(event);
}
