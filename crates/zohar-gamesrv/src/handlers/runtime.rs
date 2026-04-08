//! Runtime utilities for phase handlers.

use super::types::{PhaseResult, SessionEnd};
use std::future::Future;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::{Interval, MissedTickBehavior};
use tracing::{Instrument, info, info_span};
use zohar_net::ConnectionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisconnectDisposition {
    Standard,
    HandoffPrepared,
}

#[derive(Debug)]
struct Disconnect {
    reason: &'static str,
    disposition: DisconnectDisposition,
}

impl Disconnect {
    fn new(reason: &'static str) -> Self {
        Self {
            reason,
            disposition: DisconnectDisposition::Standard,
        }
    }

    fn handoff(reason: &'static str) -> Self {
        Self {
            reason,
            disposition: DisconnectDisposition::HandoffPrepared,
        }
    }
}

impl std::fmt::Display for Disconnect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.reason)
    }
}

impl std::error::Error for Disconnect {}

pub(crate) fn disconnect(reason: &'static str) -> anyhow::Error {
    anyhow::Error::new(Disconnect::new(reason))
}

pub(crate) fn handoff_disconnect(reason: &'static str) -> anyhow::Error {
    anyhow::Error::new(Disconnect::handoff(reason))
}

pub(crate) struct PhaseEffects<S: zohar_net::connection::NextState> {
    pub send: Vec<S::S2cPacket>,
    pub transition: Option<S::Data>,
    pub disconnect: Option<anyhow::Error>,
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
            disconnect: Some(disconnect(reason)),
        }
    }

    pub fn with_disconnect(mut self, reason: &'static str) -> Self {
        self.disconnect = Some(disconnect(reason));
        self
    }

    pub fn with_handoff_disconnect(mut self, reason: &'static str) -> Self {
        self.disconnect = Some(handoff_disconnect(reason));
        self
    }
}

fn session_end_for_disconnect(end: SessionEnd, disposition: DisconnectDisposition) -> SessionEnd {
    match disposition {
        DisconnectDisposition::Standard => end,
        DisconnectDisposition::HandoffPrepared => match end {
            SessionEnd::AfterLogin { username, .. } => SessionEnd::Handoff { username },
            other => other,
        },
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
            if let Some(disconnect) = err.downcast_ref::<Disconnect>() {
                info!(reason = %disconnect, "{err_msg}");
                Err(session_end_for_disconnect(end, disconnect.disposition))
            } else {
                info!(error = ?err, "{err_msg}");
                Err(end)
            }
        }
    }
}
