// Phase-specific packet modules
pub mod handshake;
pub mod ingame;
pub mod loading;
pub mod login;
pub mod select;

pub use crate::control_pkt::{ControlC2s, ControlS2c};
pub use crate::handshake::*;
pub use crate::phase::*;
pub use crate::pkt_seq::{PacketSequencer, SequenceMismatchError};

// Re-export phase-specific packets
pub use handshake::{HandshakeGameC2s, HandshakeGameS2c, ServerInfo, ServerStatus};
pub use ingame::{InGameC2s, InGameS2c, chat::ChatKind};
pub use loading::{LoadingC2s, LoadingS2c};
pub use login::{LoginC2s, LoginFailReason, LoginS2c};
pub use select::{SelectC2s, SelectS2c};

pub const PLAYER_NAME_MAX_LENGTH: usize = 25;

/// Type alias for player names (fixed 25-byte buffer).
pub type EntityName = FixedString<PLAYER_NAME_MAX_LENGTH>;

/// A fixed-size null-terminated string.
///
/// Always reads/writes exactly `N` bytes. The string is truncated if longer,
/// or null-padded if shorter. Maintains null terminator within the buffer.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct FixedString<const N: usize> {
    buf: [u8; N],
    /// Length of the actual string content (excluding null terminator)
    len: usize,
}

impl<const N: usize> Default for FixedString<N> {
    fn default() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> std::fmt::Debug for FixedString<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FixedString").field(&self.as_str()).finish()
    }
}

impl<const N: usize> std::fmt::Display for FixedString<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl<const N: usize> FixedString<N> {
    /// Create from a string, truncating if necessary.
    pub fn new(s: &str) -> Self {
        let mut buf = [0u8; N];
        // Reserve last byte for null terminator
        let max_content = N.saturating_sub(1);
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(max_content);
        buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
        Self { buf, len: copy_len }
    }

    /// View as a string slice.
    pub fn as_str(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.buf[..self.len])
    }

    /// View the raw bytes (including potential null padding).
    pub fn as_bytes(&self) -> &[u8; N] {
        &self.buf
    }
}

impl<const N: usize> From<&str> for FixedString<N> {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl<const N: usize> From<String> for FixedString<N> {
    fn from(s: String) -> Self {
        Self::new(&s)
    }
}

impl<const N: usize> From<binrw::NullString> for FixedString<N> {
    fn from(ns: binrw::NullString) -> Self {
        Self::new(&ns.to_string())
    }
}

use binrw::io::{Read, Seek, Write};
use binrw::{BinRead, BinWrite, Endian, Error as BinError};

impl<const N: usize> BinRead for FixedString<N> {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let mut buf = [0u8; N];
        reader.read_exact(&mut buf)?;
        // Find null terminator to determine string length
        let len = buf.iter().position(|&b| b == 0).unwrap_or(N);
        Ok(Self { buf, len })
    }
}

impl<const N: usize> BinWrite for FixedString<N> {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        _endian: Endian,
        _args: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        // Always write exactly N bytes (buffer is already null-padded)
        writer.write_all(&self.buf)?;
        Ok(())
    }
}
use num_enum::{IntoPrimitive, TryFromPrimitive};

pub trait ZeroFallback: Sized {
    type Primitive: BinRead + BinWrite + Default + Copy + PartialEq;

    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str>;
    fn into_primitive(self) -> Self::Primitive;
}

macro_rules! impl_zero_fallback_identity {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ZeroFallback for $ty {
                type Primitive = $ty;

                fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
                    Ok(raw)
                }

                fn into_primitive(self) -> Self::Primitive {
                    self
                }
            }
        )*
    };
}

impl_zero_fallback_identity!(u8, u16, u32, u64, i8, i16, i32, i64);

macro_rules! impl_zero_fallback_num_enum {
    ($ty:ty, $primitive:ty) => {
        impl $crate::game_pkt::ZeroFallback for $ty {
            type Primitive = $primitive;

            fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
                <Self as TryFromPrimitive>::try_from_primitive(raw)
                    .map_err(|_| "invalid primitive value")
            }

            fn into_primitive(self) -> Self::Primitive {
                self.into()
            }
        }
    };
}
pub(crate) use impl_zero_fallback_num_enum;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ZeroOpt<T>(pub Option<T>);

impl<T> ZeroOpt<T> {
    pub fn none() -> Self {
        Self(None)
    }

    pub fn some(value: T) -> Self {
        Self(Some(value))
    }
}

impl<T> From<Option<T>> for ZeroOpt<T> {
    fn from(value: Option<T>) -> Self {
        Self(value)
    }
}

impl<T> From<T> for ZeroOpt<T> {
    fn from(value: T) -> Self {
        Self(Some(value))
    }
}

impl<T> From<ZeroOpt<T>> for Option<T> {
    fn from(value: ZeroOpt<T>) -> Self {
        value.0
    }
}

impl<T> BinRead for ZeroOpt<T>
where
    T: ZeroFallback,
    for<'a> <T::Primitive as BinRead>::Args<'a>: Default,
{
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(
        reader: &mut R,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> binrw::BinResult<Self> {
        let pos = reader.stream_position()?;
        let raw = <T::Primitive as BinRead>::read_options(reader, endian, Default::default())?;
        if raw == T::Primitive::default() {
            return Ok(Self(None));
        }

        match T::try_from_primitive(raw) {
            Ok(value) => Ok(Self(Some(value))),
            Err(message) => Err(BinError::AssertFail {
                pos,
                message: format!(
                    "ZeroOpt failed to decode {}: {message}",
                    std::any::type_name::<T>()
                ),
            }),
        }
    }
}

impl<T> BinWrite for ZeroOpt<T>
where
    T: ZeroFallback + Copy,
    for<'a> <T::Primitive as BinWrite>::Args<'a>: Default,
{
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _: Self::Args<'_>,
    ) -> binrw::BinResult<()> {
        let raw = self.0.map(ZeroFallback::into_primitive).unwrap_or_default();
        raw.write_options(writer, endian, Default::default())
    }
}

#[binrw::binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NetId(pub u32);

impl From<u32> for NetId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<NetId> for u32 {
    fn from(value: NetId) -> Self {
        value.0
    }
}

impl ZeroFallback for NetId {
    type Primitive = u32;

    fn try_from_primitive(raw: Self::Primitive) -> Result<Self, &'static str> {
        Ok(Self(raw))
    }

    fn into_primitive(self) -> Self::Primitive {
        self.0
    }
}

#[binrw::binrw]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct WireWorldCm(i32);

impl WireWorldCm {
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> i32 {
        self.0
    }
}

impl From<i32> for WireWorldCm {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl From<WireWorldCm> for i32 {
    fn from(value: WireWorldCm) -> Self {
        value.0
    }
}

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum SkillBranch {
    BranchA = 1,
    BranchB = 2,
}
impl_zero_fallback_num_enum!(SkillBranch, u8);

#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
pub enum PlayerClassGendered {
    WarriorMale = 0,
    NinjaFemale = 1,
    SuraMale = 2,
    ShamanFemale = 3,
    WarriorFemale = 4,
    NinjaMale = 5,
    SuraFemale = 6,
    ShamanMale = 7,
}
impl_zero_fallback_num_enum!(PlayerClassGendered, u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[binrw::binrw]
#[br(repr = u8)]
#[bw(repr = u8)]
#[repr(u8)]
pub enum Empire {
    Red = 1,
    Yellow = 2,
    Blue = 3,
}
impl_zero_fallback_num_enum!(Empire, u8);
