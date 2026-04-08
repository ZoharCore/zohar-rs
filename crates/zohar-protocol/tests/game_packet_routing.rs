#![cfg(feature = "game")]

use binrw::{BinRead, BinWrite, Endian};
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use zohar_protocol::control_pkt::ControlS2c;
use zohar_protocol::game_pkt::handshake::HandshakeGameC2sSpecific;
use zohar_protocol::game_pkt::ingame::{self, Skill};
use zohar_protocol::game_pkt::loading::{LoadingC2sSpecific, LoadingS2cSpecific};
use zohar_protocol::game_pkt::login::{LoginC2sSpecific, LoginS2cSpecific};
use zohar_protocol::game_pkt::select::{SelectC2sSpecific, SelectS2cSpecific};
use zohar_protocol::game_pkt::*;

fn round_trip<T>(value: &T) -> T
where
    for<'a> T: BinRead<Args<'a> = ()> + BinWrite<Args<'a> = ()>,
{
    let mut cursor = Cursor::new(Vec::new());
    BinWrite::write_options(value, &mut cursor, Endian::Little, ()).unwrap();
    cursor.set_position(0);
    T::read_options(&mut cursor, Endian::Little, ()).unwrap()
}

#[test]
fn c2s_packets_roundtrip() {
    // Handshake
    let pkt = HandshakeGameC2s::Specific(HandshakeGameC2sSpecific::FetchChannelList);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        HandshakeGameC2s::Specific(HandshakeGameC2sSpecific::FetchChannelList)
    ));

    // Login
    let pkt = LoginC2s::Specific(LoginC2sSpecific::RequestTokenLogin {
        username: [0u8; 31],
        token: 42,
        enc_key: [0u8; 16],
    });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        LoginC2s::Specific(LoginC2sSpecific::RequestTokenLogin { .. })
    ));

    // Loading
    let pkt = LoadingC2s::Specific(LoadingC2sSpecific::SignalLoadingComplete);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        LoadingC2s::Specific(LoadingC2sSpecific::SignalLoadingComplete)
    ));

    // Select
    let pkt = SelectC2s::Specific(SelectC2sSpecific::SubmitEmpireChoice {
        empire: Empire::Blue,
    });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        SelectC2s::Specific(SelectC2sSpecific::SubmitEmpireChoice {
            empire: Empire::Blue
        })
    ));

    // InGame – Chat
    let pkt = InGameC2s::Chat(ingame::chat::ChatC2s::SubmitChatMessage {
        kind: ChatKind::Speak,
        message: b"hi".to_vec(),
    });
    let decoded = round_trip(&pkt);
    match decoded {
        InGameC2s::Chat(ingame::chat::ChatC2s::SubmitChatMessage { kind, message }) => {
            assert_eq!(kind, ChatKind::Speak);
            assert_eq!(message, b"hi");
        }
        other => panic!("unexpected packet: {other:?}"),
    }

    // InGame – Attack
    let pkt = InGameC2s::Combat(ingame::combat::CombatC2s::InputAttack {
        attack_type: ZeroOpt::some(Skill::Berserk),
        target: NetId(0x0102_0304),
        _unknown: 0x1122,
    });
    let decoded = round_trip(&pkt);
    match decoded {
        InGameC2s::Combat(ingame::combat::CombatC2s::InputAttack {
            attack_type,
            target,
            _unknown,
        }) => {
            assert_eq!(attack_type, ZeroOpt::some(Skill::Berserk));
            assert_eq!(target, NetId(0x0102_0304));
            assert_eq!(_unknown, 0x1122);
        }
        other => panic!("unexpected packet: {other:?}"),
    }

    // InGame – Target
    let pkt = InGameC2s::Combat(ingame::combat::CombatC2s::SignalTargetSwitch {
        target: NetId(0x0506_0708),
    });
    let decoded = round_trip(&pkt);
    match decoded {
        InGameC2s::Combat(ingame::combat::CombatC2s::SignalTargetSwitch { target }) => {
            assert_eq!(target, NetId(0x0506_0708));
        }
        other => panic!("unexpected packet: {other:?}"),
    }
}

#[test]
fn s2c_packets_roundtrip() {
    // Handshake – Control
    let pkt = HandshakeGameS2c::Control(ControlS2c::RequestHeartbeat);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        HandshakeGameS2c::Control(ControlS2c::RequestHeartbeat)
    ));

    // Login
    let pkt = LoginS2c::Specific(LoginS2cSpecific::SetAccountEmpire {
        empire: Empire::Yellow,
    });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        LoginS2c::Specific(LoginS2cSpecific::SetAccountEmpire {
            empire: Empire::Yellow
        })
    ));

    // Loading
    let pkt =
        LoadingS2c::Specific(LoadingS2cSpecific::SetMainCharacterStats { stats: [0u32; 255] });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        LoadingS2c::Specific(LoadingS2cSpecific::SetMainCharacterStats { .. })
    ));

    // Select
    let pkt = SelectS2c::Specific(SelectS2cSpecific::DeletePlayerResultFail);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        SelectS2c::Specific(SelectS2cSpecific::DeletePlayerResultFail)
    ));

    // InGame – System
    let pkt = InGameS2c::System(ingame::system::SystemS2c::SetChannelInfo { channel_id: 7 });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        InGameS2c::System(ingame::system::SystemS2c::SetChannelInfo { channel_id: 7 })
    ));

    let pkt = InGameS2c::System(ingame::system::SystemS2c::InitServerHandoff {
        destination_addr: WireServerAddr {
            srv_ipv4_addr: i32::from_le_bytes([127, 0, 0, 1]),
            srv_port: 13_000,
        },
    });
    let decoded = round_trip(&pkt);
    match decoded {
        InGameS2c::System(ingame::system::SystemS2c::InitServerHandoff { destination_addr }) => {
            assert_eq!(
                destination_addr.srv_ipv4_addr,
                i32::from_le_bytes([127, 0, 0, 1])
            );
            assert_eq!(destination_addr.srv_port, 13_000);
        }
        other => panic!("unexpected packet: {other:?}"),
    }
}

#[test]
fn unknown_opcode_is_rejected() {
    let mut cursor = Cursor::new(vec![0x99]);
    let result = HandshakeGameC2s::read_options(&mut cursor, Endian::Little, ());
    assert!(result.is_err());
}

#[test]
fn wire_server_addr_encodes_ipv4_and_mapped_ipv6() {
    let addr = WireServerAddr::from_socket_addr(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        13_000,
    ))
    .expect("ipv4 address");
    assert_eq!(addr.srv_ipv4_addr, i32::from_le_bytes([127, 0, 0, 1]));
    assert_eq!(addr.srv_port, 13_000);

    let mapped = WireServerAddr::from_socket_addr(SocketAddr::new(
        IpAddr::V6(Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped()),
        13_001,
    ))
    .expect("mapped ipv6 address");
    assert_eq!(mapped.srv_ipv4_addr, i32::from_le_bytes([127, 0, 0, 1]));
    assert_eq!(mapped.srv_port, 13_001);

    let native_v6 =
        WireServerAddr::from_socket_addr(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 13_002));
    assert!(native_v6.is_none());
}
