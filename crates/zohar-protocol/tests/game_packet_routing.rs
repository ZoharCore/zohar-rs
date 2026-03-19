#![cfg(feature = "game")]

use binrw::{BinRead, BinWrite, Endian};
use std::io::Cursor;
use zohar_protocol::control_pkt::ControlS2c;
use zohar_protocol::game_pkt::handshake::HandshakeGameC2sSpecific;
use zohar_protocol::game_pkt::ingame;
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
fn handshake_routes_specific() {
    let pkt = HandshakeGameC2s::Specific(HandshakeGameC2sSpecific::FetchChannelList);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        HandshakeGameC2s::Specific(HandshakeGameC2sSpecific::FetchChannelList)
    ));
}

#[test]
fn handshake_routes_control() {
    let pkt = HandshakeGameS2c::Control(ControlS2c::RequestHeartbeat);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        HandshakeGameS2c::Control(ControlS2c::RequestHeartbeat)
    ));
}

#[test]
fn handshake_rejects_unknown_opcode() {
    let mut cursor = Cursor::new(vec![0x99]);
    let result = HandshakeGameC2s::read_options(&mut cursor, Endian::Little, ());
    assert!(result.is_err());
}

#[test]
fn login_routes_specific() {
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
}

#[test]
fn login_routes_s2c_specific() {
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
}

#[test]
fn loading_routes_specific() {
    let pkt = LoadingC2s::Specific(LoadingC2sSpecific::SignalLoadingComplete);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        LoadingC2s::Specific(LoadingC2sSpecific::SignalLoadingComplete)
    ));
}

#[test]
fn loading_routes_s2c_specific() {
    let pkt =
        LoadingS2c::Specific(LoadingS2cSpecific::SetMainCharacterStats { stats: [0u32; 255] });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        LoadingS2c::Specific(LoadingS2cSpecific::SetMainCharacterStats { .. })
    ));
}

#[test]
fn select_routes_specific() {
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
}

#[test]
fn select_routes_s2c_specific() {
    let pkt = SelectS2c::Specific(SelectS2cSpecific::DeletePlayerResultFail);
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        SelectS2c::Specific(SelectS2cSpecific::DeletePlayerResultFail)
    ));
}

#[test]
fn ingame_routes_chat() {
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
}

#[test]
fn ingame_routes_attack() {
    let pkt = InGameC2s::Combat(ingame::combat::CombatC2s::InputAttack {
        attack_type: game_pkt::ZeroOpt::some(Skill::Berserk),
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
            assert_eq!(attack_type, game_pkt::ZeroOpt::some(Skill::Berserk));
            assert_eq!(target, NetId(0x0102_0304));
            assert_eq!(unknown, 0x1122);
        }
        other => panic!("unexpected packet: {other:?}"),
    }
}

#[test]
fn ingame_routes_target() {
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
fn ingame_routes_system() {
    let pkt = InGameS2c::System(ingame::system::SystemS2c::SetChannelInfo { channel_id: 7 });
    let decoded = round_trip(&pkt);
    assert!(matches!(
        decoded,
        InGameS2c::System(ingame::system::SystemS2c::SetChannelInfo { channel_id: 7 })
    ));
}
