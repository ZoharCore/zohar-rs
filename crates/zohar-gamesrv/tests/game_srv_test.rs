//! Integration tests for the game server handler chain.
//!
//! Tests use phase-specific packet types:
//! - SimpleBinRwCodec for Handshake phase (no sequence bytes)  
//! - For Login phase, tests manually append sequence bytes since SequencedBinRwCodec
//!   only validates on decode, not encode (protocol asymmetry: C2s has seq, S2c doesn't)

use binrw::BinWrite;
use futures_util::{SinkExt, StreamExt};
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Barrier;
use tokio::time::timeout;
use tokio_util::codec::{Framed, FramedParts};
use zohar_content::types::ContentCatalog;
use zohar_content::types::empires::{Empire as ContentEmpire, EmpireStartConfig};
use zohar_content::types::maps::ContentMap;
use zohar_db::{Game, GameDb, PlayersView, ProfilesView, SessionsView, postgres_backend};
use zohar_domain::Empire as DomainEmpire;
use zohar_domain::MapId;
use zohar_domain::entity::EntityId;
use zohar_domain::entity::player::{PlayerBaseAppearance, PlayerClass, PlayerGender};
use zohar_gamesrv::infra::{
    ChannelDirectory, ClusterEventBus, MapEndpointResolver, StaticChannelDirectory,
    StaticMapResolver, in_process_cluster_event_bus,
};
use zohar_gamesrv::{ContentCoords, EmpireStartMaps, GameContext, GatewayContext};
use zohar_net::SimpleBinRwCodec;
use zohar_protocol::game_pkt::handshake::{HandshakeGameC2s, HandshakeGameS2c};
use zohar_protocol::game_pkt::ingame::system::SystemS2c;
use zohar_protocol::game_pkt::ingame::{InGameC2s, InGameS2c};
use zohar_protocol::game_pkt::loading::{LoadingC2s, LoadingC2sSpecific, LoadingS2c};
use zohar_protocol::game_pkt::login::{
    LoginC2s, LoginC2sSpecific, LoginFailReason, LoginS2c, LoginS2cSpecific,
};
use zohar_protocol::game_pkt::select::{
    PlayerSelectSlot, SelectC2s, SelectC2sSpecific, SelectS2c, SelectS2cSpecific,
};
use zohar_protocol::game_pkt::{ControlC2s, ControlS2c, PacketSequencer};
use zohar_protocol::handshake::{HandshakeSyncData, WireDeltaMillis, WireMillis32};
use zohar_protocol::phase::PhaseId;
use zohar_protocol::token::TokenSigner;

const TEST_USERNAME_PREFIX: &str = "test_user";
static NEXT_TEST_USER: AtomicU32 = AtomicU32::new(1);
static NEXT_LOGIN_KEY: AtomicU32 = AtomicU32::new(1000);

fn has_test_db_url() -> bool {
    std::env::var("ZOHAR_TEST_DATABASE_URL").is_ok()
}

fn test_token_signer() -> Arc<TokenSigner> {
    Arc::new(TokenSigner::new(
        b"test-auth-token-secret".to_vec(),
        Duration::from_secs(30),
    ))
}

/// Test that concurrent login attempts are properly serialized by session lock.
/// Only one client should succeed, all others should fail with AlreadyLoggedIn.
#[tokio::test]
async fn test_concurrent_game_logins_race_condition() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (addr, _resolver, ctx, username) = setup_test_env().await?;

    let client_count = 10;
    let barrier = Arc::new(Barrier::new(client_count));
    let mut handles = Vec::with_capacity(client_count);
    let enc_key = [0; 16];

    for i in 0..client_count {
        let barrier = barrier.clone();
        let username = username.clone();
        let login_key = issue_login_key(&ctx.db, &username).await?;

        handles.push(tokio::spawn(async move {
            run_game_client(
                addr,
                barrier,
                i,
                username,
                login_key,
                enc_key,
                Duration::from_millis(300),
            )
            .await
        }));
    }

    let mut successes = 0;
    let mut failures = 0;

    for handle in handles {
        match handle.await?? {
            LoginResult::Success => successes += 1,
            LoginResult::Fail => failures += 1,
        }
    }

    assert_eq!(successes, 1, "Exactly one client should succeed");
    assert_eq!(failures, client_count - 1, "All other clients should fail");

    Ok(())
}

enum LoginResult {
    Success,
    Fail,
}

/// Test that sending a Login packet during Handshake phase causes connection to close.
#[tokio::test]
async fn test_phase_verification_failure() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (addr, _resolver, ctx, username) = setup_test_env().await?;
    let valid_token = issue_login_key(&ctx.db, &username).await?;

    let stream = TcpStream::connect(addr).await?;

    // Use handshake codec first
    let mut framed = Framed::new(
        stream,
        SimpleBinRwCodec::<HandshakeGameS2c, HandshakeGameC2s>::default(),
    );

    // Wait for SetPhase(Handshake)
    let packet = framed
        .next()
        .await
        .ok_or(anyhow::anyhow!("Stream closed early"))??;

    if !matches!(
        packet,
        HandshakeGameS2c::Control(ControlS2c::SetClientPhase {
            phase: PhaseId::Handshake
        })
    ) {
        anyhow::bail!("Expected SetPhase(Handshake), got {:?}", packet);
    }

    // Get underlying stream and switch to Login codec to send wrong-phase packet
    // Note: We use SimpleBinRwCodec here since server immediately rejects wrong-phase packet
    let stream = framed.into_inner();
    let mut framed = Framed::new(stream, SimpleBinRwCodec::<LoginS2c, LoginC2s>::default());

    // Send TokenLoginRequest during Handshake phase (should be rejected)
    framed
        .send(LoginC2s::Specific(LoginC2sSpecific::RequestTokenLogin {
            username: encode_username(&username),
            token: valid_token,
            enc_key: [0; 16],
        }))
        .await?;

    // Expect stream closure or error.
    // The server may still have an in-flight handshake control packet queued
    // (RequestHandshake) before it processes our wrong-phase payload.
    loop {
        let result = timeout(Duration::from_secs(1), framed.next()).await;
        match result {
            Err(_) => anyhow::bail!("Timed out waiting for phase-rejection disconnect"),
            Ok(None) => return Ok(()),         // Success: stream closed
            Ok(Some(Err(_))) => return Ok(()), // Success: error
            Ok(Some(Ok(pkt))) => {
                if matches!(
                    pkt,
                    LoginS2c::Control(ControlS2c::TimeSyncResponse)
                        | LoginS2c::Control(ControlS2c::RequestHandshake { .. })
                ) {
                    continue;
                }
                anyhow::bail!("Expected stream closure/error, got: {:?}", pkt);
            }
        }
    }
}

#[tokio::test]
async fn test_select_player_choices_uses_local_endpoint_fields() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (addr, _resolver, ctx, username) = setup_test_env_with_options(1, true).await?;
    let valid_token = issue_login_key(&ctx.db, &username).await?;

    let (mut select, mut sequencer) = connect_through_login(addr, &username, valid_token, [0; 16])
        .await?
        .ok_or(anyhow::anyhow!("login unexpectedly failed"))?;

    let players = await_set_player_choices(&mut select).await?;
    let first = players[0].clone();
    assert_ne!(first.db_id, 0, "expected seeded character in slot 0");
    assert_eq!(first.srv_ipv4_addr, i32::from_le_bytes([127, 0, 0, 1]));
    assert_eq!(first.srv_port, addr.port());

    send_sequenced(
        select.get_mut(),
        &SelectC2s::Specific(SelectC2sSpecific::SubmitPlayerChoice {
            slot: PlayerSelectSlot::First,
        }),
        &mut sequencer,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn test_login_key_survives_select_reconnect() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (addr, _resolver, ctx, username) = setup_test_env_with_options(1, true).await?;
    let valid_token = issue_login_key(&ctx.db, &username).await?;

    // First LOGIN2 (lobby/select connection)
    let (select, _sequencer) = connect_through_login(addr, &username, valid_token, [0; 16])
        .await?
        .ok_or(anyhow::anyhow!("first login unexpectedly failed"))?;
    drop(select);

    // Give the server task a brief moment to run session cleanup on disconnect.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Auth session must still exist for the second LOGIN2 (direct-enter reconnect).
    assert!(
        session_has_login_token(&ctx.db, &username, valid_token).await?,
        "auth session was consumed too early"
    );

    let reconnect = connect_through_login(addr, &username, valid_token, [0; 16]).await?;
    assert!(
        reconnect.is_some(),
        "second LOGIN2 with same token should succeed after select reconnect"
    );

    Ok(())
}

#[tokio::test]
async fn test_gateway_fetch_channel_list_returns_all_discovered_channels() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (_core_addr, _resolver, ctx, _username) = setup_test_env_with_options(1, true).await?;
    let (gateway_addr, channel_dir) = spawn_gateway_for_existing_user(&ctx, 1).await?;

    channel_dir.upsert(2, 13010, true).await;
    channel_dir.upsert(1, 13000, false).await;

    let stream = TcpStream::connect(gateway_addr).await?;
    let mut framed = Framed::new(
        stream,
        SimpleBinRwCodec::<HandshakeGameS2c, HandshakeGameC2s>::default(),
    );

    let mut sent_fetch = false;
    loop {
        let packet = timeout(Duration::from_secs(3), framed.next())
            .await
            .map_err(|_| anyhow::anyhow!("timed out waiting during gateway handshake"))?
            .ok_or(anyhow::anyhow!("gateway stream closed"))??;

        match packet {
            HandshakeGameS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Handshake,
            }) => {
                if !sent_fetch {
                    framed
                        .send(HandshakeGameC2s::Specific(
                            zohar_protocol::game_pkt::handshake::HandshakeGameC2sSpecific::FetchChannelList,
                        ))
                        .await?;
                    sent_fetch = true;
                }
            }
            HandshakeGameS2c::Control(ControlS2c::RequestHandshake { data }) => {
                let reply_data = HandshakeSyncData {
                    handshake: data.handshake,
                    time: WireMillis32::from(data.time.as_duration()),
                    delta: WireDeltaMillis::from(Duration::ZERO),
                };
                framed
                    .send(HandshakeGameC2s::Control(ControlC2s::HandshakeResponse {
                        data: reply_data,
                    }))
                    .await?;
            }
            HandshakeGameS2c::Specific(
                zohar_protocol::game_pkt::handshake::HandshakeGameS2cSpecific::ChannelListResponse {
                    statuses,
                    is_ok,
                    ..
                },
            ) => {
                assert_eq!(is_ok, 1);
                assert_eq!(statuses.len(), 2);
                assert_eq!(statuses[0].srv_port, 13000);
                assert_eq!(u8::from(statuses[0].status), 0);
                assert_eq!(statuses[1].srv_port, 13010);
                assert_eq!(u8::from(statuses[1].status), 2);
                return Ok(());
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_gateway_login_does_not_block_core_resume() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (core_addr, _resolver, ctx, username) = setup_test_env_with_options(1, true).await?;
    let (gateway_addr, _channel_dir) = spawn_gateway_for_existing_user(&ctx, 1).await?;

    let valid_token = issue_login_key(&ctx.db, &username).await?;
    let gateway_login =
        connect_through_login(gateway_addr, &username, valid_token, [0; 16]).await?;
    assert!(gateway_login.is_some(), "gateway login should succeed");
    if let Some((select, _)) = gateway_login {
        drop(select);
    }

    let reconnect = connect_through_login(core_addr, &username, valid_token, [0; 16]).await?;
    assert!(
        reconnect.is_some(),
        "core login should still succeed after gateway browse login"
    );
    Ok(())
}

#[tokio::test]
async fn test_gateway_rejects_submit_player_choice() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (_core_addr, _resolver, ctx, username) = setup_test_env_with_options(1, true).await?;
    let (gateway_addr, _channel_dir) = spawn_gateway_for_existing_user(&ctx, 1).await?;

    let valid_token = issue_login_key(&ctx.db, &username).await?;
    let (mut select, mut sequencer) =
        connect_through_login(gateway_addr, &username, valid_token, [0; 16])
            .await?
            .ok_or(anyhow::anyhow!("gateway login unexpectedly failed"))?;
    let _ = await_set_player_choices(&mut select).await?;

    send_sequenced(
        select.get_mut(),
        &SelectC2s::Specific(SelectC2sSpecific::SubmitPlayerChoice {
            slot: PlayerSelectSlot::First,
        }),
        &mut sequencer,
    )
    .await?;

    let result = timeout(Duration::from_secs(2), select.next()).await;
    match result {
        Ok(None) | Ok(Some(Err(_))) => Ok(()),
        Ok(Some(Ok(pkt))) => {
            anyhow::bail!("expected disconnect after submit choice, got {:?}", pkt)
        }
        Err(_) => anyhow::bail!("timed out waiting for gateway disconnect after submit choice"),
    }
}

#[tokio::test]
async fn test_select_choices_survive_missing_map_route() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let (addr, resolver, ctx, username) = setup_test_env_with_options(1, true).await?;
    let valid_token = issue_login_key(&ctx.db, &username).await?;

    resolver.remove(1, "zohar_map_a1").await;

    let (mut select, _sequencer) = connect_through_login(addr, &username, valid_token, [0; 16])
        .await?
        .ok_or(anyhow::anyhow!("login unexpectedly failed"))?;

    let players = await_set_player_choices(&mut select).await?;
    let first = players[0].clone();
    assert_ne!(first.db_id, 0, "expected seeded character in slot 0");
    assert_eq!(first.srv_ipv4_addr, 0);
    assert_eq!(first.srv_port, 0);

    Ok(())
}

#[tokio::test]
async fn test_ingame_set_channel_info_uses_configured_channel() -> anyhow::Result<()> {
    if !has_test_db_url() {
        return Ok(());
    }
    let _ = tracing_subscriber::fmt::try_init();
    let configured_channel_id = 7;
    let (addr, _resolver, ctx, username) =
        setup_test_env_with_options(configured_channel_id, true).await?;
    let valid_token = issue_login_key(&ctx.db, &username).await?;

    let (mut select, mut sequencer) = connect_through_login(addr, &username, valid_token, [0; 16])
        .await?
        .ok_or(anyhow::anyhow!("login unexpectedly failed"))?;
    let _ = await_set_player_choices(&mut select).await?;

    send_sequenced(
        select.get_mut(),
        &SelectC2s::Specific(SelectC2sSpecific::SubmitPlayerChoice {
            slot: PlayerSelectSlot::First,
        }),
        &mut sequencer,
    )
    .await?;
    await_phase_transition_select(&mut select, PhaseId::Loading).await?;

    let mut loading = switch_select_to_loading(select);
    send_sequenced(
        loading.get_mut(),
        &LoadingC2s::Specific(LoadingC2sSpecific::SignalLoadingComplete),
        &mut sequencer,
    )
    .await?;
    await_phase_transition_loading(&mut loading, PhaseId::InGame).await?;

    let mut ingame = switch_loading_to_ingame(loading);
    let channel = await_channel_info(&mut ingame).await?;
    assert_eq!(channel, configured_channel_id as u8);

    Ok(())
}

// ============================================================================
// Test Helpers
// ============================================================================

async fn setup_test_env() -> anyhow::Result<(
    std::net::SocketAddr,
    Arc<StaticMapResolver>,
    Arc<GameContext>,
    String,
)> {
    setup_test_env_with_options(1, false).await
}

async fn setup_test_env_with_options(
    channel_id: u32,
    seed_player: bool,
) -> anyhow::Result<(
    std::net::SocketAddr,
    Arc<StaticMapResolver>,
    Arc<GameContext>,
    String,
)> {
    let username = unique_test_username();
    let db_url = std::env::var("ZOHAR_TEST_DATABASE_URL").map_err(|_| {
        anyhow::anyhow!("ZOHAR_TEST_DATABASE_URL must be set for game server tests")
    })?;
    let game_db = postgres_backend::open_game_db(&db_url).await?;
    game_db.profiles().get_or_create(&username).await?;
    game_db
        .profiles()
        .update_empire(&username, DomainEmpire::Red)
        .await?;
    if seed_player {
        seed_player_slot_zero(&game_db, &username).await?;
    }

    let coords = Arc::new(ContentCoords::from_catalog(&test_content_catalog())?);
    let map_code = coords
        .map_code_by_id(MapId::new(1))
        .ok_or_else(|| anyhow::anyhow!("missing map code for map id 1"))?
        .to_string();
    let (map_events, inbound_rx) = zohar_sim::MapEventSender::channel_pair(256);
    std::thread::spawn(move || {
        let mut next_net_id = 1_u32;
        while let Ok(event) = inbound_rx.recv() {
            if let zohar_sim::InboundEvent::ReserveNetId { reply, .. } = event {
                let _ = reply.send(EntityId(next_net_id));
                next_net_id = next_net_id.saturating_add(1);
            }
        }
    });

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let resolver = Arc::new(StaticMapResolver::new());
    resolver.insert(channel_id, map_code.clone(), addr).await;

    let map_resolver: Arc<MapEndpointResolver> = Arc::new(resolver.as_ref().clone().into());
    let cluster_events: Arc<ClusterEventBus> = in_process_cluster_event_bus();

    let ctx = Arc::new(GameContext {
        db: game_db,
        token_signer: test_token_signer(),
        login_token_idle_ttl: Duration::from_secs(7 * 24 * 60 * 60),
        coords,
        heartbeat_interval: Duration::from_secs(60),
        server_id: "GAME_SERVER_1".to_string(),
        active_session_stale_threshold: Duration::from_secs(60),
        channel_id,
        map_events,
        advertised_endpoint: addr,
        map_code,
        map_resolver,
        cluster_events,
    });

    let server_ctx = ctx.clone();
    let server_start = std::time::Instant::now();
    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let ctx = server_ctx.clone();
                let conn_id = uuid::Uuid::new_v4();
                tokio::spawn(zohar_gamesrv::handlers::handle_conn(
                    socket,
                    server_start,
                    conn_id,
                    ctx,
                ));
            }
        }
    });

    Ok((addr, resolver, ctx, username))
}

async fn spawn_gateway_for_existing_user(
    core_ctx: &Arc<GameContext>,
    channel_id: u32,
) -> anyhow::Result<(std::net::SocketAddr, Arc<StaticChannelDirectory>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let resolver = Arc::new(StaticMapResolver::new());
    resolver
        .insert(
            channel_id,
            core_ctx.map_code.clone(),
            core_ctx.advertised_endpoint,
        )
        .await;

    let channel_dir = Arc::new(StaticChannelDirectory::new());
    channel_dir.upsert(channel_id, addr.port(), true).await;

    let map_resolver: Arc<MapEndpointResolver> = Arc::new(resolver.as_ref().clone().into());
    let channel_directory: Arc<ChannelDirectory> = Arc::new(channel_dir.as_ref().clone().into());

    let gateway_ctx = Arc::new(GatewayContext {
        db: core_ctx.db.clone(),
        token_signer: test_token_signer(),
        login_token_idle_ttl: Duration::from_secs(7 * 24 * 60 * 60),
        empire_start_maps: EmpireStartMaps::default(),
        heartbeat_interval: Duration::from_secs(60),
        channel_id,
        advertised_endpoint: addr,
        map_resolver,
        channel_directory,
    });

    let server_ctx = gateway_ctx.clone();
    let server_start = std::time::Instant::now();
    tokio::spawn(async move {
        loop {
            if let Ok((socket, _)) = listener.accept().await {
                let ctx = server_ctx.clone();
                let conn_id = uuid::Uuid::new_v4();
                tokio::spawn(zohar_gamesrv::handlers::handle_conn_gateway(
                    socket,
                    server_start,
                    conn_id,
                    ctx,
                ));
            }
        }
    });

    Ok((addr, channel_dir))
}

async fn seed_player_slot_zero<DB: GameDb>(db: &DB, username: &str) -> anyhow::Result<()> {
    let _ = db
        .players()
        .create(
            username,
            0,
            "wire_player",
            PlayerClass::Warrior,
            PlayerGender::Male,
            PlayerBaseAppearance::VariantA,
            4,
            4,
            4,
            4,
        )
        .await?;
    Ok(())
}

async fn issue_login_key<DB: GameDb>(db: &DB, username: &str) -> anyhow::Result<u32> {
    let login_key = NEXT_LOGIN_KEY.fetch_add(1, Ordering::Relaxed);
    db.sessions().set_login_token(username, login_key).await?;
    Ok(login_key)
}

async fn session_has_login_token(
    db: &Game,
    username: &str,
    login_key: u32,
) -> anyhow::Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM game.sessions WHERE username = $1 AND login_token = $2)",
    )
    .bind(username)
    .bind(i64::from(login_key))
    .fetch_one(db.pool())
    .await?;
    Ok(exists)
}

async fn send_sequenced<T: BinWrite>(
    stream: &mut TcpStream,
    packet: &T,
    sequencer: &mut PacketSequencer,
) -> anyhow::Result<()>
where
    for<'a> <T as BinWrite>::Args<'a>: Default,
{
    let mut buf = Cursor::new(Vec::new());
    packet.write_le(&mut buf)?;
    let mut bytes = buf.into_inner();
    bytes.push(sequencer.next());
    stream.write_all(&bytes).await?;
    Ok(())
}

async fn connect_through_login(
    addr: std::net::SocketAddr,
    username: &str,
    token: u32,
    enc_key: [u8; 16],
) -> anyhow::Result<
    Option<(
        Framed<TcpStream, SimpleBinRwCodec<SelectS2c, SelectC2s>>,
        PacketSequencer,
    )>,
> {
    let stream = TcpStream::connect(addr).await?;

    let mut handshake = Framed::new(
        stream,
        SimpleBinRwCodec::<HandshakeGameS2c, HandshakeGameC2s>::default(),
    );

    loop {
        let packet = handshake
            .next()
            .await
            .ok_or(anyhow::anyhow!("Stream closed during handshake"))??;

        match packet {
            HandshakeGameS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Handshake,
            }) => {}
            HandshakeGameS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Login,
            }) => break,
            HandshakeGameS2c::Control(ControlS2c::RequestHandshake { data }) => {
                let reply_data = HandshakeSyncData {
                    handshake: data.handshake,
                    time: WireMillis32::from(data.time.as_duration()),
                    delta: WireDeltaMillis::from(Duration::ZERO),
                };

                handshake
                    .send(HandshakeGameC2s::Control(ControlC2s::HandshakeResponse {
                        data: reply_data,
                    }))
                    .await?;
            }
            other => anyhow::bail!("Unexpected handshake packet: {:?}", other),
        }
    }

    let stream = handshake.into_inner();
    let mut login = Framed::new(stream, SimpleBinRwCodec::<LoginS2c, LoginC2s>::default());
    let mut sequencer = PacketSequencer::default();

    send_sequenced(
        login.get_mut(),
        &LoginC2s::Specific(LoginC2sSpecific::RequestTokenLogin {
            username: encode_username(username),
            token,
            enc_key,
        }),
        &mut sequencer,
    )
    .await?;

    loop {
        let packet = login
            .next()
            .await
            .ok_or(anyhow::anyhow!("Stream closed during login reply"))??;

        match packet {
            LoginS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Select,
            }) => {
                let select = switch_login_to_select(login);
                return Ok(Some((select, sequencer)));
            }
            LoginS2c::Specific(LoginS2cSpecific::LoginResultFail { reason }) => {
                return match reason {
                    LoginFailReason::AlreadyLoggedIn
                    | LoginFailReason::InvalidCredentials
                    | LoginFailReason::BlockedAccount => Ok(None),
                };
            }
            _ => continue,
        }
    }
}

fn switch_login_to_select(
    framed: Framed<TcpStream, SimpleBinRwCodec<LoginS2c, LoginC2s>>,
) -> Framed<TcpStream, SimpleBinRwCodec<SelectS2c, SelectC2s>> {
    let old_parts = framed.into_parts();
    let mut new_parts = FramedParts::new(old_parts.io, SimpleBinRwCodec::default());
    new_parts.read_buf = old_parts.read_buf;
    new_parts.write_buf = old_parts.write_buf;
    Framed::from_parts(new_parts)
}

fn switch_select_to_loading(
    framed: Framed<TcpStream, SimpleBinRwCodec<SelectS2c, SelectC2s>>,
) -> Framed<TcpStream, SimpleBinRwCodec<LoadingS2c, LoadingC2s>> {
    let old_parts = framed.into_parts();
    let mut new_parts = FramedParts::new(old_parts.io, SimpleBinRwCodec::default());
    new_parts.read_buf = old_parts.read_buf;
    new_parts.write_buf = old_parts.write_buf;
    Framed::from_parts(new_parts)
}

fn switch_loading_to_ingame(
    framed: Framed<TcpStream, SimpleBinRwCodec<LoadingS2c, LoadingC2s>>,
) -> Framed<TcpStream, SimpleBinRwCodec<InGameS2c, InGameC2s>> {
    let old_parts = framed.into_parts();
    let mut new_parts = FramedParts::new(old_parts.io, SimpleBinRwCodec::default());
    new_parts.read_buf = old_parts.read_buf;
    new_parts.write_buf = old_parts.write_buf;
    Framed::from_parts(new_parts)
}

async fn await_set_player_choices(
    framed: &mut Framed<TcpStream, SimpleBinRwCodec<SelectS2c, SelectC2s>>,
) -> anyhow::Result<[zohar_protocol::game_pkt::select::Player; 4]> {
    loop {
        let packet = timeout(Duration::from_secs(3), framed.next())
            .await
            .map_err(|_| anyhow::anyhow!("Timed out waiting for SetPlayerChoices"))?
            .ok_or(anyhow::anyhow!("Stream closed before SetPlayerChoices"))??;
        if let SelectS2c::Specific(SelectS2cSpecific::SetPlayerChoices { players, .. }) = packet {
            return Ok(players);
        }
    }
}

async fn await_phase_transition_select(
    framed: &mut Framed<TcpStream, SimpleBinRwCodec<SelectS2c, SelectC2s>>,
    expected: PhaseId,
) -> anyhow::Result<()> {
    loop {
        let packet = timeout(Duration::from_secs(3), framed.next())
            .await
            .map_err(|_| anyhow::anyhow!("Timed out waiting for Select phase transition"))?
            .ok_or(anyhow::anyhow!("Stream closed before phase transition"))??;
        if let SelectS2c::Control(ControlS2c::SetClientPhase { phase }) = packet {
            if phase == expected {
                return Ok(());
            }
        }
    }
}

async fn await_phase_transition_loading(
    framed: &mut Framed<TcpStream, SimpleBinRwCodec<LoadingS2c, LoadingC2s>>,
    expected: PhaseId,
) -> anyhow::Result<()> {
    loop {
        let packet = timeout(Duration::from_secs(3), framed.next())
            .await
            .map_err(|_| anyhow::anyhow!("Timed out waiting for Loading phase transition"))?
            .ok_or(anyhow::anyhow!("Stream closed before loading transition"))??;
        if let LoadingS2c::Control(ControlS2c::SetClientPhase { phase }) = packet {
            if phase == expected {
                return Ok(());
            }
        }
    }
}

async fn await_channel_info(
    framed: &mut Framed<TcpStream, SimpleBinRwCodec<InGameS2c, InGameC2s>>,
) -> anyhow::Result<u8> {
    loop {
        let packet = timeout(Duration::from_secs(3), framed.next())
            .await
            .map_err(|_| anyhow::anyhow!("Timed out waiting for SetChannelInfo"))?
            .ok_or(anyhow::anyhow!("Stream closed before SetChannelInfo"))??;
        if let InGameS2c::System(SystemS2c::SetChannelInfo { channel_id }) = packet {
            return Ok(channel_id);
        }
    }
}

async fn run_game_client(
    addr: std::net::SocketAddr,
    barrier: Arc<Barrier>,
    _id: usize,
    username: String,
    token: u32,
    enc_key: [u8; 16],
    hold_after_success: Duration,
) -> anyhow::Result<LoginResult> {
    let stream = TcpStream::connect(addr).await?;

    let mut handshake = Framed::new(
        stream,
        SimpleBinRwCodec::<HandshakeGameS2c, HandshakeGameC2s>::default(),
    );

    loop {
        let packet = handshake
            .next()
            .await
            .ok_or(anyhow::anyhow!("Stream closed during handshake"))??;

        match packet {
            HandshakeGameS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Handshake,
            }) => {}
            HandshakeGameS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Login,
            }) => break,
            HandshakeGameS2c::Control(ControlS2c::RequestHandshake { data }) => {
                let reply_data = HandshakeSyncData {
                    handshake: data.handshake,
                    time: WireMillis32::from(data.time.as_duration()),
                    delta: WireDeltaMillis::from(Duration::ZERO),
                };

                handshake
                    .send(HandshakeGameC2s::Control(ControlC2s::HandshakeResponse {
                        data: reply_data,
                    }))
                    .await?;
            }
            _ => continue,
        }
    }

    let stream = handshake.into_inner();
    let mut login = Framed::new(stream, SimpleBinRwCodec::<LoginS2c, LoginC2s>::default());
    let mut sequencer = PacketSequencer::default();

    // Synchronize all clients to send login at the same time
    barrier.wait().await;

    send_sequenced(
        login.get_mut(),
        &LoginC2s::Specific(LoginC2sSpecific::RequestTokenLogin {
            username: encode_username(&username),
            token,
            enc_key,
        }),
        &mut sequencer,
    )
    .await?;

    loop {
        let packet = login
            .next()
            .await
            .ok_or(anyhow::anyhow!("Stream closed during login reply"))??;
        match packet {
            LoginS2c::Control(ControlS2c::SetClientPhase {
                phase: PhaseId::Select,
            }) => {
                tokio::time::sleep(hold_after_success).await;
                return Ok(LoginResult::Success);
            }
            LoginS2c::Specific(LoginS2cSpecific::LoginResultFail { reason }) => {
                return match reason {
                    LoginFailReason::AlreadyLoggedIn
                    | LoginFailReason::InvalidCredentials
                    | LoginFailReason::BlockedAccount => Ok(LoginResult::Fail),
                };
            }
            _ => continue,
        }
    }
}

fn unique_test_username() -> String {
    let id = NEXT_TEST_USER.fetch_add(1, Ordering::Relaxed);
    format!("{TEST_USERNAME_PREFIX}_{id}")
}

fn encode_username(s: &str) -> [u8; 31] {
    let mut buf = [0; 31];
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate().take(30) {
        buf[i] = *b;
    }
    buf
}

fn test_content_catalog() -> ContentCatalog {
    ContentCatalog {
        maps: vec![
            ContentMap {
                map_id: 1,
                code: "zohar_map_a1".to_string(),
                name: "a1".to_string(),
                map_width: 1024.0,
                map_height: 1280.0,
                empire: Some(ContentEmpire::Red),
                base_x: Some(4096.0),
                base_y: Some(8960.0),
            },
            ContentMap {
                map_id: 21,
                code: "zohar_map_b1".to_string(),
                name: "b1".to_string(),
                map_width: 1024.0,
                map_height: 1280.0,
                empire: Some(ContentEmpire::Yellow),
                base_x: Some(0.0),
                base_y: Some(1024.0),
            },
            ContentMap {
                map_id: 41,
                code: "zohar_map_c1".to_string(),
                name: "c1".to_string(),
                map_width: 1024.0,
                map_height: 1280.0,
                empire: Some(ContentEmpire::Blue),
                base_x: Some(9216.0),
                base_y: Some(2048.0),
            },
        ],
        empire_start_configs: vec![
            EmpireStartConfig {
                empire: ContentEmpire::Red,
                start_map_id: 1,
                start_x: 597.0,
                start_y: 682.0,
            },
            EmpireStartConfig {
                empire: ContentEmpire::Yellow,
                start_map_id: 21,
                start_x: 557.0,
                start_y: 555.0,
            },
            EmpireStartConfig {
                empire: ContentEmpire::Blue,
                start_map_id: 41,
                start_x: 480.0,
                start_y: 736.0,
            },
        ],
        ..ContentCatalog::default()
    }
}
