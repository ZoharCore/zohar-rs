use std::ops::{Index, IndexMut};

use super::Stat;
use crate::stats::core::EnumIntegerValueStore;

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PointValueStore {
    points: EnumIntegerValueStore<Stat>,
}

impl PointValueStore {
    pub fn new() -> Self {
        Self {
            points: EnumIntegerValueStore::new(),
        }
    }

    pub fn clear(&mut self) {
        self.points.clear();
    }

    pub fn get(&self, point: Stat) -> i32 {
        self.points.get(point)
    }

    pub fn set(&mut self, point: Stat, value: i32) {
        self.points.set(point, value);
    }

    pub fn add(&mut self, point: Stat, delta: i32) {
        self.points.add(point, delta);
    }

    pub fn iter(&self) -> impl Iterator<Item = (Stat, i32)> + '_ {
        self.points.iter()
    }
}

impl Index<Stat> for PointValueStore {
    type Output = i32;

    fn index(&self, index: Stat) -> &Self::Output {
        &self.points[index]
    }
}

impl IndexMut<Stat> for PointValueStore {
    fn index_mut(&mut self, index: Stat) -> &mut Self::Output {
        &mut self.points[index]
    }
}
