//! Type-safe connection wrapper using the typestate pattern.
//!
//! `Connection<S>` wraps a Framed TCP stream and enforces at compile-time
//! that only packets valid for state `S` can be sent or received.
//!
//! Codec selection is driven by the `ConnectionState::Codec` associated type:
//! - `Handshake` → `SimpleBinRwCodec` (no sequence byte)
//! - All other states → `SequencedBinRwCodec` (validates trailing sequence byte)
//!
//! # Example
//!
//! ```ignore
//! let mut conn = Connection::<HandshakeGame>::new(stream);
//! conn.send(ControlS2c::SyncRequest { data }.into()).await?;
//!
//! let conn = conn.into_next(());
//! // Now uses SequencedBinRwCodec and only LoginS2c can be sent
//! ```

#[cfg(feature = "net-auth")]
pub mod auth_conn;
#[cfg(feature = "net-game")]
pub mod game_conn;

use crate::Sequenced;
use crate::codec::SequencedBinRwCodec;
use futures_util::{SinkExt, StreamExt};
use std::fmt::Debug;
use std::io;
use std::net::IpAddr;
use tokio::net::TcpStream;
use tokio_util::codec::{Decoder, Encoder, Framed};
use zohar_protocol::phase::PhaseId;
// ============================================================================
// Connection - Generic over ConnectionState
// ============================================================================

/// Type-safe connection wrapper. The codec is selected at compile-time
/// based on the `S::Codec` associated type.
pub struct Connection<S: ConnectionState> {
    framed: Framed<TcpStream, S::Codec>,
    state: S,
}

// ============================================================================
// NextState - Type-level transition mapping
// ============================================================================

pub trait NextState: ConnectionState {
    type Next: ConnectionState;
    type Data;

    fn transition(conn: Connection<Self>, data: Self::Data) -> Connection<Self::Next>;
}

pub type NextConnection<S> = Connection<<S as NextState>::Next>;

fn transition_with_codec<Cur, Next>(
    conn: Connection<Cur>,
    state: Next,
    codec: Next::Codec,
) -> Connection<Next>
where
    Cur: ConnectionState,
    Next: ConnectionState,
{
    let parts = conn.framed.into_parts();
    let stream = parts.io;
    let buffer = parts.read_buf;

    let mut new_parts = Framed::new(stream, codec).into_parts();
    new_parts.read_buf = buffer;

    Connection {
        framed: Framed::from_parts(new_parts),
        state,
    }
}

fn transition_default<Cur, Next>(conn: Connection<Cur>, state: Next) -> Connection<Next>
where
    Cur: ConnectionState,
    Next: ConnectionState,
    Next::Codec: Default,
{
    transition_with_codec(conn, state, Default::default())
}

fn transition_with_sequencer<Cur, Next>(conn: Connection<Cur>, state: Next) -> Connection<Next>
where
    Cur: ConnectionState<
        Codec = SequencedBinRwCodec<
            <Cur as ConnectionState>::C2sPacket,
            <Cur as ConnectionState>::S2cPacket,
        >,
    >,
    Next: ConnectionState<
        Codec = SequencedBinRwCodec<
            <Next as ConnectionState>::C2sPacket,
            <Next as ConnectionState>::S2cPacket,
        >,
    >,
{
    let parts = conn.framed.into_parts();
    let stream = parts.io;
    let buffer = parts.read_buf;
    let sequencer = parts.codec.state.sequencer;

    let codec = SequencedBinRwCodec::<
        <Next as ConnectionState>::C2sPacket,
        <Next as ConnectionState>::S2cPacket,
    >::new(Sequenced { sequencer });

    let mut new_parts = Framed::new(stream, codec).into_parts();
    new_parts.read_buf = buffer;

    Connection {
        framed: Framed::from_parts(new_parts),
        state,
    }
}
// ============================================================================
// Universal impl for ALL states - send/recv work automatically
// ============================================================================

impl<S: ConnectionState> Connection<S> {
    /// Get a reference to the current state data.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Get the phase ID for this connection state.
    pub fn phase_id(&self) -> PhaseId {
        S::PHASE_ID
    }

    /// Get mutable access to state data.
    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Send a packet - uses the state's codec type automatically.
    pub async fn send(&mut self, packet: S::S2cPacket) -> io::Result<()> {
        self.framed.send(packet).await
    }

    /// Receive a packet - uses the state's codec type automatically.
    pub async fn recv(&mut self) -> io::Result<Option<S::C2sPacket>> {
        self.framed.next().await.transpose()
    }

    /// Get local address info (IP as i32 in little-endian, port).
    pub fn local_addr_info(&self) -> io::Result<(i32, u16)> {
        let stream = self.framed.get_ref();
        let addr = stream.local_addr()?;

        let srv_ipv4_addr = match addr.ip() {
            IpAddr::V4(ip) => ip,
            IpAddr::V6(ip) => ip.to_ipv4().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrNotAvailable,
                    "Connected via IPv6 but protocol requires IPv4",
                )
            })?,
        };

        let ip_int = i32::from_le_bytes(srv_ipv4_addr.octets());
        let port = addr.port();

        Ok((ip_int, port))
    }

    /// Get peer IP as a normalized string.
    pub fn peer_ip_string(&self) -> io::Result<String> {
        let stream = self.framed.get_ref();
        let addr = stream.peer_addr()?;
        Ok(addr.ip().to_string())
    }
}

impl<S: NextState> Connection<S> {
    pub fn into_next(self, data: S::Data) -> Connection<S::Next> {
        S::transition(self, data)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Trait implemented by connection state types.
///
/// Each state defines:
/// - Which phase ID to send in SetPhase packets
/// - Which C2S/S2C packet enums are valid
/// - Which codec to use for this phase (lifted to the type level)
pub trait ConnectionState: sealed::Sealed + Sized {
    /// Wire value for SetPhase packet
    const PHASE_ID: PhaseId;

    /// Client-to-server packet type valid in this phase
    type C2sPacket: Debug;

    /// Server-to-client packet type valid in this phase
    type S2cPacket: Debug;

    /// The codec type for this connection phase.
    /// Handshake uses SimpleBinRwCodec (no sequence), others use SequencedBinRwCodec.
    type Codec: Encoder<Self::S2cPacket, Error = io::Error>
        + Decoder<Item = Self::C2sPacket, Error = io::Error>
        + Default
        + Send
        + 'static;
}

pub trait SetPhasePacket {
    fn set_phase(phase: PhaseId) -> Self;
}

pub trait ConnectionPhaseExt<S: NextState> {
    #[allow(async_fn_in_trait)]
    async fn into_next_with_phase(self, data: S::Data) -> io::Result<Connection<S::Next>>
    where
        S::S2cPacket: SetPhasePacket;
}

impl<S: NextState> ConnectionPhaseExt<S> for Connection<S> {
    async fn into_next_with_phase(self, data: S::Data) -> io::Result<Connection<S::Next>>
    where
        S::S2cPacket: SetPhasePacket,
    {
        let mut conn = self;
        let next_phase = <S as NextState>::Next::PHASE_ID;
        conn.send(S::S2cPacket::set_phase(next_phase)).await?;
        Ok(conn.into_next(data))
    }
}
