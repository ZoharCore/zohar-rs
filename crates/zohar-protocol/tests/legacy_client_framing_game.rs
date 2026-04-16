#![cfg(feature = "game")]

mod packet_framing_support;

use packet_framing_support::{assert_packet_frame, encoded_bytes};
use zohar_protocol::game_pkt::ingame::chat::{ChatC2s, ChatKind, ChatS2c};
use zohar_protocol::game_pkt::ingame::combat::CombatC2s;
use zohar_protocol::game_pkt::ingame::movement::{MovementC2s, MovementKind, MovementS2c};
use zohar_protocol::game_pkt::ingame::system::SystemS2c;
use zohar_protocol::game_pkt::ingame::world::{EntityType, WorldS2c};
use zohar_protocol::game_pkt::select::{CreatePlayerError, Player};
use zohar_protocol::game_pkt::{
    Empire, HandshakeGameC2s, HandshakeGameS2c, LoadingC2s, LoadingS2c, LoginC2s, LoginS2c, NetId,
    SelectC2s, SelectS2c, ServerInfo, ServerStatus, WireMillis32, WireServerAddr, ZeroOpt,
};

#[test]
fn chat_c2s_size_field_matches_buffer_length() {
    let pkt = ChatC2s::SubmitChatMessage {
        kind: ChatKind::Speak,
        message: b"hello".to_vec(),
    };

    let raw = encoded_bytes(&pkt);

    assert_eq!(raw[0], 0x03);
    assert!(raw.len() >= 3);
    let size_field = u16::from_le_bytes([raw[1], raw[2]]) as usize;
    assert_eq!(
        size_field,
        raw.len(),
        "size field should equal total serialized packet length (includes header)"
    );
}

#[test]
fn chat_s2c_size_field_matches_buffer_length() {
    let pkt = ChatS2c::NotifyChatMessage {
        kind: ChatKind::Speak,
        net_id: ZeroOpt::none(),
        empire: ZeroOpt::none(),
        message: b"world".to_vec(),
    };

    let raw = encoded_bytes(&pkt);

    assert_eq!(raw[0], 0x04);
    assert!(raw.len() >= 3);
    let size_field = u16::from_le_bytes([raw[1], raw[2]]) as usize;
    assert_eq!(
        size_field,
        raw.len(),
        "size field should equal total serialized packet length (includes header)"
    );
}

#[test]
fn handshake_c2s_fetch_channel_list_is_single_byte() {
    assert_packet_frame(
        &HandshakeGameC2s::Specific(
            zohar_protocol::game_pkt::handshake::HandshakeGameC2sSpecific::FetchChannelList,
        ),
        0xCE,
        1,
    );
}

#[test]
fn handshake_s2c_channel_list_response_keeps_its_legacy_length() {
    let pkt = HandshakeGameS2c::Specific(
        zohar_protocol::game_pkt::handshake::HandshakeGameS2cSpecific::ChannelListResponse {
            statuses: vec![ServerInfo {
                srv_port: 13000,
                status: ServerStatus::Online,
            }],
            is_ok: 1,
        },
    );

    let raw = encoded_bytes(&pkt);
    assert_eq!(raw[0], 0xD2);
    assert_eq!(raw.len(), 9);
    assert_eq!(
        u32::from_le_bytes([raw[1], raw[2], raw[3], raw[4]]) as usize,
        1
    );
}

#[test]
fn login_c2s_request_token_login_keeps_its_legacy_length() {
    assert_packet_frame(
        &LoginC2s::Specific(
            zohar_protocol::game_pkt::login::LoginC2sSpecific::RequestTokenLogin {
                username: [b'u'; 31],
                token: 0x1122_3344,
                enc_key: [b'k'; 16],
            },
        ),
        0x6D,
        52,
    );
}

#[test]
fn login_s2c_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &LoginS2c::Specific(
            zohar_protocol::game_pkt::login::LoginS2cSpecific::LoginResultFail {
                reason: zohar_protocol::game_pkt::login::LoginFailReason::BlockedAccount,
            },
        ),
        0x07,
        10,
    );
    assert_packet_frame(
        &LoginS2c::Specific(
            zohar_protocol::game_pkt::login::LoginS2cSpecific::SetAccountEmpire {
                empire: Empire::Yellow,
            },
        ),
        0x5A,
        2,
    );
}

#[test]
fn select_c2s_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &SelectC2s::Specific(
            zohar_protocol::game_pkt::select::SelectC2sSpecific::SubmitEmpireChoice {
                empire: Empire::Blue,
            },
        ),
        0x5A,
        2,
    );
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::SetAccountEmpire {
                empire: Empire::Yellow,
            },
        ),
        0x5A,
        2,
    );
    assert_packet_frame(
        &SelectC2s::Specific(
            zohar_protocol::game_pkt::select::SelectC2sSpecific::RequestCreatePlayer {
                slot: zohar_protocol::game_pkt::select::PlayerSelectSlot::First,
                name: [b'a'; zohar_protocol::game_pkt::PLAYER_NAME_MAX_LENGTH],
                class_gender: zohar_protocol::game_pkt::PlayerClassGendered::WarriorMale,
                appearance: zohar_protocol::game_pkt::select::PlayerBaseAppearance::VariantA,
                _reserved: 0,
                _reserved_stats: 0,
            },
        ),
        0x04,
        34,
    );
    assert_packet_frame(
        &SelectC2s::Specific(
            zohar_protocol::game_pkt::select::SelectC2sSpecific::RequestDeletePlayer {
                slot: zohar_protocol::game_pkt::select::PlayerSelectSlot::Fourth,
                code: *b"1234567",
                _reserved: 0,
            },
        ),
        0x05,
        10,
    );
    assert_packet_frame(
        &SelectC2s::Specific(
            zohar_protocol::game_pkt::select::SelectC2sSpecific::SubmitPlayerChoice {
                slot: zohar_protocol::game_pkt::select::PlayerSelectSlot::Second,
            },
        ),
        0x06,
        2,
    );
}

#[test]
fn select_s2c_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::SetAccountEmpire {
                empire: Empire::Yellow,
            },
        ),
        0x5A,
        2,
    );
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::SetPlayerChoices {
                players: [
                    Player::empty(),
                    Player::empty(),
                    Player::empty(),
                    Player::empty(),
                ],
                guild_ids: [0u32; 4],
                guild_names: Default::default(),
            },
        ),
        0x20,
        329,
    );
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::CreatePlayerResultOk {
                slot: zohar_protocol::game_pkt::select::PlayerSelectSlot::First,
                new_player: Player::empty(),
            },
        ),
        0x08,
        65,
    );
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::CreatePlayerResultFail {
                error: CreatePlayerError::NameAlreadyExists,
            },
        ),
        0x09,
        2,
    );
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::DeletePlayerResultOk {
                slot: zohar_protocol::game_pkt::select::PlayerSelectSlot::Third,
            },
        ),
        0x0A,
        2,
    );
    assert_packet_frame(
        &SelectS2c::Specific(
            zohar_protocol::game_pkt::select::SelectS2cSpecific::DeletePlayerResultFail,
        ),
        0x0B,
        1,
    );
}

#[test]
fn loading_c2s_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &LoadingC2s::Specific(
            zohar_protocol::game_pkt::loading::LoadingC2sSpecific::SubmitClientVersion {
                client: [b'c'; 33],
                version: [b'v'; 33],
            },
        ),
        0xF1,
        67,
    );
    assert_packet_frame(
        &LoadingC2s::Specific(
            zohar_protocol::game_pkt::loading::LoadingC2sSpecific::SignalLoadingComplete,
        ),
        0x0A,
        1,
    );
}

#[test]
fn ingame_warp_packet_keeps_its_legacy_length() {
    assert_packet_frame(
        &zohar_protocol::game_pkt::ingame::InGameS2c::System(SystemS2c::InitServerHandoff {
            destination_addr: WireServerAddr {
                srv_ipv4_addr: i32::from_le_bytes([127, 0, 0, 1]),
                srv_port: 13_000,
            },
        }),
        0x41,
        15,
    );
}

#[test]
fn loading_s2c_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &LoadingS2c::Specific(
            zohar_protocol::game_pkt::loading::LoadingS2cSpecific::SetMainCharacter {
                net_id: zohar_protocol::game_pkt::NetId(0x0102_0304),
                class_gender: zohar_protocol::game_pkt::PlayerClassGendered::WarriorMale,
                name: "legacy".into(),
                pos: (100, 200).into(),
                empire: Empire::Blue,
                skill_branch: zohar_protocol::game_pkt::ZeroOpt::some(
                    zohar_protocol::game_pkt::SkillBranch::BranchA,
                ),
            },
        ),
        0x71,
        46,
    );
    assert_packet_frame(
        &LoadingS2c::Specific(
            zohar_protocol::game_pkt::loading::LoadingS2cSpecific::SetMainCharacterStats {
                stats: [0u32; 255],
            },
        ),
        0x10,
        1021,
    );
}

#[test]
fn movement_c2s_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &MovementC2s::InputMovement {
            kind: MovementKind::Move,
            arg: 0,
            rot: 0,
            pos: (100, 200).into(),
            ts: WireMillis32::from(0u32),
        },
        0x07,
        16,
    );
}

#[test]
fn movement_s2c_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &MovementS2c::SyncEntityMovement {
            kind: MovementKind::Move,
            arg: 0,
            rot: 0,
            net_id: NetId(0x0102_0304),
            pos: (100, 200).into(),
            ts: WireMillis32::from(0u32),
            duration: WireMillis32::from(500u32),
        },
        0x03,
        24,
    );
}

#[test]
fn combat_c2s_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &CombatC2s::InputAttack {
            attack_type: ZeroOpt::none(),
            target: NetId(1),
            _unknown: 0,
        },
        0x02,
        8,
    );
    assert_packet_frame(&CombatC2s::SignalTargetSwitch { target: NetId(2) }, 0x3D, 5);
}

#[test]
fn system_s2c_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &SystemS2c::SetServerTime {
            time: WireMillis32::from(0u32),
        },
        0x6A,
        5,
    );
    assert_packet_frame(&SystemS2c::SetChannelInfo { channel_id: 1 }, 0x79, 2);
}

#[test]
fn world_s2c_packets_keep_their_legacy_lengths() {
    assert_packet_frame(
        &WorldS2c::SpawnEntity {
            net_id: NetId(1),
            angle: 0.0,
            pos: (0, 0).into(),
            entity_type: EntityType::Player,
            race_num: 0,
            move_speed: 100,
            attack_speed: 100,
            state_flags: 0,
            buff_flags: 0,
        },
        0x01,
        35,
    );
    assert_packet_frame(
        &WorldS2c::SetEntityDetails {
            net_id: NetId(1),
            name: "test".into(),
            body_part: 0,
            wep_part: 0,
            hair_part: 0,
            empire: ZeroOpt::none(),
            guild_id: 0,
            level: 1,
            rank_pts: 0,
            pvp_mode: 0,
            mount_id: 0,
        },
        0x88,
        54,
    );
    assert_packet_frame(&WorldS2c::DestroyEntity { net_id: NetId(1) }, 0x02, 5);
}
