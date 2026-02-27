//! Area of Interest (AOI) spatial indexing for entity visibility and game logic queries.
//!
//! Provides a shared spatial store for:
//! - visibility candidate lookup
//! - game logic range queries (AOE, sensors, etc.)

use flat_spatial::Grid;
use flat_spatial::grid::GridHandle;
use std::collections::HashMap;
use zohar_domain::coords::LocalPos;
use zohar_domain::entity::EntityId;

/// Cell size for spatial grid partitioning.
/// Optimal: roughly half of typical query radius to minimize cell iterations.
pub const CELL_SIZE: i32 = 55;

// ============================================================================
// Types
// ============================================================================

/// The underlying spatial grid storing EntityId at each position.
type InnerGrid = Grid<EntityId, LocalPos>;

// ============================================================================
// SpatialIndex
// ============================================================================

/// Shared spatial store for visibility candidate and gameplay queries.
///
/// Wraps `flat_spatial::Grid` with:
/// - Bidirectional EntityId ↔ GridHandle mapping
/// - Generic radius query methods
/// - Future: region-based observer tracking for wake/sleep
pub struct SpatialIndex {
    grid: InnerGrid,
    handles: HashMap<EntityId, GridHandle>,
}

impl Default for SpatialIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialIndex {
    /// Creates a new empty spatial index with default cell size.
    pub fn new() -> Self {
        Self {
            grid: InnerGrid::new(CELL_SIZE),
            handles: HashMap::new(),
        }
    }

    /// Creates a spatial index with a custom cell size.
    pub fn with_cell_size(cell_size: i32) -> Self {
        Self {
            grid: InnerGrid::new(cell_size),
            handles: HashMap::new(),
        }
    }

    /// Insert an entity at a position. Returns the grid handle.
    pub fn insert(&mut self, entity_id: EntityId, pos: LocalPos) -> GridHandle {
        let handle = self.grid.insert(pos, entity_id);
        self.handles.insert(entity_id, handle);
        handle
    }

    /// Update an entity's position (lazy - applied on maintain).
    pub fn update_position(&mut self, entity_id: EntityId, pos: LocalPos) {
        if let Some(&handle) = self.handles.get(&entity_id) {
            self.grid.set_position(handle, pos);
        }
    }

    /// Remove an entity from the spatial index.
    pub fn remove(&mut self, entity_id: EntityId) -> Option<GridHandle> {
        if let Some(handle) = self.handles.remove(&entity_id) {
            self.grid.remove_maintain(handle);
            Some(handle)
        } else {
            None
        }
    }

    /// Apply pending position updates. Call once per tick.
    pub fn maintain(&mut self) {
        self.grid.maintain();
    }

    /// Query entities within a radius of a position.
    ///
    /// Returns an iterator over EntityIds within the specified range.
    /// Used for visibility candidate lookup and game logic range queries.
    pub fn query_in_radius(
        &self,
        center: LocalPos,
        radius: f32,
    ) -> impl Iterator<Item = EntityId> + '_ {
        self.grid
            .query_around(center, radius)
            .map(|(handle, _pos)| {
                *self
                    .grid
                    .get(handle)
                    .expect("handle from query must exist")
                    .1
            })
    }

    /// Get the handle for an entity, if it exists in the index.
    pub fn get_handle(&self, entity_id: EntityId) -> Option<GridHandle> {
        self.handles.get(&entity_id).copied()
    }

    /// Check if an entity exists in the index.
    pub fn contains(&self, entity_id: EntityId) -> bool {
        self.handles.contains_key(&entity_id)
    }

    /// Number of entities in the index.
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Returns true if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

// ============================================================================
// Helpers
// ============================================================================

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query_finds_entity() {
        let mut index = SpatialIndex::new();
        let entity = EntityId(1);
        let pos = LocalPos::new(10.0, 10.0);

        index.insert(entity, pos);

        let found: Vec<_> = index.query_in_radius(pos, 120.0).collect();
        assert!(found.contains(&entity));
    }

    #[test]
    fn query_respects_radius() {
        let mut index = SpatialIndex::new();
        let near = EntityId(1);
        let far = EntityId(2);

        index.insert(near, LocalPos::new(0.0, 0.0));
        index.insert(far, LocalPos::new(100.0, 100.0)); // ~141m diagonal

        let found: Vec<_> = index
            .query_in_radius(LocalPos::new(0.0, 0.0), 10.0)
            .collect();
        assert!(found.contains(&near));
        assert!(!found.contains(&far));
    }

    #[test]
    fn remove_entity_excludes_from_query() {
        let mut index = SpatialIndex::new();
        let entity = EntityId(1);
        let pos = LocalPos::new(5.0, 5.0);

        index.insert(entity, pos);
        index.remove(entity);

        let found: Vec<_> = index.query_in_radius(pos, 120.0).collect();
        assert!(!found.contains(&entity));
    }

    #[test]
    fn update_position_works_after_maintain() {
        let mut index = SpatialIndex::new();
        let entity = EntityId(1);

        index.insert(entity, LocalPos::new(0.0, 0.0));
        index.update_position(entity, LocalPos::new(100.0, 100.0));
        index.maintain();

        // Should NOT be found at old position
        let at_old: Vec<_> = index
            .query_in_radius(LocalPos::new(0.0, 0.0), 5.0)
            .collect();
        assert!(!at_old.contains(&entity));

        // Should be found at new position
        let at_new: Vec<_> = index
            .query_in_radius(LocalPos::new(100.0, 100.0), 5.0)
            .collect();
        assert!(at_new.contains(&entity));
    }
}
