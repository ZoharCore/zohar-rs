use crate::coords::{LocalBox, LocalBoxExt, LocalPos, LocalSize};
use crate::entity::mob::MobId;
use rand::{Rng, RngExt};
use std::sync::Arc;
use std::time::Duration;

/// One of the eight cardinal/diagonal directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Direction {
    pub fn from_content_raw(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::North),
            2 => Some(Self::NorthEast),
            3 => Some(Self::East),
            4 => Some(Self::SouthEast),
            5 => Some(Self::South),
            6 => Some(Self::SouthWest),
            7 => Some(Self::West),
            8 => Some(Self::NorthWest),
            _ => None,
        }
    }

    pub fn to_angle(self) -> f32 {
        match self {
            Self::North => 0.0,
            Self::NorthEast => 45.0,
            Self::East => 90.0,
            Self::SouthEast => 135.0,
            Self::South => 180.0,
            Self::SouthWest => 225.0,
            Self::West => 270.0,
            Self::NorthWest => 315.0,
        }
    }

    pub fn random(rng: &mut impl Rng) -> Self {
        match rng.random_range(0..8) {
            0 => Self::North,
            1 => Self::NorthEast,
            2 => Self::East,
            3 => Self::SouthEast,
            4 => Self::South,
            5 => Self::SouthWest,
            6 => Self::West,
            _ => Self::NorthWest,
        }
    }
}

/// How a spawned entity should face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacingStrategy {
    /// Pick a random direction at spawn time.
    Random,
    /// Face a specific direction.
    Fixed(Direction),
}

/// Spawn area with pre-computed bounding box.
#[derive(Debug, Clone)]
pub struct SpawnArea {
    pub center: LocalPos,
    pub extent: LocalSize,
    pub bounds: LocalBox,
}

impl SpawnArea {
    /// Create a spawn area from center and extent, pre-computing bounds.
    pub fn new(center: LocalPos, extent: LocalSize) -> Self {
        let bounds = LocalBox::from_center_half_extent(center, extent);
        Self {
            center,
            extent,
            bounds,
        }
    }
}

/// Immutable spawn rule definition (derived from content).
#[derive(Debug, Clone)]
pub struct WeightedGroupChoice {
    pub members: Arc<[MobId]>,
    pub weight: u32,
}

#[derive(Debug, Clone)]
pub enum SpawnTemplate {
    Mob(MobId),
    Group(Arc<[MobId]>),
    GroupGroup(Arc<[WeightedGroupChoice]>),
}

#[derive(Debug)]
pub struct SpawnRuleDef {
    pub template: SpawnTemplate,
    pub area: SpawnArea,
    pub facing: FacingStrategy,
    pub max_count: usize,
    pub regen_time: Duration,
}

/// Shared reference to a spawn rule definition.
pub type SpawnRule = Arc<SpawnRuleDef>;

#[cfg(test)]
mod tests {
    use crate::entity::mob::spawn::Direction;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn direction_to_angle_covers_all_variants() {
        assert_eq!(Direction::North.to_angle(), 0.0);
        assert_eq!(Direction::NorthEast.to_angle(), 45.0);
        assert_eq!(Direction::East.to_angle(), 90.0);
        assert_eq!(Direction::SouthEast.to_angle(), 135.0);
        assert_eq!(Direction::South.to_angle(), 180.0);
        assert_eq!(Direction::SouthWest.to_angle(), 225.0);
        assert_eq!(Direction::West.to_angle(), 270.0);
        assert_eq!(Direction::NorthWest.to_angle(), 315.0);
    }

    #[test]
    fn direction_random_is_always_valid() {
        let mut rng = SmallRng::seed_from_u64(42);
        for _ in 0..256 {
            let dir = Direction::random(&mut rng);
            assert!((0.0..360.0).contains(&dir.to_angle()));
        }
    }
}
