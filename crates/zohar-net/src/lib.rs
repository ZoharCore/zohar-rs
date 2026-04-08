#[cfg(not(any(feature = "net-auth", feature = "net-game")))]
compile_error!("zohar-net requires feature \"net-auth\" and/or \"net-game\"");

pub mod codec;
pub mod connection;

pub use codec::{BinRwCodec, Sequenced, SequencedBinRwCodec, SimpleBinRwCodec};
pub use connection::Connection;
#[cfg(feature = "net-game")]
pub use connection::ConnectionPhaseExt;
pub use connection::ConnectionState;
#[cfg(feature = "net-game")]
pub use connection::SetPhasePacket;
#[cfg(feature = "net-auth")]
pub use connection::auth_conn::Auth;
#[cfg(feature = "net-auth")]
pub use connection::auth_conn::HandshakeAuth;
#[cfg(feature = "net-game")]
pub use connection::game_conn::Authenticated;
#[cfg(feature = "net-game")]
pub use connection::game_conn::HandshakeGame;
#[cfg(feature = "net-game")]
pub use connection::game_conn::HasPlayer;
#[cfg(feature = "net-game")]
pub use connection::game_conn::InGame;
#[cfg(feature = "net-game")]
pub use connection::game_conn::Loading;
#[cfg(feature = "net-game")]
pub use connection::game_conn::Login;
#[cfg(feature = "net-game")]
pub use connection::game_conn::Select;
use std::fmt::{Debug, Display};
use std::future::Future;
use std::io;
use std::time::Instant;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::oneshot;
use tracing::{error, info};
use uuid::Uuid;

pub async fn listen<A, H, Fut>(addr: A, handler: H)
where
    A: ToSocketAddrs + Debug + Display,
    H: Fn(TcpStream, Instant, Uuid) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    listen_with_ready(addr, None, handler).await;
}

pub async fn listen_on<H, Fut>(listener: TcpListener, handler: H)
where
    H: Fn(TcpStream, Instant, Uuid) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    match listener.local_addr() {
        Ok(addr) => info!(?addr, "Started listening!"),
        Err(error) => info!(error = %error, "Started listening!"),
    }
    accept_loop(listener, Instant::now(), handler).await;
}

pub async fn listen_with_ready<A, H, Fut>(
    addr: A,
    ready_tx: Option<oneshot::Sender<io::Result<()>>>,
    handler: H,
) where
    A: ToSocketAddrs + Debug + Display,
    H: Fn(TcpStream, Instant, Uuid) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => {
            if let Some(ready_tx) = ready_tx {
                let _ = ready_tx.send(Ok(()));
            }
            info!(?addr, "Started listening!");
            l
        }
        Err(e) => {
            if let Some(ready_tx) = ready_tx {
                let _ = ready_tx.send(Err(io::Error::new(e.kind(), e.to_string())));
            }
            error!(error = %e, "Failed to bind listener");
            return;
        }
    };

    accept_loop(listener, Instant::now(), handler).await;
}

async fn accept_loop<H, Fut>(listener: TcpListener, server_start: Instant, handler: H)
where
    H: Fn(TcpStream, Instant, Uuid) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    loop {
        let (stream, peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                error!(error = %e, "Failed to accept connection");
                continue;
            }
        };

        let local_addr = stream.local_addr().ok().unwrap();
        let conn_id = Uuid::new_v4();

        info!(
            ?conn_id,
            ?local_addr,
            ?peer_addr,
            "New connection established"
        );

        let jh = tokio::spawn(handler(stream, server_start, conn_id));
        drop(jh) // fire-and-forget
    }
}

pub struct ShortId(pub Uuid);

impl Display for ShortId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        const START_LEN: usize = 4;
        const END_LEN: usize = 2;

        let s = self.0.simple().to_string();
        let len = s.len();

        let start = &s[..START_LEN];

        let end = &s[len - END_LEN..];

        write!(f, "{}..{}", start, end)
    }
}
