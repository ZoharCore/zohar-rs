use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use tokio::sync::RwLock;

#[cfg(feature = "kube-resolver")]
use anyhow::Context;
#[cfg(feature = "kube-resolver")]
use kube::api::ListParams;
#[cfg(feature = "kube-resolver")]
use kube::core::{ApiResource, DynamicObject, GroupVersionKind};
#[cfg(feature = "kube-resolver")]
use kube::{Api, Client};

#[cfg(feature = "kube-resolver")]
type DirectoryClient = Client;
#[cfg(not(feature = "kube-resolver"))]
type DirectoryClient = ();

#[derive(Clone)]
pub struct ChannelDirectory {
    inner: ChannelDirectoryImpl,
}

#[derive(Clone)]
enum ChannelDirectoryImpl {
    Static(StaticChannelDirectory),
    KubeService(KubeServiceChannelDirectory),
}

impl ChannelDirectory {
    pub async fn list_channels(&self) -> anyhow::Result<Vec<ChannelEntry>> {
        match &self.inner {
            ChannelDirectoryImpl::Static(directory) => directory.list_channels().await,
            ChannelDirectoryImpl::KubeService(directory) => directory.list_channels().await,
        }
    }
}

impl From<StaticChannelDirectory> for ChannelDirectory {
    fn from(value: StaticChannelDirectory) -> Self {
        Self {
            inner: ChannelDirectoryImpl::Static(value),
        }
    }
}

impl From<KubeServiceChannelDirectory> for ChannelDirectory {
    fn from(value: KubeServiceChannelDirectory) -> Self {
        Self {
            inner: ChannelDirectoryImpl::KubeService(value),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelEntry {
    pub channel_id: u32,
    pub port: u16,
    pub ready: bool,
}

#[derive(Default, Clone)]
pub struct StaticChannelDirectory {
    entries: Arc<RwLock<HashMap<u32, ChannelEntry>>>,
}

impl StaticChannelDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn upsert(&self, channel_id: u32, port: u16, ready: bool) {
        self.entries.write().await.insert(
            channel_id,
            ChannelEntry {
                channel_id,
                port,
                ready,
            },
        );
    }

    pub async fn list_channels(&self) -> anyhow::Result<Vec<ChannelEntry>> {
        let mut entries: Vec<_> = self.entries.read().await.values().copied().collect();
        entries.sort_by_key(|entry| entry.channel_id);
        Ok(entries)
    }
}

#[derive(Clone)]
#[cfg_attr(not(feature = "kube-resolver"), allow(dead_code))]
pub struct KubeServiceChannelDirectory {
    client: Option<DirectoryClient>,
    namespace: String,
    service_selector: String,
}

impl KubeServiceChannelDirectory {
    pub fn new(
        client: Option<DirectoryClient>,
        namespace: impl Into<String>,
        service_selector: impl Into<String>,
    ) -> Self {
        Self {
            client,
            namespace: namespace.into(),
            service_selector: service_selector.into(),
        }
    }
}

#[cfg(feature = "kube-resolver")]
fn parse_service_entry(service: &DynamicObject) -> anyhow::Result<(String, ChannelEntry)> {
    let name = service
        .metadata
        .name
        .clone()
        .ok_or_else(|| anyhow!("service metadata.name missing"))?;

    let labels = service
        .metadata
        .labels
        .as_ref()
        .ok_or_else(|| anyhow!("service labels missing"))?;
    let channel_id: u32 = labels
        .get("channel")
        .ok_or_else(|| anyhow!("service label 'channel' missing"))?
        .parse()
        .with_context(|| format!("invalid service channel label for {name}"))?;

    let port_raw = service
        .data
        .get("spec")
        .and_then(|spec| spec.get("ports"))
        .and_then(|ports| ports.as_array())
        .and_then(|ports| ports.first())
        .and_then(|port| port.get("port"))
        .and_then(|port| port.as_u64())
        .ok_or_else(|| anyhow!("service spec.ports[0].port missing for {name}"))?;
    let port = u16::try_from(port_raw).context("service port out of range")?;

    Ok((
        name,
        ChannelEntry {
            channel_id,
            port,
            ready: false,
        },
    ))
}

#[cfg(feature = "kube-resolver")]
fn endpoints_ready(endpoints: &DynamicObject) -> bool {
    endpoints
        .data
        .get("subsets")
        .and_then(|subsets| subsets.as_array())
        .map(|subsets| {
            subsets.iter().any(|subset| {
                subset
                    .get("addresses")
                    .and_then(|addresses| addresses.as_array())
                    .map(|addresses| !addresses.is_empty())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

impl KubeServiceChannelDirectory {
    pub async fn list_channels(&self) -> anyhow::Result<Vec<ChannelEntry>> {
        #[cfg(not(feature = "kube-resolver"))]
        {
            Err(anyhow!("kube resolver disabled for channel directory"))
        }

        #[cfg(feature = "kube-resolver")]
        {
            let Some(client) = self.client.clone() else {
                return Err(anyhow!("kube client unavailable for channel directory"));
            };

            let svc_gvk = GroupVersionKind::gvk("", "v1", "Service");
            let svc_ar = ApiResource::from_gvk(&svc_gvk);
            let services_api: Api<DynamicObject> =
                Api::namespaced_with(client.clone(), &self.namespace, &svc_ar);

            let ep_gvk = GroupVersionKind::gvk("", "v1", "Endpoints");
            let ep_ar = ApiResource::from_gvk(&ep_gvk);
            let endpoints_api: Api<DynamicObject> =
                Api::namespaced_with(client, &self.namespace, &ep_ar);

            let services = services_api
                .list(&ListParams::default().labels(&self.service_selector))
                .await
                .with_context(|| {
                    format!(
                        "list channel entry services with selector '{}'",
                        self.service_selector
                    )
                })?;

            let mut by_channel: HashMap<u32, ChannelEntry> = HashMap::new();
            for service in &services.items {
                let (service_name, mut entry) = parse_service_entry(service)?;
                if let Some(endpoints) = endpoints_api.get_opt(&service_name).await? {
                    entry.ready = endpoints_ready(&endpoints);
                }

                by_channel
                    .entry(entry.channel_id)
                    .and_modify(|existing| {
                        // Prefer ready service if multiple entries claim the same channel.
                        if entry.ready && !existing.ready {
                            *existing = entry;
                        }
                    })
                    .or_insert(entry);
            }

            let mut entries: Vec<_> = by_channel.into_values().collect();
            entries.sort_by_key(|entry| entry.channel_id);
            Ok(entries)
        }
    }
}
