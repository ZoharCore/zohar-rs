use crate::coords::{LocalBox, LocalPos, LocalSize};
use crate::{MobId, MobKind, MobRank};
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
        let bounds = LocalBox::new(
            LocalPos::new(center.x - extent.width, center.y - extent.height),
            LocalPos::new(center.x + extent.width, center.y + extent.height),
        );
        Self {
            center,
            extent,
            bounds,
        }
    }

    /// Generate a random point within the spawn area.
    pub fn random_point(&self, rng: &mut impl Rng) -> LocalPos {
        if self.extent.width == 0.0 && self.extent.height == 0.0 {
            self.center
        } else {
            LocalPos::new(
                rng.random_range(self.bounds.min.x..=self.bounds.max.x),
                rng.random_range(self.bounds.min.y..=self.bounds.max.y),
            )
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

#[derive(Debug)]
pub struct MobPrototypeDef {
    pub mob_id: MobId,
    pub mob_kind: MobKind,
    pub name: String,
    pub rank: MobRank,
    pub level: u32,
    pub move_speed: u8,
    pub attack_speed: u8,
    pub empire: Option<crate::Empire>,
}

impl MobPrototypeDef {
    pub fn placeholder() -> Self {
        Self {
            mob_id: MobId::new(101),
            mob_kind: MobKind::Monster,
            name: "mob_proto error".to_string(),
            rank: MobRank::Pawn,
            level: 1,
            move_speed: 0,
            attack_speed: 0,
            empire: None,
        }
    }
}

pub type MobPrototype = Arc<MobPrototypeDef>;

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn spawn_area_random_point_stays_in_bounds_and_zero_extent_returns_center() {
        let area = SpawnArea::new(LocalPos::new(10.0, 20.0), LocalSize::new(3.0, 4.0));
        let mut rng = SmallRng::seed_from_u64(7);

        for _ in 0..256 {
            let point = area.random_point(&mut rng);
            assert!(point.x >= area.bounds.min.x && point.x <= area.bounds.max.x);
            assert!(point.y >= area.bounds.min.y && point.y <= area.bounds.max.y);
        }

        let zero = SpawnArea::new(LocalPos::new(5.0, 6.0), LocalSize::new(0.0, 0.0));
        assert_eq!(zero.random_point(&mut rng), LocalPos::new(5.0, 6.0));
    }
}
