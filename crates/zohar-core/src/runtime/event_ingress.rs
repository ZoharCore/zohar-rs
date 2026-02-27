use std::sync::Arc;
use zohar_gamesrv::infra::{ClusterEvent, ClusterEventBus, GlobalShoutEvent};
use zohar_sim::{LocalMapInbound, MapEventSender};

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
        .try_send(LocalMapInbound::GlobalShout {
            from_player_name: shout.from_player_name.clone(),
            from_empire: shout.from_empire,
            message_bytes: shout.message.as_bytes().to_vec(),
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::Empire;
    use zohar_sim::InboundEvent;

    #[test]
    fn global_shout_is_forwarded_to_map_inbound() {
        let (map_events, rx) = MapEventSender::channel_pair(2);
        let event = ClusterEvent::GlobalShout(GlobalShoutEvent {
            from_player_name: "alice".to_string(),
            from_empire: Empire::Yellow,
            message: "hello".to_string(),
        });

        assert!(forward_cluster_event(&event, &map_events));

        let InboundEvent::GlobalShout { msg } = rx.recv().expect("event") else {
            panic!("expected global shout");
        };
        assert_eq!(msg.from_player_name, "alice");
        assert_eq!(msg.from_empire, Empire::Yellow);
        assert_eq!(msg.message_bytes, b"hello");
    }
}
