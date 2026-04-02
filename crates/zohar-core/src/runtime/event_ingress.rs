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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zohar_domain::Empire;
    use zohar_domain::appearance::PlayerAppearance;
    use zohar_domain::coords::{LocalPos, LocalSize};
    use zohar_domain::entity::EntityId;
    use zohar_domain::entity::player::PlayerId;
    use zohar_sim::{
        EntityMotionSpeedTable, MapConfig, MapInstanceKey, SharedConfig, WanderConfig,
        build_map_app,
    };

    fn test_map_runtime() -> (bevy::prelude::App, MapEventSender) {
        build_map_app(
            SharedConfig {
                motion_speeds: Arc::new(EntityMotionSpeedTable::default()),
                mobs: Arc::new(HashMap::new()),
                wander: WanderConfig::default(),
                mob_chat: Arc::default(),
            },
            MapConfig {
                map_key: MapInstanceKey::shared(1, zohar_domain::MapId::new(41)),
                empire: None,
                local_size: LocalSize::new(16_384.0, 16_384.0),
                navigator: None,
                spawn_rules: Vec::new(),
            },
            2,
        )
    }

    #[test]
    fn global_shout_is_forwarded_to_map_inbound() {
        let (mut app, map_events) = test_map_runtime();
        app.update();
        let mut player_rx = map_events
            .enter_player(zohar_map_port::EnterMsg {
                player_id: PlayerId::from(1),
                player_net_id: EntityId(7),
                initial_pos: LocalPos::new(1.0, 2.0),
                appearance: PlayerAppearance {
                    empire: Empire::Yellow,
                    ..PlayerAppearance::default()
                },
            })
            .expect("player enter");
        let _ = app.world_mut().try_run_schedule(bevy::prelude::PreUpdate);
        app.world_mut().run_schedule(bevy::prelude::FixedFirst);
        app.world_mut().run_schedule(bevy::prelude::FixedUpdate);
        app.world_mut().run_schedule(bevy::prelude::FixedPostUpdate);

        let event = ClusterEvent::GlobalShout(GlobalShoutEvent {
            from_player_name: "alice".to_string(),
            from_empire: Empire::Yellow,
            message: "hello".to_string(),
        });

        assert!(forward_cluster_event(&event, &map_events));
        let _ = app.world_mut().try_run_schedule(bevy::prelude::PreUpdate);
        app.world_mut().run_schedule(bevy::prelude::FixedFirst);
        app.world_mut().run_schedule(bevy::prelude::FixedUpdate);
        app.world_mut().run_schedule(bevy::prelude::FixedPostUpdate);

        let event = player_rx.try_recv().expect("chat event");
        assert!(matches!(
            event,
            zohar_map_port::PlayerEvent::Chat {
                channel: zohar_map_port::ChatChannel::Shout,
                empire: Some(Empire::Yellow),
                message,
                ..
            } if message == b"alice : hello\0"
        ));
    }
}
