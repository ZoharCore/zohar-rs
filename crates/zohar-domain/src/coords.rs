use euclid::{Box2D, Point2D, Size2D};

pub struct GlobalSpace;
pub type WorldPos = Point2D<f32, GlobalSpace>;

pub struct LocalSpace;
pub type LocalPos = Point2D<f32, LocalSpace>;
pub type LocalSize = Size2D<f32, LocalSpace>;
pub type LocalBox = Box2D<f32, LocalSpace>;
