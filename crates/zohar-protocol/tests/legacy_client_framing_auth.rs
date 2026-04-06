#![cfg(feature = "auth")]

mod packet_framing_support;

use packet_framing_support::assert_packet_frame;
use zohar_protocol::auth_pkt::{
    AuthC2s, AuthC2sSpecific, AuthS2c, AuthS2cSpecific, HandshakeAuthC2s, HandshakeAuthS2c,
    LoginFailureReason,
};
use zohar_protocol::control_pkt::{ControlC2s, ControlS2c};

#[test]
fn auth_c2s_request_password_login_keeps_its_frame_size() {
    assert_packet_frame(
        &AuthC2s::Specific(AuthC2sSpecific::RequestPasswordLogin {
            username: [b'u'; 31],
            password: [b'p'; 17],
            enc_key: [b'k'; 16],
        }),
        0x6F,
        65,
    );
}

#[test]
fn auth_s2c_login_result_fail_keeps_its_frame_size() {
    assert_packet_frame(
        &AuthS2c::Specific(AuthS2cSpecific::LoginResultFail {
            reason: LoginFailureReason::ServerAtCapacity,
        }),
        0x07,
        14,
    );
}

#[test]
fn auth_s2c_login_result_ok_keeps_its_frame_size() {
    assert_packet_frame(
        &AuthS2c::Specific(AuthS2cSpecific::LoginResultOk {
            token: 0x1122_3344,
            is_ok: 1,
        }),
        0x96,
        6,
    );
}

#[test]
fn auth_handshake_c2s_control_packets_keep_their_opcode_only_frame() {
    assert_packet_frame(
        &HandshakeAuthC2s::Control(ControlC2s::HeartbeatResponse),
        0xFE,
        1,
    );
}

#[test]
fn auth_handshake_s2c_control_packets_keep_their_opcode_only_frame() {
    assert_packet_frame(
        &HandshakeAuthS2c::Control(ControlS2c::RequestHeartbeat),
        0x2C,
        1,
    );
}
