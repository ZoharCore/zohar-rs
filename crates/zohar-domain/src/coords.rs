use euclid::{Box2D, Length, Point2D, Rotation2D, Size2D, Vector2D};
use rand::RngExt;
use rand::prelude::SmallRng;
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
