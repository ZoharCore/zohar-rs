use anyhow::{Context, anyhow};
use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::error;

#[derive(Deserialize, Debug, Clone)]
pub struct GameServer {
    pub status: Option<GameServerStatus>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GameServerStatus {
    pub address: String,
    pub ports: Vec<GameServerPort>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GameServerPort {
    pub name: Option<String>,
    pub port: u16,
}

#[derive(Clone)]
pub struct Sdk {
    client: Client,
    base_url: String,
}

pub struct HealthCheck {
    tx: mpsc::Sender<()>,
}

impl HealthCheck {
    pub async fn send(&self, _: ()) -> anyhow::Result<()> {
        self.tx
            .send(())
            .await
            .map_err(|_| anyhow!("health check channel closed"))
    }
}

impl Sdk {
    pub async fn new(_: Option<()>, _: Option<()>) -> anyhow::Result<Self> {
        let port = env::var("AGONES_SDK_HTTP_PORT").unwrap_or_else(|_| "9358".to_string());
        let base_url = format!("http://localhost:{}", port);
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("build reqwest client")?;

        Ok(Self { client, base_url })
    }

    pub fn health_check(&self) -> HealthCheck {
        let (tx, mut rx) = mpsc::channel(1);
        let client = self.client.clone();
        let health_url = format!("{}/health", self.base_url);

        tokio::spawn(async move {
            while let Some(()) = rx.recv().await {
                if let Err(e) = client
                    .post(&health_url)
                    .json(&serde_json::json!({}))
                    .send()
                    .await
                {
                    error!(error = ?e, "Agones health check failed");
                }
            }
        });

        HealthCheck { tx }
    }

    pub async fn ready(&mut self) -> anyhow::Result<()> {
        self.client
            .post(format!("{}/ready", self.base_url))
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("agones ready request")?
            .error_for_status()
            .context("agones ready status")?;
        Ok(())
    }

    pub async fn allocate(&mut self) -> anyhow::Result<()> {
        self.client
            .post(format!("{}/allocate", self.base_url))
            .json(&serde_json::json!({}))
            .send()
            .await
            .context("agones allocate request")?
            .error_for_status()
            .context("agones allocate status")?;
        Ok(())
    }

    pub async fn get_gameserver(&self) -> anyhow::Result<GameServer> {
        let response = self
            .client
            .get(format!("{}/gameserver", self.base_url))
            .send()
            .await
            .context("get gameserver request")?;

        response
            .json::<GameServer>()
            .await
            .context("parse gameserver json")
    }
}
