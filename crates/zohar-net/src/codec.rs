//! BinRw codecs for packet serialization/deserialization.
//!
//! Two codec variants:
//! - `SimpleBinRwCodec` - No sequence tracking (used for Handshake phase)
//! - `SequencedBinRwCodec` - Validates trailing sequence byte (used for Login/Select/Loading/InGame phases)
//!
//! The sequence byte is handled transparently at codec level - packet enums
//! don't need `_seq` fields.

use binrw::error::CustomError;
use binrw::{BinRead, BinWrite, Endian, Error as BinError};
use std::fmt::{Debug, Formatter};
use std::io::Cursor;
use std::marker::PhantomData;
use tokio_util::bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use tracing::{error, trace};
use zohar_protocol::phase::PhaseMismatchError;
use zohar_protocol::pkt_seq::{PacketSequencer, SequenceMismatchError};

/// Stateless codec - no sequence tracking.
pub struct Stateless;

/// Sequenced codec state - validates trailing sequence byte for replay protection.
#[derive(Default)]
pub struct Sequenced {
    pub sequencer: PacketSequencer,
}

pub struct BinRwCodec<I, O, S> {
    /// Codec state - public for extraction during phase transitions.
    pub state: S,
    encode_scratchpad: Vec<u8>,
    _marker: PhantomData<(I, O)>,
}

impl<I, O, S> BinRwCodec<I, O, S> {
    pub fn new(state: S) -> Self {
        Self {
            state,
            encode_scratchpad: Vec::new(),
            _marker: PhantomData,
        }
    }
}

/// Simple codec that reads/writes binrw types with no sequence tracking.
/// Used for Handshake phase packets.
pub type SimpleBinRwCodec<I, O> = BinRwCodec<I, O, Stateless>;

/// Sequenced codec that validates trailing sequence byte for replay protection.
/// Used for Login/Select/Loading/InGame phase packets.
pub type SequencedBinRwCodec<I, O> = BinRwCodec<I, O, Sequenced>;

impl<I, O> Default for SimpleBinRwCodec<I, O> {
    fn default() -> Self {
        Self::new(Stateless)
    }
}

impl<I, O> Default for SequencedBinRwCodec<I, O> {
    fn default() -> Self {
        Self::new(Sequenced::default())
    }
}

// SimpleBinRwCodec - no sequence tracking
impl<I, O> Decoder for SimpleBinRwCodec<I, O>
where
    I: BinRead + Debug + 'static,
    for<'a> I::Args<'a>: Default,
{
    type Item = I;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        let mut cursor = Cursor::new(&src[..]);

        match I::read_options(&mut cursor, Endian::Little, Default::default()) {
            Ok(packet) => {
                let consumed = cursor.position() as usize;
                trace!(
                    packet_type = %type_name_short::<I>(),
                    packet = ?packet,
                    packet_raw = %HexFmt(&src[..consumed]),
                    "Decoded packet"
                );
                src.advance(consumed);
                Ok(Some(packet))
            }
            Err(err) if is_incomplete(&err) => Ok(None),
            Err(err) => Err(decode_error_to_io::<I>(src, &err)),
        }
    }
}

// SequencedBinRwCodec - reads packet, then validates trailing sequence byte.
// A small set of control opcodes remain unsequenced for protocol compatibility.
impl<I, O> Decoder for SequencedBinRwCodec<I, O>
where
    I: BinRead + Debug + 'static,
    for<'a> I::Args<'a>: Default,
{
    type Item = I;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        // Peek at opcode to determine if this is an unsequenced control packet
        let opcode = src[0];
        let has_sequence = !zohar_protocol::control_pkt::is_unsequenced_c2s_opcode(opcode);

        let mut cursor = Cursor::new(&src[..]);

        // 1. Parse the packet
        let packet = match I::read_options(&mut cursor, Endian::Little, Default::default()) {
            Ok(p) => p,
            Err(err) if is_incomplete(&err) => return Ok(None),
            Err(err) => return Err(decode_error_to_io::<I>(src, &err)),
        };

        let packet_len = cursor.position() as usize;

        if has_sequence {
            // 2. Check if we have the trailing sequence byte
            if packet_len >= src.len() {
                return Ok(None);
            }

            // 3. Read and validate sequence byte
            let seq_byte = src[packet_len];
            if !self.state.sequencer.check(seq_byte) {
                let expected = self.state.sequencer.expected();
                error!(
                    packet_type = %type_name_short::<I>(),
                    expected = %format_args!("0x{:02X}", expected),
                    got = %format_args!("0x{:02X}", seq_byte),
                    "Packet sequence mismatch"
                );
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    SequenceMismatchError {
                        expected,
                        got: seq_byte,
                    },
                ));
            }

            let total_consumed = packet_len + 1;
            trace!(
                packet_type = %type_name_short::<I>(),
                packet = ?packet,
                seq = %format_args!("0x{:02X}", seq_byte),
                packet_raw = %HexFmt(&src[..total_consumed]),
                "Decoded sequenced packet"
            );
            src.advance(total_consumed);
        } else {
            // Control packet - no sequence byte
            trace!(
                packet_type = %type_name_short::<I>(),
                packet = ?packet,
                packet_raw = %HexFmt(&src[..packet_len]),
                "Decoded control packet (no sequence)"
            );
            src.advance(packet_len);
        }

        Ok(Some(packet))
    }
}

impl<I, O> Encoder<O> for SimpleBinRwCodec<I, O>
where
    O: BinWrite + Debug + 'static,
    for<'a> O::Args<'a>: Default,
{
    type Error = std::io::Error;

    fn encode(&mut self, item: O, dst: &mut BytesMut) -> Result<(), Self::Error> {
        encode_item(&mut self.encode_scratchpad, item, dst)
    }
}

impl<I, O> Encoder<O> for SequencedBinRwCodec<I, O>
where
    O: BinWrite + Debug + 'static,
    for<'a> O::Args<'a>: Default,
{
    type Error = std::io::Error;

    fn encode(&mut self, item: O, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // S2c packets don't have sequence bytes in this protocol
        encode_item(&mut self.encode_scratchpad, item, dst)
    }
}

fn encode_item<O>(
    scratchpad: &mut Vec<u8>,
    item: O,
    dst: &mut BytesMut,
) -> Result<(), std::io::Error>
where
    O: BinWrite + Debug + 'static,
    for<'a> O::Args<'a>: Default,
{
    let mut cursor = Cursor::new(std::mem::take(scratchpad));

    item.write_options(&mut cursor, Endian::Little, Default::default())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let written = cursor.into_inner();

    trace!(
        packet_type = %type_name_short::<O>(),
        packet = ?item,
        packet_raw = %HexFmt(&written),
        "Encoding packet"
    );

    dst.extend_from_slice(&written);

    *scratchpad = written;
    scratchpad.clear();

    Ok(())
}

fn decode_error_to_io<I>(src: &BytesMut, err: &BinError) -> std::io::Error {
    if let Some(phase_err) = find_source_error::<PhaseMismatchError>(err) {
        let opcode = src.first().copied().unwrap_or(0);
        error!(
            packet_type = %type_name_short::<I>(),
            current_phase = ?phase_err.actual(),
            opcode = %format_args!("0x{:02X}", opcode),
            packet_raw = %HexFmt(src),
            "Packet rejected due to phase mismatch"
        );
        return std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Phase mismatch: had {:?} on connection, opcode 0x{:02X}, payload {}",
                phase_err.actual(),
                opcode,
                HexFmt(src)
            ),
        );
    }

    if is_unknown_opcode(err) {
        let opcode = src.first().copied().unwrap_or(0);
        error!(
            packet_type = %type_name_short::<I>(),
            opcode = %format_args!("0x{:02X}", opcode),
            packet_raw = %HexFmt(src),
            "Unknown opcode"
        );
        return std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Unknown opcode 0x{opcode:02X}, payload {}", HexFmt(src)),
        );
    }

    error!(error = %err, "Failed to decode packet");
    std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string())
}

fn is_incomplete(err: &BinError) -> bool {
    if err.is_eof() {
        return true;
    }
    match err {
        BinError::EnumErrors { variant_errors, .. } => {
            variant_errors.iter().any(|(_, e)| is_incomplete(e))
        }
        BinError::Backtrace(bt) => is_incomplete(&bt.error),
        _ => false,
    }
}

fn find_source_error<T: CustomError + 'static>(err: &BinError) -> Option<&T> {
    match err {
        BinError::Custom { err, .. } => err.downcast_ref::<T>(),
        BinError::EnumErrors { variant_errors, .. } => variant_errors
            .iter()
            .find_map(|(_, e)| find_source_error(e)),
        BinError::Backtrace(bt) => find_source_error(&bt.error),
        _ => None,
    }
}

fn is_unknown_opcode(err: &BinError) -> bool {
    fn is_start_magic(err: &BinError) -> bool {
        match err {
            BinError::BadMagic { pos, .. } => *pos == 0,
            BinError::Backtrace(bt) => is_start_magic(&bt.error),
            _ => false,
        }
    }

    match err {
        BinError::EnumErrors { variant_errors, .. } => {
            !variant_errors.is_empty() && variant_errors.iter().all(|(_, e)| is_start_magic(e))
        }
        BinError::Backtrace(bt) => is_unknown_opcode(&bt.error),
        e => is_start_magic(e),
    }
}

fn type_name_short<T>() -> &'static str {
    std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or("?")
}

struct HexFmt<'a>(&'a [u8]);

impl std::fmt::Display for HexFmt<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x")?;
        for byte in self.0 {
            write!(f, "{:02X}", byte)?;
        }
        Ok(())
    }
}
