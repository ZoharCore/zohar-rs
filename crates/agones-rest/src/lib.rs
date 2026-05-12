use anyhow::Result;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObjectMeta {
    pub name: Option<String>,
    pub namespace: Option<String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub annotations: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Port {
    pub name: Option<String>,
    pub port: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Status {
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub ports: Vec<Port>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GameServer {
    #[serde(alias = "objectMeta")]
    pub object_meta: Option<ObjectMeta>,
    pub status: Option<Status>,
}

#[derive(Clone)]
pub struct Sdk {
    client: Client,
    base_url: Url,
    health_tx: mpsc::Sender<()>,
}

impl Sdk {
    pub async fn new(port: Option<u16>, _keep_alive: Option<Duration>) -> Result<Self> {
        let port = port.unwrap_or_else(|| {
            env::var("AGONES_SDK_HTTP_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(9358)
        });

        let base_url = Url::parse(&format!("http://127.0.0.1:{}/", port))?;
        let client = Client::builder().no_proxy().build()?;

        let gs_url = base_url.join("gameserver")?;
        tokio::time::timeout(Duration::from_secs(45), async {
            let mut connect_interval = tokio::time::interval(Duration::from_millis(500));
            loop {
                connect_interval.tick().await;

                let get_req = client.get(gs_url.clone());
                if let Ok(resp) = get_req.send().await {
                    if resp.error_for_status_ref().is_ok() {
                        break;
                    } else {
                        eprintln!(
                            "Agones REST new() polling got HTTP status: {}",
                            resp.status()
                        );
                    }
                }
            }
        })
        .await?;

        let (health_tx, mut health_rx) = mpsc::channel::<()>(1);

        let health_url = base_url.join("health")?;
        let health_client = client.clone();

        tokio::spawn(async move {
            while let Some(()) = health_rx.recv().await {
                if let Err(e) = health_client
                    .post(health_url.clone())
                    .header("Content-Type", "application/json")
                    .body("{}")
                    .send()
                    .await
                {
                    eprintln!("Agones REST health check failed: {}", e);
                }
            }
        });

        Ok(Self {
            client,
            base_url,
            health_tx,
        })
    }

    pub fn health_check(&self) -> mpsc::Sender<()> {
        self.health_tx.clone()
    }

    pub async fn ready(&self) -> Result<()> {
        let url = self.base_url.join("ready")?;
        self.client
            .post(url)
            .json(&serde_json::json!({}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn allocate(&self) -> Result<()> {
        let url = self.base_url.join("allocate")?;
        self.client
            .post(url)
            .json(&serde_json::json!({}))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_gameserver(&self) -> Result<GameServer> {
        let url = self.base_url.join("gameserver")?;
        let res = self.client.get(url).send().await?.error_for_status()?;
        let gs: GameServer = res.json().await?;
        Ok(gs)
    }
}
