//! Local planning over walkable grid cells.

use pathfinding::prelude::astar;
use zohar_domain::coords::LocalPos;

use super::grid::{GridCell, TerrainFlagsGrid};
use super::reachability::{ReachabilityGrid, WalkabilityView};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalPlanScope {
    pub local_search_radius_m: f32,
}

impl Default for LocalPlanScope {
    fn default() -> Self {
        Self {
            local_search_radius_m: 20.0,
        }
    }
}

impl LocalPlanScope {
    pub(crate) fn search_radius_cells(self, terrain: &TerrainFlagsGrid) -> Option<usize> {
        if !self.local_search_radius_m.is_finite() || self.local_search_radius_m <= 0.0 {
            return None;
        }

        let cells = (self.local_search_radius_m * terrain.inv_cell_size_m).ceil();
        if !cells.is_finite() || cells < 1.0 || cells > usize::MAX as f32 {
            return None;
        }

        Some(cells as usize)
    }

    pub(crate) fn goal_is_in_scope(self, start: LocalPos, goal: LocalPos) -> bool {
        let distance = (goal.x - start.x).hypot(goal.y - start.y);
        distance.is_finite() && distance <= self.local_search_radius_m
    }
}

pub(crate) fn find_local_cell_path(
    walkability: &WalkabilityView,
    reachability: &ReachabilityGrid,
    start: GridCell,
    goal: GridCell,
    scope: LocalPlanScope,
) -> Option<Vec<GridCell>> {
    if !walkability.is_walkable_cell(start)
        || !walkability.is_walkable_cell(goal)
        || !reachability.same_component(start, goal)
    {
        return None;
    }

    let terrain = walkability.terrain();
    let bounds = terrain.search_bounds(start, scope.search_radius_cells(terrain)?);
    if !bounds.contains(goal) {
        return None;
    }

    let result = astar(
        &start,
        |cell| {
            terrain
                .neighbors4(*cell)
                .filter(|&neighbor| {
                    bounds.contains(neighbor) && walkability.is_walkable_cell(neighbor)
                })
                .map(|neighbor| (neighbor, 1usize))
                .collect::<Vec<_>>()
        },
        |cell| cell.manhattan_distance(goal),
        |cell| *cell == goal,
    )?;

    Some(result.0)
}

#[derive(Debug, Clone)]
pub struct NavPath {
    pub cells: Vec<GridCell>,
    pub waypoints: Vec<LocalPos>,
}

pub(crate) fn prune_cells_to_path(
    terrain: &TerrainFlagsGrid,
    cells: &[GridCell],
    start: LocalPos,
    goal: LocalPos,
    mut segment_clear: impl FnMut(LocalPos, LocalPos) -> bool,
) -> NavPath {
    if cells.is_empty() {
        return NavPath {
            cells: Vec::new(),
            waypoints: Vec::new(),
        };
    }

    let mut anchor_positions = Vec::with_capacity(cells.len());
    for (idx, cell) in cells.iter().copied().enumerate() {
        if idx == 0 {
            anchor_positions.push(start);
        } else if idx + 1 == cells.len() {
            anchor_positions.push(goal);
        } else if let Some(center) = terrain.cell_center(cell) {
            anchor_positions.push(center);
        }
    }
    if anchor_positions.len() == 1 && start != goal {
        anchor_positions.push(goal);
    }

    let mut waypoints = vec![anchor_positions[0]];
    let mut anchor_idx = 0usize;

    // Greedily skip intermediate anchors while line of sight remains clear.
    while anchor_idx + 1 < anchor_positions.len() {
        let mut furthest = anchor_idx + 1;
        while furthest + 1 < anchor_positions.len()
            && segment_clear(anchor_positions[anchor_idx], anchor_positions[furthest + 1])
        {
            furthest += 1;
        }
        waypoints.push(anchor_positions[furthest]);
        anchor_idx = furthest;
    }

    NavPath {
        cells: cells.to_vec(),
        waypoints,
    }
}

#[cfg(test)]
mod tests {
    use zohar_domain::TerrainFlags;
    use zohar_domain::coords::LocalPos;

    use super::prune_cells_to_path;
    use crate::navigation::{GridCell, TerrainFlagsGrid};

    fn test_grid(width: usize, height: usize) -> TerrainFlagsGrid {
        TerrainFlagsGrid::new(
            1.0,
            width,
            height,
            vec![TerrainFlags::empty(); width * height],
        )
        .expect("terrain flags grid")
    }

    #[test]
    fn pruning_shortens_clear_stretches() {
        let terrain = test_grid(8, 1);
        let cells = vec![
            GridCell { x: 0, y: 0 },
            GridCell { x: 1, y: 0 },
            GridCell { x: 2, y: 0 },
            GridCell { x: 3, y: 0 },
        ];
        let path = prune_cells_to_path(
            &terrain,
            &cells,
            LocalPos::new(0.1, 0.1),
            LocalPos::new(3.9, 0.1),
            |_from, _to| true,
        );

        assert_eq!(
            path.waypoints,
            vec![LocalPos::new(0.1, 0.1), LocalPos::new(3.9, 0.1)]
        );
    }
}
