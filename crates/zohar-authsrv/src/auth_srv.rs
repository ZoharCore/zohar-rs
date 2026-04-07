use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use tokio::net::TcpStream;
use tracing::{Instrument, debug, error, info, info_span, instrument, warn};
use uuid::Uuid;
use zohar_db::{AccountsView, Auth, AuthDb};
use zohar_net::connection::auth_conn::{Auth as AuthState, HandshakeAuth as Handshake};
use zohar_net::{Connection, ConnectionState, ShortId};
use zohar_protocol::auth_pkt::{
    AuthC2s, AuthC2sSpecific, AuthS2c, AuthS2cSpecific, HandshakeAuthC2s as HandshakeC2s,
    LoginFailureReason,
};
use zohar_protocol::control_pkt::{ControlC2s, ControlS2c};
use zohar_protocol::decode_cstr;
use zohar_protocol::handshake::{HandshakeOutcome, HandshakeState};
use zohar_protocol::phase::PhaseId;
use zohar_protocol::token::TokenSigner;

// for production tune these down to avoid DOS
const ARGON2_M_COST: u32 = 256 * 1024;
const ARGON2_T_COST: u32 = 6;
const ARGON2_P_COST: u32 = 1;

fn desired_argon2() -> Argon2<'static> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, None).unwrap();
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

/// Standard result type for phase handlers.
type PhaseResult<T> = Result<T, anyhow::Error>;

/// Expected disconnect (e.g., client closed connection).
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

fn is_disconnect(err: &anyhow::Error) -> bool {
    err.downcast_ref::<Disconnect>().is_some()
}

fn disconnect(reason: &'static str) -> anyhow::Error {
    anyhow::Error::new(Disconnect::new(reason))
}

/// Convert `Option`/`Result` into a `PhaseResult` that disconnects on failure.
trait OrDisconnect<T> {
    fn or_disconnect(self) -> PhaseResult<T>;
}

impl<T> OrDisconnect<T> for Option<T> {
    fn or_disconnect(self) -> PhaseResult<T> {
        self.ok_or_else(|| disconnect("connection closed"))
    }
}

impl<T, E> OrDisconnect<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn or_disconnect(self) -> PhaseResult<T> {
        self.map_err(anyhow::Error::from)
    }
}

async fn run_phase<S, T, Fut>(err_msg: &'static str, fut: Fut) -> PhaseResult<T>
where
    S: ConnectionState,
    Fut: Future<Output = PhaseResult<T>>,
{
    let phase = S::PHASE_ID;
    let phase_span = info_span!("conn_phase", phase = ?phase);
    match fut.instrument(phase_span.clone()).await {
        Ok(value) => Ok(value),
        Err(err) => {
            if is_disconnect(&err) {
                phase_span.in_scope(|| info!(reason = %err, "{err_msg}"));
            } else {
                phase_span.in_scope(|| info!(error = ?err, "{err_msg}"));
            }
            Err(err)
        }
    }
}

#[instrument(skip_all, fields(cid = %ShortId(_conn_id)))]
pub async fn handle_conn(
    stream: TcpStream,
    server_start: Instant,
    _conn_id: Uuid,
    auth_db: Auth,
    token_signer: Arc<TokenSigner>,
) {
    info!("Auth connection handler initialized");

    let conn = Connection::<Handshake>::new(stream);

    let now = Instant::now();
    let mut handshake = HandshakeState::new(server_start, now);
    let _ = run_connection(conn, auth_db, token_signer, &mut handshake).await;

    debug!("Connection handler finished");
}

async fn run_connection(
    conn: Connection<Handshake>,
    auth_db: Auth,
    token_signer: Arc<TokenSigner>,
    handshake: &mut HandshakeState,
) -> PhaseResult<()> {
    let conn = run_phase::<Handshake, _, _>(
        "Disconnected during handshake",
        handle_handshake(conn, handshake),
    )
    .await?;

    run_phase::<AuthState, _, _>(
        "Disconnected during auth",
        handle_auth(conn, auth_db, token_signer),
    )
    .await?;

    Ok(())
}

/// Handle the handshake phase.
async fn handle_handshake(
    mut conn: Connection<Handshake>,
    handshake: &mut HandshakeState,
) -> PhaseResult<Connection<AuthState>> {
    let now = Instant::now();
    conn.send(
        ControlS2c::SetClientPhase {
            phase: PhaseId::Handshake,
        }
        .into(),
    )
    .await?;
    conn.send(
        ControlS2c::RequestHandshake {
            data: handshake.initial_sync_data(now),
        }
        .into(),
    )
    .await?;

    loop {
        let packet = conn.recv().await?.or_disconnect()?;
        match packet {
            HandshakeC2s::Control(ControlC2s::HandshakeResponse { data }) => {
                let now = Instant::now();
                let outcome = handshake.handle(data, now)?;
                match outcome {
                    HandshakeOutcome::CompletedInitial => {
                        conn.send(
                            ControlS2c::SetClientPhase {
                                phase: PhaseId::Auth,
                            }
                            .into(),
                        )
                        .await?;
                        return Ok(conn.into_next(()));
                    }
                    HandshakeOutcome::SendHandshakeSync(data) => {
                        conn.send(ControlS2c::RequestHandshake { data }.into())
                            .await?;
                    }
                    HandshakeOutcome::SendTimeSyncAck => {
                        // no-op for auth server
                    }
                }
            }
            HandshakeC2s::Control(ControlC2s::HeartbeatResponse) => {
                return Err(disconnect("heartbeat not allowed during handshake"));
            }
            HandshakeC2s::Control(ControlC2s::RequestTimeSync { .. }) => {
                return Err(disconnect("time sync request not allowed during handshake"));
            }
        }
    }
}

/// Handle the auth phase.
async fn handle_auth(
    mut conn: Connection<AuthState>,
    auth_db: Auth,
    token_signer: Arc<TokenSigner>,
) -> PhaseResult<()> {
    loop {
        let packet = conn.recv().await?.or_disconnect()?;
        match packet {
            AuthC2s::Specific(AuthC2sSpecific::RequestPasswordLogin {
                username,
                password,
                enc_key,
            }) => {
                let username = decode_cstr(&username);
                let password = decode_cstr(&password);

                let account = match auth_db.accounts().find_by_username(&username).await {
                    Ok(account) => account,
                    Err(err) => {
                        error!(error = %err, "Failed to query account");
                        let _ = conn
                            .send(AuthS2c::Specific(AuthS2cSpecific::LoginResultFail {
                                reason: LoginFailureReason::InvalidCredentials,
                            }))
                            .await;
                        continue;
                    }
                };

                let (account_username, stored_hash) = match account {
                    Some(account) => (Some(account.username), account.password_hash),
                    None => (None, dummy_password_hash().to_string()),
                };

                let check = match tokio::task::spawn_blocking(move || {
                    // CPU-bound check in separate tokio task to avoid blocking tokio runtime
                    verify_password(&stored_hash, &password)
                })
                .await
                {
                    Ok(check) => check,
                    Err(err) => {
                        error!(error = %err, "Password check task failed");
                        let _ = conn
                            .send(AuthS2c::Specific(AuthS2cSpecific::LoginResultFail {
                                reason: LoginFailureReason::InvalidCredentials,
                            }))
                            .await;
                        continue;
                    }
                };
                if !check.valid || account_username.is_none() {
                    let _ = conn
                        .send(AuthS2c::Specific(AuthS2cSpecific::LoginResultFail {
                            reason: LoginFailureReason::InvalidCredentials,
                        }))
                        .await;
                    continue;
                }
                let account_username = account_username.expect("account must exist after check");

                if let Some(upgraded_hash) = check.upgraded_hash
                    && let Err(err) = auth_db
                        .accounts()
                        .update_password(&account_username, &upgraded_hash)
                        .await
                {
                    warn!(error = %err, "Failed to upgrade password hash");
                }

                // Auth is stateless, no session check. Session conflicts are handled by game server
                let token = token_signer.issue(&account_username, enc_key);

                let _ = conn
                    .send(AuthS2c::Specific(AuthS2cSpecific::LoginResultOk {
                        token,
                        is_ok: true.into(),
                    }))
                    .await;
            }

            // Accepts heartbeat and ignores time sync/handshake replies
            AuthC2s::Control(ControlC2s::HeartbeatResponse) => {
                continue;
            }
            AuthC2s::Control(ControlC2s::RequestTimeSync { .. }) => {
                continue;
            }
            AuthC2s::Control(ControlC2s::HandshakeResponse { .. }) => {
                continue;
            }
        }
    }
}

fn needs_rehash(parsed: &PasswordHash<'_>) -> bool {
    if parsed.algorithm.as_str() != "argon2id" {
        return true;
    }

    let version_ok = parsed
        .version
        .as_ref()
        .map(|value| *value == Version::V0x13 as u32)
        .unwrap_or(false);
    if !version_ok {
        return true;
    }

    let m_cost = parsed
        .params
        .get("m")
        .and_then(|value| value.to_string().parse::<u32>().ok());
    let t_cost = parsed
        .params
        .get("t")
        .and_then(|value| value.to_string().parse::<u32>().ok());
    let p_cost = parsed
        .params
        .get("p")
        .and_then(|value| value.to_string().parse::<u32>().ok());

    !matches!(
        (m_cost, t_cost, p_cost),
        (Some(m), Some(t), Some(p))
            if m == ARGON2_M_COST && t == ARGON2_T_COST && p == ARGON2_P_COST
    )
}

struct PasswordCheck {
    valid: bool,
    upgraded_hash: Option<String>,
}

fn dummy_password_hash() -> &'static str {
    static DUMMY_HASH: OnceLock<String> = OnceLock::new();
    DUMMY_HASH.get_or_init(|| {
        let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
        desired_argon2()
            .hash_password("my_dummy_password".as_bytes(), &salt)
            .expect("failed to build dummy password hash")
            .to_string()
    })
}

fn verify_password(stored_hash: &str, password: &str) -> PasswordCheck {
    if stored_hash.starts_with("$argon2") {
        let parsed = match PasswordHash::new(stored_hash) {
            Ok(hash) => hash,
            Err(err) => {
                warn!(error = %err, "Invalid password hash format");
                return PasswordCheck {
                    valid: false,
                    upgraded_hash: None,
                };
            }
        };

        let argon2 = desired_argon2();
        let valid = argon2.verify_password(password.as_bytes(), &parsed).is_ok();
        if !valid {
            return PasswordCheck {
                valid: false,
                upgraded_hash: None,
            };
        }

        let upgraded_hash = if needs_rehash(&parsed) {
            let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
            match argon2.hash_password(password.as_bytes(), &salt) {
                Ok(hash) => Some(hash.to_string()),
                Err(err) => {
                    warn!(error = %err, "Failed to hash password for upgrade");
                    None
                }
            }
        } else {
            None
        };

        return PasswordCheck {
            valid: true,
            upgraded_hash,
        };
    }

    if stored_hash != password {
        return PasswordCheck {
            valid: false,
            upgraded_hash: None,
        };
    }

    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    match desired_argon2().hash_password(password.as_bytes(), &salt) {
        Ok(hash) => PasswordCheck {
            valid: true,
            upgraded_hash: Some(hash.to_string()),
        },
        Err(err) => {
            warn!(error = %err, "Failed to hash password for upgrade");
            PasswordCheck {
                valid: true,
                upgraded_hash: None,
            }
        }
    }
}
