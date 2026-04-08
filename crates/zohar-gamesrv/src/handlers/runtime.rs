//! Runtime utilities for phase handlers.

use super::types::{PhaseResult, SessionEnd};
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{Interval, MissedTickBehavior};
use tracing::{Instrument, info, info_span};
use zohar_net::ConnectionState;

#[derive(Debug)]
struct Disconnect {
    reason: &'static str,
}

impl Disconnect {
    fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

impl std::fmt::Display for Disconnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for Disconnect {}

pub(crate) fn is_disconnect(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Disconnect>().is_some()
}

pub(crate) fn disconnect(reason: &'static str) -> anyhow::Error {
    anyhow::Error::new(Disconnect::new(reason))
}

pub(crate) struct PhaseEffects<S: zohar_net::connection::NextState> {
    pub send: Vec<S::S2cPacket>,
    pub transition: Option<S::Data>,
    pub disconnect: Option<&'static str>,
}

impl<S: zohar_net::connection::NextState> PhaseEffects<S> {
    pub fn empty() -> Self {
        Self {
            send: Vec::new(),
            transition: None,
            disconnect: None,
        }
    }

    pub fn send(packet: S::S2cPacket) -> Self {
        Self {
            send: vec![packet],
            transition: None,
            disconnect: None,
        }
    }

    pub fn send_many<I>(packets: I) -> Self
    where
        I: IntoIterator<Item = S::S2cPacket>,
    {
        Self {
            send: packets.into_iter().collect(),
            transition: None,
            disconnect: None,
        }
    }

    pub fn transition(data: S::Data) -> Self {
        Self {
            send: Vec::new(),
            transition: Some(data),
            disconnect: None,
        }
    }

    pub fn disconnect(reason: &'static str) -> Self {
        Self {
            send: Vec::new(),
            transition: None,
            disconnect: Some(reason),
        }
    }

    pub fn with_disconnect(mut self, reason: &'static str) -> Self {
        self.disconnect = Some(reason);
        self
    }
}

pub(crate) fn base_phase_span<S: ConnectionState>() -> tracing::Span {
    info_span!(
        "conn_phase",
        phase = ?S::PHASE_ID,
        username = tracing::field::Empty,
        player = tracing::field::Empty
    )
}

pub(crate) fn make_heartbeat_interval(interval: Duration) -> Interval {
    let mut heartbeat = tokio::time::interval(interval);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    heartbeat
}

pub(crate) async fn wait_for_server_drain(drain_rx: &mut Option<watch::Receiver<bool>>) {
    let Some(drain_rx) = drain_rx else {
        std::future::pending::<()>().await;
        return;
    };
    if *drain_rx.borrow_and_update() {
        return;
    }
    let _ = drain_rx.changed().await;
}

pub(crate) async fn run_phase<T>(
    err_msg: &'static str,
    end: SessionEnd,
    span: tracing::Span,
    fut: impl Future<Output = PhaseResult<T>>,
) -> Result<T, SessionEnd> {
    match fut.instrument(span).await {
        Ok(conn) => Ok(conn),
        Err(err) => {
            if is_disconnect(&err) {
                info!(reason = %err, "{err_msg}");
            } else {
                info!(error = ?err, "{err_msg}");
            }
            Err(end)
        }
    }
}
