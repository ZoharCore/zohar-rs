#[cfg(feature = "auth")]
pub mod auth_pkt;
#[cfg(any(feature = "auth", feature = "game"))]
pub mod control_pkt;
#[cfg(feature = "game")]
pub mod game_pkt;
#[cfg(any(feature = "auth", feature = "game"))]
pub mod handshake;
#[cfg(any(feature = "auth", feature = "game"))]
pub mod phase;
#[cfg(any(feature = "auth", feature = "game"))]
pub mod pkt_seq;

#[cfg(feature = "token")]
pub mod token;

#[cfg(any(feature = "auth", feature = "game"))]
mod route_packets;

pub fn decode_cstr(bytes: &[u8]) -> String {
    bytes
        .split(|&b| b == 0)
        .next()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default()
}
