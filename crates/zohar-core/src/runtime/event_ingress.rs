use std::sync::Arc;
use zohar_gamesrv::infra::{ClusterEvent, ClusterEventBus, GlobalShoutEvent};
use zohar_map_port::GlobalShoutMsg;
use zohar_sim::MapEventSender;

pub(crate) fn spawn_cluster_event_ingress(
    runtime: &tokio::runtime::Runtime,
    cluster_events: Arc<ClusterEventBus>,
    map_events: MapEventSender,
) {
    runtime.spawn(async move {
        let Ok(mut rx) = cluster_events.subscribe().await else {
            return;
        };
        while let Ok(event) = rx.recv().await {
            let _ = forward_cluster_event(event.as_ref(), &map_events);
        }
    });
}

pub(crate) fn forward_cluster_event(event: &ClusterEvent, map_events: &MapEventSender) -> bool {
    match event {
        ClusterEvent::GlobalShout(shout) => forward_global_shout(shout, map_events),
    }
}

fn forward_global_shout(shout: &GlobalShoutEvent, map_events: &MapEventSender) -> bool {
    map_events
        .try_send_global_shout(GlobalShoutMsg {
            from_player_name: shout.from_player_name.clone(),
            from_empire: shout.from_empire,
            message_bytes: shout.message.as_bytes().to_vec(),
        })
        .is_ok()
}
