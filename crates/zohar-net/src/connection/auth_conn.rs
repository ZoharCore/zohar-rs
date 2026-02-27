use super::{NextState, sealed};
use crate::{Connection, ConnectionState, SequencedBinRwCodec, SimpleBinRwCodec, connection};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use zohar_protocol::auth_pkt::{AuthC2s, AuthS2c, HandshakeAuthC2s, HandshakeAuthS2c};
use zohar_protocol::phase::PhaseId;

impl Connection<HandshakeAuth> {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            framed: Framed::new(stream, Default::default()),
            state: HandshakeAuth,
        }
    }
}

impl NextState for HandshakeAuth {
    type Next = Auth;
    type Data = ();

    fn transition(conn: Connection<Self>, (): Self::Data) -> Connection<Self::Next> {
        connection::transition_default(conn, Auth)
    }
}

/// Initial handshake state - time synchronization (auth server).
/// No session data yet.
#[derive(Debug, Default)]
pub struct HandshakeAuth;

impl sealed::Sealed for HandshakeAuth {}

impl ConnectionState for HandshakeAuth {
    const PHASE_ID: PhaseId = PhaseId::Handshake;

    type C2sPacket = HandshakeAuthC2s;
    type S2cPacket = HandshakeAuthS2c;
    type Codec = SimpleBinRwCodec<HandshakeAuthC2s, HandshakeAuthS2c>;
}

/// Auth phase - credentials are validated on auth server.
#[derive(Debug, Default)]
pub struct Auth;

impl sealed::Sealed for Auth {}

impl ConnectionState for Auth {
    const PHASE_ID: PhaseId = PhaseId::Auth;

    type C2sPacket = AuthC2s;
    type S2cPacket = AuthS2c;
    type Codec = SequencedBinRwCodec<AuthC2s, AuthS2c>;
}
