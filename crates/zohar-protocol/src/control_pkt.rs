use crate::handshake::HandshakeSyncData;
use crate::phase::PhaseId;
use binrw::{BinRead, BinResult, BinWrite, Endian};
use std::io::{Read, Seek, Write};

pub const OPCODE_HEARTBEAT_REQUEST: u8 = 0x2C;
pub const OPCODE_HEARTBEAT_RESPONSE: u8 = 0xFE;
pub const OPCODE_HANDSHAKE_REQUEST: u8 = 0xFF;
pub const OPCODE_HANDSHAKE_RESPONSE: u8 = 0xFF;
pub const OPCODE_TIME_SYNC_REQUEST: u8 = 0xFC;
pub const OPCODE_TIME_SYNC_RESPONSE: u8 = 0xFC;
pub const OPCODE_SET_CLIENT_PHASE: u8 = 0xFD;

#[inline]
pub fn is_unsequenced_c2s_opcode(opcode: u8) -> bool {
    matches!(
        opcode,
        OPCODE_HEARTBEAT_RESPONSE | OPCODE_HANDSHAKE_RESPONSE
    )
}

#[derive(Debug, Clone)]
pub enum ControlC2s {
    HeartbeatResponse,
    HandshakeResponse { data: HandshakeSyncData },
    RequestTimeSync { data: HandshakeSyncData },
}

#[derive(Debug, Clone)]
pub enum ControlS2c {
    RequestHeartbeat,
    RequestHandshake { data: HandshakeSyncData },
    TimeSyncResponse,
    SetClientPhase { phase: PhaseId },
}

pub fn read_control_c2s_with_opcode<R: Read + Seek>(
    opcode: u8,
    reader: &mut R,
    endian: Endian,
) -> BinResult<Option<ControlC2s>> {
    match opcode {
        OPCODE_HEARTBEAT_RESPONSE => Ok(Some(ControlC2s::HeartbeatResponse)),
        OPCODE_HANDSHAKE_RESPONSE => {
            let data = HandshakeSyncData::read_options(reader, endian, ())?;
            Ok(Some(ControlC2s::HandshakeResponse { data }))
        }
        OPCODE_TIME_SYNC_REQUEST => {
            let data = HandshakeSyncData::read_options(reader, endian, ())?;
            Ok(Some(ControlC2s::RequestTimeSync { data }))
        }
        _ => Ok(None),
    }
}

pub fn read_control_s2c_with_opcode<R: Read + Seek>(
    opcode: u8,
    reader: &mut R,
    endian: Endian,
) -> BinResult<Option<ControlS2c>> {
    match opcode {
        OPCODE_HEARTBEAT_REQUEST => Ok(Some(ControlS2c::RequestHeartbeat)),
        OPCODE_HANDSHAKE_REQUEST => {
            let data = HandshakeSyncData::read_options(reader, endian, ())?;
            Ok(Some(ControlS2c::RequestHandshake { data }))
        }
        OPCODE_TIME_SYNC_RESPONSE => Ok(Some(ControlS2c::TimeSyncResponse)),
        OPCODE_SET_CLIENT_PHASE => {
            let phase = PhaseId::read_options(reader, endian, ())?;
            Ok(Some(ControlS2c::SetClientPhase { phase }))
        }
        _ => Ok(None),
    }
}

impl ControlC2s {
    pub fn opcode(&self) -> u8 {
        match self {
            ControlC2s::HeartbeatResponse => OPCODE_HEARTBEAT_RESPONSE,
            ControlC2s::HandshakeResponse { .. } => OPCODE_HANDSHAKE_RESPONSE,
            ControlC2s::RequestTimeSync { .. } => OPCODE_TIME_SYNC_REQUEST,
        }
    }
}

impl ControlS2c {
    pub fn opcode(&self) -> u8 {
        match self {
            ControlS2c::RequestHeartbeat => OPCODE_HEARTBEAT_REQUEST,
            ControlS2c::RequestHandshake { .. } => OPCODE_HANDSHAKE_REQUEST,
            ControlS2c::TimeSyncResponse => OPCODE_TIME_SYNC_RESPONSE,
            ControlS2c::SetClientPhase { .. } => OPCODE_SET_CLIENT_PHASE,
        }
    }
}

impl BinRead for ControlC2s {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let opcode = u8::read_options(reader, endian, ())?;
        match read_control_c2s_with_opcode(opcode, reader, endian)? {
            Some(control) => Ok(control),
            None => Err(binrw::Error::AssertFail {
                pos: reader.stream_position()?,
                message: "Unknown ControlC2s opcode".into(),
            }),
        }
    }
}

impl BinRead for ControlS2c {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<Self> {
        let opcode = u8::read_options(reader, endian, ())?;
        match read_control_s2c_with_opcode(opcode, reader, endian)? {
            Some(control) => Ok(control),
            None => Err(binrw::Error::AssertFail {
                pos: reader.stream_position()?,
                message: "Unknown ControlS2c opcode".into(),
            }),
        }
    }
}

impl BinWrite for ControlC2s {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let opcode = self.opcode();
        opcode.write_options(writer, endian, ())?;
        match self {
            ControlC2s::HeartbeatResponse => Ok(()),
            ControlC2s::HandshakeResponse { data } => data.write_options(writer, endian, ()),
            ControlC2s::RequestTimeSync { data } => data.write_options(writer, endian, ()),
        }
    }
}

impl BinWrite for ControlS2c {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        (): Self::Args<'_>,
    ) -> BinResult<()> {
        let opcode = self.opcode();
        opcode.write_options(writer, endian, ())?;
        match self {
            ControlS2c::RequestHeartbeat => Ok(()),
            ControlS2c::RequestHandshake { data } => data.write_options(writer, endian, ()),
            ControlS2c::TimeSyncResponse => Ok(()),
            ControlS2c::SetClientPhase { phase } => phase.write_options(writer, endian, ()),
        }
    }
}
