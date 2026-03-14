use std::collections::VecDeque;
use std::sync::Arc;

use super::grid::{GridCell, TerrainFlagsGrid};

const UNASSIGNED_COMPONENT: u32 = 0;

#[derive(Debug, Clone)]
pub(crate) struct WalkabilityView {
    terrain: Arc<TerrainFlagsGrid>,
}

impl WalkabilityView {
    pub(crate) fn new(terrain: Arc<TerrainFlagsGrid>) -> Self {
        Self { terrain }
    }

    pub(crate) fn terrain(&self) -> &TerrainFlagsGrid {
        &self.terrain
    }

    pub(crate) fn is_walkable_cell(&self, cell: GridCell) -> bool {
        self.terrain
            .flags_at(cell)
            .is_some_and(|flags| flags.is_walkable())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ReachabilityGrid {
    width: usize,
    height: usize,
    components: Box<[u32]>,
}

impl ReachabilityGrid {
    /// Label each 4-connected walkable component with a component id.
    pub(crate) fn new(walkability: &WalkabilityView) -> Self {
        let terrain = walkability.terrain();
        let mut components = vec![UNASSIGNED_COMPONENT; terrain.width() * terrain.height()];
        let mut next_component_id: u32 = 1;

        for cell in terrain.all_cells() {
            let idx = terrain
                .index_of(cell)
                .expect("all_cells must stay in bounds");

            if components[idx] != UNASSIGNED_COMPONENT || !walkability.is_walkable_cell(cell) {
                continue;
            }

            Self::flood_fill_component(
                terrain,
                walkability,
                &mut components,
                cell,
                next_component_id,
            );
            next_component_id = next_component_id.saturating_add(1);
        }

        Self {
            width: terrain.width(),
            height: terrain.height(),
            components: components.into_boxed_slice(),
        }
    }

    pub(crate) fn component_for_cell(&self, cell: GridCell) -> Option<u32> {
        if cell.x >= self.width || cell.y >= self.height {
            return None;
        }

        let component = self.components[cell.y * self.width + cell.x];
        (component != UNASSIGNED_COMPONENT).then_some(component)
    }

    pub(crate) fn same_component(&self, start: GridCell, goal: GridCell) -> bool {
        match (
            self.component_for_cell(start),
            self.component_for_cell(goal),
        ) {
            (Some(start_component), Some(goal_component)) => start_component == goal_component,
            _ => false,
        }
    }

    fn flood_fill_component(
        terrain: &TerrainFlagsGrid,
        walkability: &WalkabilityView,
        components: &mut [u32],
        seed: GridCell,
        component_id: u32,
    ) {
        let mut queue = VecDeque::from([seed]);
        let seed_idx = terrain.index_of(seed).expect("seed must stay in bounds");
        components[seed_idx] = component_id;

        while let Some(current) = queue.pop_front() {
            for neighbor in terrain.neighbors4(current) {
                let neighbor_idx = terrain
                    .index_of(neighbor)
                    .expect("neighbors4 must stay in bounds");

                if components[neighbor_idx] != UNASSIGNED_COMPONENT
                    || !walkability.is_walkable_cell(neighbor)
                {
                    continue;
                }

                components[neighbor_idx] = component_id;
                queue.push_back(neighbor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{ReachabilityGrid, WalkabilityView};
    use crate::navigation::{GridCell, TerrainFlagsGrid};
    use zohar_domain::TerrainFlags;

    fn test_grid(
        width: usize,
        height: usize,
        blocked_cells: &[(usize, usize)],
    ) -> TerrainFlagsGrid {
        let mut flags = vec![TerrainFlags::empty(); width * height];
        for (x, y) in blocked_cells.iter().copied() {
            flags[y * width + x] = TerrainFlags::BLOCK;
        }
        TerrainFlagsGrid::new(1.0, width, height, flags).expect("terrain flags grid")
    }

    #[test]
    fn separates_disconnected_regions() {
        let walkability =
            WalkabilityView::new(Arc::new(test_grid(3, 3, &[(1, 0), (1, 1), (1, 2)])));
        let reachability = ReachabilityGrid::new(&walkability);

        assert!(reachability.same_component(GridCell { x: 0, y: 0 }, GridCell { x: 0, y: 2 }));
        assert!(!reachability.same_component(GridCell { x: 0, y: 1 }, GridCell { x: 2, y: 1 }));
    }
}
