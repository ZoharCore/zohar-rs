use std::ops::{Index, IndexMut};

use enum_map::{EnumArray, EnumMap, enum_map};

#[cfg_attr(feature = "admin-brp", derive(bevy::prelude::Reflect))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumIntegerValueStore<K>
where
    K: EnumArray<i32> + Copy,
    <K as EnumArray<i32>>::Array: Clone + PartialEq + Eq,
{
    values: EnumMap<K, i32>,
}

impl<K> Default for EnumIntegerValueStore<K>
where
    K: EnumArray<i32> + Copy,
    <K as EnumArray<i32>>::Array: Clone + PartialEq + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K> EnumIntegerValueStore<K>
where
    K: EnumArray<i32> + Copy,
    <K as EnumArray<i32>>::Array: Clone + PartialEq + Eq,
{
    pub fn new() -> Self {
        Self {
            values: enum_map! { _ => 0 },
        }
    }

    pub fn clear(&mut self) {
        self.values = enum_map! { _ => 0 };
    }

    pub fn get(&self, key: K) -> i32 {
        self.values[key]
    }

    pub fn set(&mut self, key: K, value: i32) {
        self.values[key] = value;
    }

    pub fn add(&mut self, key: K, delta: i32) {
        self.values[key] += delta;
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, i32)> + '_ {
        self.values
            .iter()
            .filter_map(|(key, value)| (*value != 0).then_some((key, *value)))
    }
}

impl<K> Index<K> for EnumIntegerValueStore<K>
where
    K: EnumArray<i32> + Copy,
    <K as EnumArray<i32>>::Array: Clone + PartialEq + Eq,
{
    type Output = i32;

    fn index(&self, index: K) -> &Self::Output {
        &self.values[index]
    }
}

impl<K> IndexMut<K> for EnumIntegerValueStore<K>
where
    K: EnumArray<i32> + Copy,
    <K as EnumArray<i32>>::Array: Clone + PartialEq + Eq,
{
    fn index_mut(&mut self, index: K) -> &mut Self::Output {
        &mut self.values[index]
    }
}

#[cfg(test)]
mod tests {
    use enum_map::Enum;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Enum)]
    enum TestKey {
        Alpha,
        Beta,
    }

    #[test]
    fn stores_values_by_enum_key() {
        let mut store = EnumIntegerValueStore::<TestKey>::new();
        store.set(TestKey::Alpha, 42);
        store.add(TestKey::Alpha, 1);

        assert_eq!(store.get(TestKey::Alpha), 43);
    }

    #[test]
    fn supports_index_syntax() {
        let mut store = EnumIntegerValueStore::<TestKey>::new();
        store[TestKey::Beta] = 10;
        store[TestKey::Beta] += 5;

        assert_eq!(store[TestKey::Beta], 15);
    }
}
