use std::collections::HashMap;
#[cfg(feature = "kube-resolver")]
use std::net::IpAddr;
use std::net::{Ipv4Addr, SocketAddr};
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
use serde_json::Value;
#[cfg(feature = "kube-resolver")]
type ResolverClient = Client;
#[cfg(not(feature = "kube-resolver"))]
type ResolverClient = ();

#[derive(Clone)]
pub struct MapEndpointResolver {
    inner: MapEndpointResolverImpl,
}

#[derive(Clone)]
enum MapEndpointResolverImpl {
    Static(StaticMapResolver),
    KubeAgones(KubeAgonesMapResolver),
}

impl MapEndpointResolver {
    pub async fn resolve(&self, channel_id: u32, map_code: &str) -> anyhow::Result<SocketAddr> {
        match &self.inner {
            MapEndpointResolverImpl::Static(resolver) => {
                resolver.resolve(channel_id, map_code).await
            }
            MapEndpointResolverImpl::KubeAgones(resolver) => {
                resolver.resolve(channel_id, map_code).await
            }
        }
    }
}

impl From<StaticMapResolver> for MapEndpointResolver {
    fn from(value: StaticMapResolver) -> Self {
        Self {
            inner: MapEndpointResolverImpl::Static(value),
        }
    }
}

impl From<KubeAgonesMapResolver> for MapEndpointResolver {
    fn from(value: KubeAgonesMapResolver) -> Self {
        Self {
            inner: MapEndpointResolverImpl::KubeAgones(value),
        }
    }
}

#[derive(Default, Clone)]
pub struct StaticMapResolver {
    endpoints: Arc<RwLock<HashMap<(u32, String), SocketAddr>>>,
}

impl StaticMapResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert(&self, channel_id: u32, map_code: impl Into<String>, endpoint: SocketAddr) {
        self.endpoints
            .write()
            .await
            .insert((channel_id, map_code.into()), endpoint);
    }

    pub async fn remove(&self, channel_id: u32, map_code: &str) {
        self.endpoints
            .write()
            .await
            .remove(&(channel_id, map_code.to_string()));
    }

    pub async fn resolve(&self, channel_id: u32, map_code: &str) -> anyhow::Result<SocketAddr> {
        let key = (channel_id, map_code.to_string());
        let Some(endpoint) = self.endpoints.read().await.get(&key).copied() else {
            return Err(anyhow!(
                "no endpoint registered for channel={} map={}",
                channel_id,
                map_code
            ));
        };
        Ok(endpoint)
    }
}

#[derive(Clone)]
#[cfg_attr(not(feature = "kube-resolver"), allow(dead_code))]
pub struct KubeAgonesMapResolver {
    client: Option<ResolverClient>,
    namespace: String,
    advertised_ipv4_override: Option<Ipv4Addr>,
    endpoint_mode: EndpointMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointMode {
    Agones,
    ServiceNodePort,
}

impl std::str::FromStr for EndpointMode {
    type Err = anyhow::Error;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "service-nodeport" | "service_nodeport" | "service" | "nodeport" => {
                Ok(Self::ServiceNodePort)
            }
            "agones" => Ok(Self::Agones),
            _ => Err(anyhow!(
                "invalid map endpoint mode '{raw}', expected one of: agones, service-nodeport"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MapResolverConfig {
    pub endpoint_mode: EndpointMode,
    pub advertised_ipv4_override: Option<Ipv4Addr>,
}

impl MapResolverConfig {
    pub fn new(endpoint_mode: EndpointMode, advertised_ipv4_override: Option<Ipv4Addr>) -> Self {
        Self {
            endpoint_mode,
            advertised_ipv4_override,
        }
    }
}

impl KubeAgonesMapResolver {
    pub fn new(
        client: Option<ResolverClient>,
        namespace: impl Into<String>,
        config: MapResolverConfig,
    ) -> Self {
        Self {
            client,
            namespace: namespace.into(),
            advertised_ipv4_override: config.advertised_ipv4_override,
            endpoint_mode: config.endpoint_mode,
        }
    }
}

#[cfg(feature = "kube-resolver")]
fn parse_gameserver_endpoint(gs: &DynamicObject) -> anyhow::Result<SocketAddr> {
    let status = gs
        .data
        .get("status")
        .ok_or_else(|| anyhow!("gameserver is missing status"))?;

    let address = status
        .get("address")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("gameserver status.address missing"))?;
    let ports = status
        .get("ports")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("gameserver status.ports missing"))?;
    let first_port = ports
        .first()
        .ok_or_else(|| anyhow!("gameserver status.ports empty"))?;
    let port_raw = first_port
        .get("port")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("gameserver status.ports[0].port missing"))?;
    let port = u16::try_from(port_raw).context("gameserver port out of range")?;
    let ip: IpAddr = address
        .parse()
        .with_context(|| format!("invalid gameserver status.address '{address}'"))?;
    Ok(SocketAddr::new(ip, port))
}

#[cfg(feature = "kube-resolver")]
fn parse_service_node_port(svc: &DynamicObject) -> anyhow::Result<u16> {
    let spec = svc
        .data
        .get("spec")
        .ok_or_else(|| anyhow!("service is missing spec"))?;
    let ports = spec
        .get("ports")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("service spec.ports missing"))?;
    let first_port = ports
        .first()
        .ok_or_else(|| anyhow!("service spec.ports empty"))?;

    let node_port = first_port
        .get("nodePort")
        .and_then(Value::as_u64)
        .or_else(|| first_port.get("port").and_then(Value::as_u64))
        .ok_or_else(|| anyhow!("service spec.ports[0].nodePort/port missing"))?;

    u16::try_from(node_port).context("service nodePort out of range")
}

#[cfg(feature = "kube-resolver")]
fn parse_service_lb_ip(svc: &DynamicObject) -> Option<Ipv4Addr> {
    let ingress = svc
        .data
        .get("status")
        .and_then(|status| status.get("loadBalancer"))
        .and_then(|lb| lb.get("ingress"))
        .and_then(Value::as_array)?;
    let first = ingress.first()?;
    first
        .get("ip")
        .and_then(Value::as_str)
        .and_then(|ip| ip.parse::<Ipv4Addr>().ok())
}

impl KubeAgonesMapResolver {
    pub async fn resolve(&self, channel_id: u32, map_code: &str) -> anyhow::Result<SocketAddr> {
        #[cfg(not(feature = "kube-resolver"))]
        {
            return Err(anyhow!(
                "kube resolver disabled for channel={} map={}",
                channel_id,
                map_code
            ));
        }

        #[cfg(feature = "kube-resolver")]
        {
            let Some(client) = self.client.clone() else {
                return Err(anyhow!(
                    "kube client unavailable for channel={} map={}",
                    channel_id,
                    map_code
                ));
            };

            match self.endpoint_mode {
                EndpointMode::Agones => {
                    let gvk = GroupVersionKind::gvk("agones.dev", "v1", "GameServer");
                    let ar = ApiResource::from_gvk(&gvk);
                    let api: Api<DynamicObject> =
                        Api::namespaced_with(client, &self.namespace, &ar);
                    let selector = format!("channel={},map={}", channel_id, map_code);

                    let list = api
                        .list(&ListParams::default().labels(&selector))
                        .await
                        .with_context(|| {
                            format!("list agones gameservers with selector '{selector}'")
                        })?;
                    let gs = list.items.first().ok_or_else(|| {
                        anyhow!(
                            "no agones gameserver found for channel={} map={}",
                            channel_id,
                            map_code
                        )
                    })?;
                    let mut endpoint = parse_gameserver_endpoint(gs)?;
                    if let Some(ip) = self.advertised_ipv4_override {
                        endpoint.set_ip(IpAddr::V4(ip));
                    }
                    Ok(endpoint)
                }
                EndpointMode::ServiceNodePort => {
                    let gvk = GroupVersionKind::gvk("", "v1", "Service");
                    let ar = ApiResource::from_gvk(&gvk);
                    let api: Api<DynamicObject> =
                        Api::namespaced_with(client, &self.namespace, &ar);
                    let selector = format!(
                        "app.kubernetes.io/component=map-endpoint,channel={},map={}",
                        channel_id, map_code
                    );
                    let list = api
                        .list(&ListParams::default().labels(&selector))
                        .await
                        .with_context(|| {
                            format!("list map endpoint services with selector '{selector}'")
                        })?;
                    let svc = list.items.first().ok_or_else(|| {
                        anyhow!(
                            "no map endpoint service found for channel={} map={}",
                            channel_id,
                            map_code
                        )
                    })?;
                    let port = parse_service_node_port(svc)?;
                    let ip = self
                        .advertised_ipv4_override
                        .or_else(|| parse_service_lb_ip(svc))
                        .ok_or_else(|| {
                            anyhow!(
                                "service-nodeport mode requires ZOHAR_MAP_ADVERTISE_IP or service loadBalancer ip"
                            )
                        })?;
                    Ok(SocketAddr::new(IpAddr::V4(ip), port))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "kube-resolver")]
    use kube::core::ObjectMeta;
    #[cfg(feature = "kube-resolver")]
    use serde_json::json;

    #[tokio::test]
    async fn static_resolver_roundtrip() {
        let resolver = StaticMapResolver::new();
        let endpoint: SocketAddr = "127.0.0.1:13000".parse().expect("parse");
        resolver.insert(1, "zohar_map_a1", endpoint).await;
        let resolved = resolver.resolve(1, "zohar_map_a1").await.expect("resolve");
        assert_eq!(resolved, endpoint);
    }

    #[cfg(feature = "kube-resolver")]
    #[test]
    fn parse_gameserver_endpoint_reads_status_address_and_port() {
        let gvk = GroupVersionKind::gvk("agones.dev", "v1", "GameServer");
        let ar = ApiResource::from_gvk(&gvk);
        let mut gs = DynamicObject::new("core-ch1-town1", &ar);
        gs.metadata = ObjectMeta::default();
        gs.data = json!({
            "status": {
                "address": "127.0.0.1",
                "ports": [{"port": 13001}]
            }
        });

        let endpoint = parse_gameserver_endpoint(&gs).expect("endpoint");
        assert_eq!(endpoint, "127.0.0.1:13001".parse().expect("parse"));
    }

    #[cfg(feature = "kube-resolver")]
    #[test]
    fn parse_service_node_port_reads_node_port() {
        let gvk = GroupVersionKind::gvk("", "v1", "Service");
        let ar = ApiResource::from_gvk(&gvk);
        let mut svc = DynamicObject::new("map-endpoint", &ar);
        svc.metadata = ObjectMeta::default();
        svc.data = json!({
            "spec": {
                "ports": [{"port": 13000, "nodePort": 31749}]
            }
        });

        let node_port = parse_service_node_port(&svc).expect("node port");
        assert_eq!(node_port, 31749);
    }
}
