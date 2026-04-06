use super::cluster_events::{ClusterEvent, GlobalShoutEvent};
use super::message_bus::PgMessageBus;
use crate::adapters::{ToDomain, ToProtocol};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{OnceCell, broadcast};
use tracing::warn;
use zohar_protocol::game_pkt::Empire as WireEmpire;

const TOPIC_GLOBAL_SHOUT_V1: &str = "core.global_shout.v1";

#[derive(Serialize, Deserialize)]
struct GlobalShoutPayload {
    from_player_name: String,
    from_empire: u8,
    message: String,
}

#[derive(Clone)]
pub(crate) struct PgClusterEventBus {
    transport: PgMessageBus,
    tx: broadcast::Sender<Arc<ClusterEvent>>,
    pump_once: Arc<OnceCell<()>>,
}

impl PgClusterEventBus {
    pub(crate) fn new(pool: sqlx::PgPool, database_url: impl Into<String>) -> Self {
        let transport = PgMessageBus::new(pool, database_url);
        let (tx, _) = broadcast::channel(1024);
        Self {
            transport,
            tx,
            pump_once: Arc::new(OnceCell::new()),
        }
    }

    async fn ensure_pump(&self) -> Result<()> {
        self.pump_once
            .get_or_try_init(|| async {
                let mut raw_rx = self.transport.subscribe(TOPIC_GLOBAL_SHOUT_V1).await?;
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    while let Ok(payload) = raw_rx.recv().await {
                        match decode_global_shout(&payload) {
                            Ok(event) => {
                                let _ = tx.send(Arc::new(event));
                            }
                            Err(error) => {
                                warn!(error = ?error, "failed to decode cluster event payload");
                            }
                        }
                    }
                });
                Ok::<(), anyhow::Error>(())
            })
            .await?;
        Ok(())
    }
}

impl PgClusterEventBus {
    pub(crate) async fn publish(&self, event: Arc<ClusterEvent>) -> Result<()> {
        let payload = encode_event(event.as_ref())?;
        let topic = topic_for_event(event.as_ref());
        self.transport.publish(topic, &payload).await
    }

    pub(crate) async fn subscribe(&self) -> Result<broadcast::Receiver<Arc<ClusterEvent>>> {
        self.ensure_pump().await?;
        Ok(self.tx.subscribe())
    }
}

fn topic_for_event(event: &ClusterEvent) -> &'static str {
    match event {
        ClusterEvent::GlobalShout(_) => TOPIC_GLOBAL_SHOUT_V1,
    }
}

fn encode_event(event: &ClusterEvent) -> Result<String> {
    match event {
        ClusterEvent::GlobalShout(shout) => encode_global_shout(shout),
    }
}

fn encode_global_shout(shout: &GlobalShoutEvent) -> Result<String> {
    let payload = GlobalShoutPayload {
        from_player_name: shout.from_player_name.clone(),
        from_empire: u8::from(shout.from_empire.to_protocol()),
        message: shout.message.clone(),
    };
    Ok(serde_json::to_string(&payload)?)
}

fn decode_global_shout(payload: &str) -> Result<ClusterEvent> {
    let decoded: GlobalShoutPayload = serde_json::from_str(payload)?;
    let from_empire = WireEmpire::try_from(decoded.from_empire)?.to_domain();
    Ok(ClusterEvent::GlobalShout(GlobalShoutEvent {
        from_player_name: decoded.from_player_name,
        from_empire,
        message: decoded.message,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zohar_domain::Empire;

    #[test]
    fn cluster_event_codec_happy_and_error_paths() {
        // Roundtrip encoding/decoding
        let event = ClusterEvent::GlobalShout(GlobalShoutEvent {
            from_player_name: "alice".to_string(),
            from_empire: Empire::Yellow,
            message: "ping".to_string(),
        });
        let encoded = encode_event(&event).expect("encode");
        let decoded = decode_global_shout(&encoded).expect("decode");
        assert_eq!(decoded, event);

        // Invalid payload (missing required fields)
        let err = decode_global_shout(r#"{"from_player_name":"a"}"#).expect_err("must fail");
        assert!(
            err.to_string().contains("missing"),
            "unexpected error: {err:#}"
        );

        // Invalid empire code
        let payload = GlobalShoutPayload {
            from_player_name: "alice".to_string(),
            from_empire: 0,
            message: "ping".to_string(),
        };
        let encoded = serde_json::to_string(&payload).expect("encode");
        let err = decode_global_shout(&encoded).expect_err("must fail");
        assert!(
            err.to_string().contains("No discriminant"),
            "unexpected error: {err:#}"
        );
    }
}
