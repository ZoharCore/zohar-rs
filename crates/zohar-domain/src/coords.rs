use euclid::{Box2D, Length, Point2D, Rotation2D, Size2D, Vector2D};
use rand::RngExt;
use rand::prelude::SmallRng;
use std::fmt;
#[cfg(feature = "admin-brp")]
use std::marker::PhantomData;

pub enum GlobalMeters {}
pub type WorldPos = Point2D<f32, GlobalMeters>;

pub enum LocalMeters {}
pub type LocalPos = Point2D<f32, LocalMeters>;
pub type LocalSize = Size2D<f32, LocalMeters>;
pub type LocalBox = Box2D<f32, LocalMeters>;
pub type LocalDistMeters = Length<f32, LocalMeters>;
pub type LocalVec = Vector2D<f32, LocalMeters>;
pub type LocalRotation = Rotation2D<f32, LocalMeters, LocalMeters>;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
#[repr(transparent)]
pub struct Facing72(u8);

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facing72Error {
    value: u8,
}

impl Facing72Error {
    pub const fn raw(self) -> u8 {
        self.value
    }
}

impl fmt::Display for Facing72Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "facing {} is out of range 0..=71", self.value)
    }
}

impl std::error::Error for Facing72Error {}

impl Facing72 {
    pub const MIN: u8 = 0;
    pub const MAX: u8 = 71;

    pub fn new(value: u8) -> Result<Self, Facing72Error> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(Facing72Error { value })
        }
    }

    pub const fn from_wrapped(value: u8) -> Self {
        Self(value % 72)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Facing72 {
    type Error = Facing72Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Facing72> for u8 {
    fn from(value: Facing72) -> Self {
        value.0
    }
}

#[cfg(feature = "admin-brp")]
#[bevy::reflect::reflect_remote(Point2D<f32, GlobalMeters>)]
#[derive(Debug, Clone, Copy)]
#[reflect(Debug)]
pub struct WorldPosReflect {
    pub x: f32,
    pub y: f32,
    #[reflect(ignore)]
    pub _unit: PhantomData<GlobalMeters>,
}

#[cfg(feature = "admin-brp")]
#[bevy::reflect::reflect_remote(Point2D<f32, LocalMeters>)]
#[derive(Debug, Clone, Copy)]
#[reflect(Debug)]
pub struct LocalPosReflect {
    pub x: f32,
    pub y: f32,
    #[reflect(ignore)]
    pub _unit: PhantomData<LocalMeters>,
}

#[cfg(feature = "admin-brp")]
#[bevy::reflect::reflect_remote(Size2D<f32, LocalMeters>)]
#[derive(Debug, Clone, Copy)]
#[reflect(Debug)]
pub struct LocalSizeReflect {
    pub width: f32,
    pub height: f32,
    #[reflect(ignore)]
    pub _unit: PhantomData<LocalMeters>,
}

#[cfg(feature = "admin-brp")]
#[bevy::reflect::reflect_remote(Box2D<f32, LocalMeters>)]
#[derive(Debug, Clone, Copy)]
#[reflect(Debug)]
pub struct LocalBoxReflect {
    #[reflect(remote = LocalPosReflect)]
    pub min: LocalPos,
    #[reflect(remote = LocalPosReflect)]
    pub max: LocalPos,
}

pub trait LocalPosExt {
    fn shifted(self, heading: LocalRotation, distance: LocalDistMeters) -> LocalPos;
}

impl LocalPosExt for LocalPos {
    fn shifted(self, heading: LocalRotation, distance: LocalDistMeters) -> LocalPos {
        self + heading.transform_vector(LocalVec::new(distance.get(), 0.0))
    }
}

pub trait LocalBoxExt {
    fn sample_pos(self, rng: &mut SmallRng) -> LocalPos;
    fn from_center_half_extent(center: LocalPos, extent: LocalSize) -> LocalBox;
    fn intersect(self, other: LocalBox) -> Option<LocalBox>;
    fn from_center_half_extent_uniform(center: LocalPos, extent_len: LocalDistMeters) -> LocalBox {
        Self::from_center_half_extent(center, LocalSize::new(extent_len.get(), extent_len.get()))
    }
}

impl LocalBoxExt for LocalBox {
    fn sample_pos(self, rng: &mut SmallRng) -> LocalPos {
        LocalPos::new(
            rng.random_range(self.min.x..=self.max.x),
            rng.random_range(self.min.y..=self.max.y),
        )
    }

    fn from_center_half_extent(center: LocalPos, extent: LocalSize) -> LocalBox {
        LocalBox::new(
            LocalPos::new(center.x - extent.width, center.y - extent.height),
            LocalPos::new(center.x + extent.width, center.y + extent.height),
        )
    }

    fn intersect(self, other: LocalBox) -> Option<LocalBox> {
        let min = self.min.max(other.min);
        let max = self.max.min(other.max);
        if min.x > max.x || min.y > max.y {
            return None;
        }
        Some(LocalBox::new(min, max))
    }
}
