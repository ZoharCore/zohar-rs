use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::{Mutex, broadcast};
use tracing::warn;

#[derive(Clone)]
pub(crate) struct PgMessageBus {
    pool: PgPool,
    database_url: String,
    channels: Arc<Mutex<HashMap<String, broadcast::Sender<String>>>>,
}

impl PgMessageBus {
    pub(crate) fn new(pool: PgPool, database_url: impl Into<String>) -> Self {
        Self {
            pool,
            database_url: database_url.into(),
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn publish(&self, channel: &str, payload: &str) -> anyhow::Result<()> {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(channel)
            .bind(payload)
            .execute(&self.pool)
            .await
            .with_context(|| format!("publish pg notify for channel '{channel}'"))?;
        Ok(())
    }

    pub(crate) async fn subscribe(
        &self,
        channel: &str,
    ) -> anyhow::Result<broadcast::Receiver<String>> {
        let mut channels = self.channels.lock().await;
        if let Some(sender) = channels.get(channel) {
            return Ok(sender.subscribe());
        }

        let (sender, _) = broadcast::channel::<String>(1024);
        channels.insert(channel.to_string(), sender.clone());

        let database_url = self.database_url.clone();
        let channel_name = channel.to_string();
        let task_sender = sender.clone();
        tokio::spawn(async move {
            loop {
                let mut listener = match PgListener::connect(&database_url).await {
                    Ok(listener) => listener,
                    Err(error) => {
                        warn!(
                            channel = %channel_name,
                            error = ?error,
                            "pg message bus listener connect failed"
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };

                if let Err(error) = listener.listen(&channel_name).await {
                    warn!(
                        channel = %channel_name,
                        error = ?error,
                        "pg message bus listen failed"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }

                loop {
                    let notification = match listener.recv().await {
                        Ok(notification) => notification,
                        Err(error) => {
                            warn!(
                                channel = %channel_name,
                                error = ?error,
                                "pg message bus recv failed; reconnecting"
                            );
                            break;
                        }
                    };
                    let _ = task_sender.send(notification.payload().to_owned());
                }

                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });

        Ok(sender.subscribe())
    }
}
