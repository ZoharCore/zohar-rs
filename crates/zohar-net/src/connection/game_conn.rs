use super::{NextState, sealed};
use crate::{Connection, ConnectionState, SequencedBinRwCodec, SimpleBinRwCodec};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use zohar_domain::MapId;
use zohar_domain::appearance::PlayerVisualProfile;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::player::{
    PlayerGameplayBootstrap, PlayerId, PlayerPlaytime, PlayerRuntimeEpoch,
};
use zohar_protocol::game_pkt::{
    HandshakeGameC2s, HandshakeGameS2c, InGameC2s, InGameS2c, LoadingC2s, LoadingS2c, LoginC2s,
    LoginS2c, NetId, PhaseId, SelectC2s, SelectS2c,
};

impl Connection<HandshakeGame> {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            framed: Framed::new(stream, Default::default()),
            state: HandshakeGame,
        }
    }
}

impl NextState for HandshakeGame {
    type Next = Login;
    type Data = ();

    fn transition(conn: Connection<Self>, (): Self::Data) -> Connection<Self::Next> {
        super::transition_default(conn, Login)
    }
}

impl NextState for Login {
    type Next = Select;
    type Data = String;

    fn transition(conn: Connection<Self>, username: Self::Data) -> Connection<Self::Next> {
        super::transition_with_sequencer(conn, Select { username })
    }
}

impl Connection<Select> {
    /// Get the authenticated username.
    pub fn username(&self) -> &str {
        &self.state.username
    }
}

impl NextState for Select {
    type Next = Loading;
    type Data = SelectedPlayer;

    fn transition(conn: Connection<Self>, player_id: Self::Data) -> Connection<Self::Next> {
        let mut conn = conn;
        let username = std::mem::take(&mut conn.state.username);
        let SelectedPlayer {
            player_id,
            player_name,
        } = player_id;
        let state = Loading {
            username,
            player_id,
            player_name,
        };
        super::transition_with_sequencer(conn, state)
    }
}

impl Connection<Loading> {
    pub fn username(&self) -> &str {
        &self.state.username
    }

    pub fn player_id(&self) -> PlayerId {
        self.state.player_id
    }

    pub fn player_name(&self) -> &str {
        &self.state.player_name
    }
}

impl NextState for Loading {
    type Next = InGame;
    type Data = LoadedPlayer;

    fn transition(conn: Connection<Self>, loaded_player: Self::Data) -> Connection<Self::Next> {
        let mut conn = conn;
        let username = std::mem::take(&mut conn.state.username);
        let state = InGame {
            username,
            player_id: conn.state.player_id,
            player_name: conn.state.player_name.clone(),
            loaded_player,
        };
        super::transition_with_sequencer(conn, state)
    }
}

impl Connection<InGame> {
    pub fn username(&self) -> &str {
        &self.state.username
    }

    pub fn player_id(&self) -> PlayerId {
        self.state.player_id
    }

    pub fn player_name(&self) -> &str {
        &self.state.player_name
    }

    pub fn net_id(&self) -> NetId {
        self.state.loaded_player.net_id
    }

    pub fn entry(&self) -> &LoadedPlayer {
        &self.state.loaded_player
    }
}

impl NextState for InGame {
    type Next = Select;
    type Data = ();

    fn transition(conn: Connection<Self>, (): Self::Data) -> Connection<Self::Next> {
        let mut conn = conn;
        let username = std::mem::take(&mut conn.state.username);
        let state = Select { username };
        super::transition_with_sequencer(conn, state)
    }
}

/// Initial handshake state - time synchronization (game server).
/// No session data yet.
#[derive(Debug, Default)]
pub struct HandshakeGame;

impl sealed::Sealed for HandshakeGame {}

impl ConnectionState for HandshakeGame {
    const PHASE_ID: PhaseId = PhaseId::Handshake;

    type C2sPacket = HandshakeGameC2s;
    type S2cPacket = HandshakeGameS2c;
    type Codec = SimpleBinRwCodec<HandshakeGameC2s, HandshakeGameS2c>;
}

/// Login state - awaiting token validation.
/// No session data yet.
#[derive(Debug, Default)]
pub struct Login;

impl sealed::Sealed for Login {}

impl ConnectionState for Login {
    const PHASE_ID: PhaseId = PhaseId::Login;

    type C2sPacket = LoginC2s;
    type S2cPacket = LoginS2c;
    type Codec = SequencedBinRwCodec<LoginC2s, LoginS2c>;
}

/// Character select state - player is choosing a character.
/// Username is now available.
#[derive(Debug)]
pub struct Select {
    pub username: String,
}

impl sealed::Sealed for Select {}

impl ConnectionState for Select {
    const PHASE_ID: PhaseId = PhaseId::Select;

    type C2sPacket = SelectC2s;
    type S2cPacket = SelectS2c;
    type Codec = SequencedBinRwCodec<SelectC2s, SelectS2c>;
}

/// Loading state - player data is being loaded after character select.
/// Username and player_id are now available.
#[derive(Debug)]
pub struct Loading {
    pub username: String,
    pub player_id: PlayerId,
    pub player_name: String,
}

impl sealed::Sealed for Loading {}

impl ConnectionState for Loading {
    const PHASE_ID: PhaseId = PhaseId::Loading;

    type C2sPacket = LoadingC2s;
    type S2cPacket = LoadingS2c;
    type Codec = SequencedBinRwCodec<LoadingC2s, LoadingS2c>;
}

/// In-game state - player is fully loaded and playing.
/// Username and player_id are available.
#[derive(Debug)]
pub struct InGame {
    pub username: String,
    pub player_id: PlayerId,
    pub player_name: String,
    pub loaded_player: LoadedPlayer,
}

impl sealed::Sealed for InGame {}

impl ConnectionState for InGame {
    const PHASE_ID: PhaseId = PhaseId::InGame;

    type C2sPacket = InGameC2s;
    type S2cPacket = InGameS2c;
    type Codec = SequencedBinRwCodec<InGameC2s, InGameS2c>;
}

impl Authenticated for Select {
    fn username(&self) -> &str {
        &self.username
    }
}

impl Authenticated for Loading {
    fn username(&self) -> &str {
        &self.username
    }
}

impl Authenticated for InGame {
    fn username(&self) -> &str {
        &self.username
    }
}

impl HasPlayer for Loading {
    fn player_id(&self) -> PlayerId {
        self.player_id
    }
}

impl HasPlayer for InGame {
    fn player_id(&self) -> PlayerId {
        self.player_id
    }
}

/// Marker trait for states where the user is authenticated (has username).
pub trait Authenticated: ConnectionState {
    fn username(&self) -> &str;
}

/// Marker trait for states where a player is selected (has player_id).
pub trait HasPlayer: Authenticated {
    fn player_id(&self) -> PlayerId;
}

#[derive(Debug)]
pub struct SelectedPlayer {
    pub player_id: PlayerId,
    pub player_name: String,
}

#[derive(Debug, Clone)]
pub struct LoadedPlayer {
    pub net_id: NetId,
    pub map_id: MapId,
    pub runtime_epoch: PlayerRuntimeEpoch,
    pub playtime: PlayerPlaytime,
    pub initial_pos: LocalPos,
    pub visual_profile: PlayerVisualProfile,
    pub gameplay: PlayerGameplayBootstrap,
}

impl super::SetPhasePacket for HandshakeGameS2c {
    fn set_phase(phase: PhaseId) -> Self {
        zohar_protocol::control_pkt::ControlS2c::SetClientPhase { phase }.into()
    }
}

impl super::SetPhasePacket for LoginS2c {
    fn set_phase(phase: PhaseId) -> Self {
        zohar_protocol::control_pkt::ControlS2c::SetClientPhase { phase }.into()
    }
}

impl super::SetPhasePacket for SelectS2c {
    fn set_phase(phase: PhaseId) -> Self {
        zohar_protocol::control_pkt::ControlS2c::SetClientPhase { phase }.into()
    }
}

impl super::SetPhasePacket for LoadingS2c {
    fn set_phase(phase: PhaseId) -> Self {
        zohar_protocol::control_pkt::ControlS2c::SetClientPhase { phase }.into()
    }
}

impl super::SetPhasePacket for InGameS2c {
    fn set_phase(phase: PhaseId) -> Self {
        zohar_protocol::control_pkt::ControlS2c::SetClientPhase { phase }.into()
    }
}
