use std::collections::BTreeSet;

use super::Stat;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSet<S> {
    inner: BTreeSet<S>,
}

pub type StatChangeSet = ChangeSet<Stat>;

impl<S> Default for ChangeSet<S> {
    fn default() -> Self {
        Self {
            inner: BTreeSet::new(),
        }
    }
}

impl<S> ChangeSet<S>
where
    S: Ord + Copy,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, stat: S) -> bool {
        self.inner.contains(&stat)
    }

    pub fn insert(&mut self, stat: S) {
        self.inner.insert(stat);
    }

    pub fn extend(&mut self, stats: impl IntoIterator<Item = S>) {
        self.inner.extend(stats);
    }

    pub fn iter(&self) -> impl Iterator<Item = S> + '_ {
        self.inner.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}
