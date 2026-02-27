use anyhow::{Context, anyhow};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use tracing::warn;

pub(crate) async fn resolve_advertised_endpoint(
    local_addr: SocketAddr,
) -> anyhow::Result<SocketAddr> {
    let resolved = resolve_from_agones().await;
    if let Err(error) = &resolved {
        warn!(
            error = ?error,
            "Agones SDK endpoint resolution failed; falling back to local listener address"
        );
    }
    Ok(select_advertised_endpoint(local_addr, resolved))
}

fn select_advertised_endpoint(
    local_addr: SocketAddr,
    resolved: anyhow::Result<SocketAddr>,
) -> SocketAddr {
    match resolved {
        Ok(endpoint) => endpoint,
        Err(_) => local_addr,
    }
}

async fn resolve_from_agones() -> anyhow::Result<SocketAddr> {
    let mut sdk = agones::Sdk::new(None, None)
        .await
        .context("connect to agones sdk")?;
    let health = sdk.health_check();

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if health.send(()).await.is_err() {
                break;
            }
        }
    });

    sdk.ready().await.context("agones ready")?;
    sdk.allocate().await.context("agones allocate")?;

    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let gameserver = sdk
            .get_gameserver()
            .await
            .context("get gameserver from agones sdk")?;
        let Some(status) = gameserver.status else {
            if Instant::now() >= deadline {
                return Err(anyhow!("agones gameserver status missing"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        };
        let address = status.address.trim();
        let Some(port) = status.ports.first().map(|entry| entry.port as u16) else {
            if Instant::now() >= deadline {
                return Err(anyhow!("agones gameserver has no allocated ports"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        };
        if address.is_empty() {
            if Instant::now() >= deadline {
                return Err(anyhow!("agones gameserver status.address is empty"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            continue;
        }
        let ip: IpAddr = address
            .parse()
            .with_context(|| format!("invalid agones status.address '{address}'"))?;
        return Ok(SocketAddr::new(ip, port));
    }
}

#[cfg(test)]
mod tests {
    use super::select_advertised_endpoint;
    use anyhow::anyhow;
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn fallback_uses_local_listener_on_error() {
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 13000));
        let selected = select_advertised_endpoint(local, Err(anyhow!("agones unavailable")));
        assert_eq!(selected, local);
    }

    #[test]
    fn prefer_agones_endpoint_when_available() {
        let local = SocketAddr::from((Ipv4Addr::LOCALHOST, 13000));
        let resolved = SocketAddr::from((Ipv4Addr::new(10, 1, 2, 3), 23000));
        let selected = select_advertised_endpoint(local, Ok(resolved));
        assert_eq!(selected, resolved);
    }
}
