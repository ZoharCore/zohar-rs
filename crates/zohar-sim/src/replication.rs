//! Replication ownership state for AOI visibility.
//!
//! This module tracks which observer currently owns a replicated view of a target entity.
//! Once an edge is visible, the caller is responsible for sending target updates until
//! the edge transitions back to hidden.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use zohar_domain::entity::EntityId;

/// Distance hysteresis settings for AOI visibility.
#[derive(Debug, Clone, Copy)]
pub struct InterestConfig {
    pub spawn_radius: f32,
    pub despawn_radius: f32,
}

impl Default for InterestConfig {
    fn default() -> Self {
        Self {
            spawn_radius: 110.0,
            despawn_radius: 120.0,
        }
    }
}

/// Result of reconciling one observer against current AOI candidates.
#[derive(Debug, Default)]
pub struct VisibilityDiff {
    pub entered: Vec<EntityId>,
    pub left: Vec<EntityId>,
}

/// Directed replication graph:
/// - observer -> visible targets
/// - target -> current observers
#[derive(Debug, Default)]
pub struct ReplicationGraph {
    visible_by_observer: HashMap<EntityId, HashSet<EntityId>>,
    observers_by_target: HashMap<EntityId, HashSet<EntityId>>,
}

impl ReplicationGraph {
    /// Reconcile one observer's visible set using hysteresis candidate sets.
    ///
    /// - `spawn_candidates`: entities inside spawn radius (eligible to newly enter).
    /// - `retain_candidates`: entities inside despawn radius (eligible to remain visible).
    pub fn reconcile_observer(
        &mut self,
        observer: EntityId,
        spawn_candidates: &HashSet<EntityId>,
        retain_candidates: &HashSet<EntityId>,
    ) -> VisibilityDiff {
        let old_visible = self
            .visible_by_observer
            .get(&observer)
            .cloned()
            .unwrap_or_default();

        let new_visible: HashSet<EntityId> = old_visible
            .intersection(retain_candidates)
            .copied()
            .chain(spawn_candidates.iter().copied())
            .collect();

        let entered: Vec<EntityId> = new_visible.difference(&old_visible).copied().collect();
        let left: Vec<EntityId> = old_visible.difference(&new_visible).copied().collect();

        for target in &entered {
            self.observers_by_target
                .entry(*target)
                .or_default()
                .insert(observer);
        }

        for target in &left {
            if let Some(observers) = self.observers_by_target.get_mut(target) {
                observers.remove(&observer);
                if observers.is_empty() {
                    self.observers_by_target.remove(target);
                }
            }
        }

        if new_visible.is_empty() {
            self.visible_by_observer.remove(&observer);
        } else {
            self.visible_by_observer.insert(observer, new_visible);
        }

        VisibilityDiff { entered, left }
    }

    /// Remove an observer row and all reverse edges.
    pub fn remove_observer(&mut self, observer: EntityId) -> Vec<EntityId> {
        let Some(targets) = self.visible_by_observer.remove(&observer) else {
            return Vec::new();
        };

        for target in &targets {
            if let Some(observers) = self.observers_by_target.get_mut(target) {
                observers.remove(&observer);
                if observers.is_empty() {
                    self.observers_by_target.remove(target);
                }
            }
        }

        targets.into_iter().collect()
    }

    /// Remove a target column and all reverse edges. Returns prior observers.
    pub fn remove_target(&mut self, target: EntityId) -> Vec<EntityId> {
        let Some(observers) = self.observers_by_target.remove(&target) else {
            return Vec::new();
        };

        for observer in &observers {
            if let Some(targets) = self.visible_by_observer.get_mut(observer) {
                targets.remove(&target);
                if targets.is_empty() {
                    self.visible_by_observer.remove(observer);
                }
            }
        }

        observers.into_iter().collect()
    }

    /// Remove one directed observer -> target visibility edge.
    pub fn remove_visibility(&mut self, observer: EntityId, target: EntityId) -> bool {
        let mut removed = false;

        if let Some(targets) = self.visible_by_observer.get_mut(&observer) {
            removed = targets.remove(&target);
            if targets.is_empty() {
                self.visible_by_observer.remove(&observer);
            }
        }

        if removed && let Some(observers) = self.observers_by_target.get_mut(&target) {
            observers.remove(&observer);
            if observers.is_empty() {
                self.observers_by_target.remove(&target);
            }
        }

        removed
    }

    /// Snapshot all observers currently receiving a given target.
    #[allow(dead_code)]
    pub fn observers_for(&self, target: EntityId) -> Vec<EntityId> {
        self.observers_by_target
            .get(&target)
            .map(|observers| observers.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn is_visible(&self, observer: EntityId, target: EntityId) -> bool {
        self.visible_by_observer
            .get(&observer)
            .is_some_and(|targets| targets.contains(&target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconcile_applies_hysteresis_and_updates_reverse_index() {
        let mut graph = ReplicationGraph::default();
        let observer = EntityId(1);
        let near = EntityId(2);
        let far = EntityId(3);

        let spawn: HashSet<_> = [near].into_iter().collect();
        let retain: HashSet<_> = [near, far].into_iter().collect();
        let diff = graph.reconcile_observer(observer, &spawn, &retain);
        assert_eq!(diff.entered, vec![near]);
        assert!(diff.left.is_empty());
        assert!(graph.is_visible(observer, near));
        assert_eq!(graph.observers_for(near), vec![observer]);

        // Near left spawn range but is still in retain range -> must stay visible.
        let spawn: HashSet<EntityId> = HashSet::new();
        let retain: HashSet<_> = [near].into_iter().collect();
        let diff = graph.reconcile_observer(observer, &spawn, &retain);
        assert!(diff.entered.is_empty());
        assert!(diff.left.is_empty());
        assert!(graph.is_visible(observer, near));
    }

    #[test]
    fn remove_target_and_observer_drop_both_indexes() {
        let mut graph = ReplicationGraph::default();
        let o1 = EntityId(1);
        let o2 = EntityId(2);
        let t = EntityId(9);

        let spawn: HashSet<_> = [t].into_iter().collect();
        let retain: HashSet<_> = [t].into_iter().collect();
        graph.reconcile_observer(o1, &spawn, &retain);
        graph.reconcile_observer(o2, &spawn, &retain);

        let mut observers = graph.remove_target(t);
        observers.sort_unstable_by_key(|id| id.0);
        assert_eq!(observers, vec![o1, o2]);
        assert!(!graph.is_visible(o1, t));
        assert!(!graph.is_visible(o2, t));

        // No-op after target removal.
        assert!(graph.remove_observer(o1).is_empty());
    }

    #[test]
    fn remove_visibility_drops_only_one_directed_edge() {
        let mut graph = ReplicationGraph::default();
        let o1 = EntityId(1);
        let o2 = EntityId(2);
        let t = EntityId(9);

        let spawn: HashSet<_> = [t].into_iter().collect();
        let retain: HashSet<_> = [t].into_iter().collect();
        graph.reconcile_observer(o1, &spawn, &retain);
        graph.reconcile_observer(o2, &spawn, &retain);

        assert!(graph.remove_visibility(o1, t));
        assert!(!graph.is_visible(o1, t));
        assert!(graph.is_visible(o2, t));
        assert_eq!(graph.observers_for(t), vec![o2]);
    }
}
