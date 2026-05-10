mod grid;
mod planner;
mod reachability;
mod segment;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use zohar_domain::coords::LocalPos;

pub use grid::{GridCell, TerrainFlagsGrid};
pub use planner::NavPath;

use planner::LocalPlanScope;
use reachability::{ReachabilityGrid, WalkabilityView};

#[derive(Debug)]
pub struct MapNavigator {
    walkability: WalkabilityView,
    reachability: ReachabilityGrid,
}

impl MapNavigator {
    pub fn new(terrain: TerrainFlagsGrid) -> Self {
        let walkability = WalkabilityView::new(Arc::new(terrain));
        let reachability = ReachabilityGrid::new(&walkability);
        Self {
            walkability,
            reachability,
        }
    }

    pub fn can_stand(&self, pos: LocalPos) -> bool {
        segment::can_stand(&self.walkability, pos)
    }

    pub fn segment_clear(&self, start: LocalPos, goal: LocalPos) -> bool {
        segment::segment_clear(&self.walkability, start, goal)
    }

    pub fn clip_segment(&self, start: LocalPos, goal: LocalPos) -> LocalPos {
        segment::clip_segment(&self.walkability, start, goal)
    }

    pub fn same_component(&self, start: LocalPos, goal: LocalPos) -> bool {
        self.resolve_endpoints(start, goal)
            .is_some_and(|route_cells| self.cells_share_component(route_cells))
    }

    pub fn next_waypoint(&self, start: LocalPos, goal: LocalPos) -> Option<LocalPos> {
        self.next_waypoint_with_scope(start, goal, LocalPlanScope::default())
    }

    pub fn find_path(&self, start: LocalPos, goal: LocalPos) -> Option<NavPath> {
        self.find_path_with_scope(start, goal, LocalPlanScope::default())
    }

    fn next_waypoint_with_scope(
        &self,
        start: LocalPos,
        goal: LocalPos,
        scope: LocalPlanScope,
    ) -> Option<LocalPos> {
        match self.resolve_route(start, goal, scope) {
            RouteResolution::Direct(_) => Some(goal),
            RouteResolution::Planned(cells) => {
                let path = self.build_path(&cells, start, goal);
                next_waypoint_from_path(&path)
            }
            RouteResolution::InvalidEndpoints
            | RouteResolution::OutOfScope
            | RouteResolution::Unreachable => None,
        }
    }

    fn find_path_with_scope(
        &self,
        start: LocalPos,
        goal: LocalPos,
        scope: LocalPlanScope,
    ) -> Option<NavPath> {
        match self.resolve_route(start, goal, scope) {
            RouteResolution::Direct(route_cells) => {
                Some(self.build_direct_path(route_cells, start, goal))
            }
            RouteResolution::Planned(cells) => Some(self.build_path(&cells, start, goal)),
            RouteResolution::InvalidEndpoints
            | RouteResolution::OutOfScope
            | RouteResolution::Unreachable => None,
        }
    }

    fn resolve_route(
        &self,
        start: LocalPos,
        goal: LocalPos,
        scope: LocalPlanScope,
    ) -> RouteResolution {
        let Some(route_cells) = self.resolve_endpoints(start, goal) else {
            return RouteResolution::InvalidEndpoints;
        };

        if self.segment_clear(start, goal) {
            return RouteResolution::Direct(route_cells);
        }
        if !scope.goal_is_in_scope(start, goal) {
            return RouteResolution::OutOfScope;
        }
        if !self.cells_share_component(route_cells) {
            return RouteResolution::Unreachable;
        }

        let Some(cells) = planner::find_local_cell_path(
            &self.walkability,
            &self.reachability,
            route_cells.start_cell,
            route_cells.goal_cell,
            scope,
        ) else {
            return RouteResolution::Unreachable;
        };

        RouteResolution::Planned(cells)
    }

    fn resolve_endpoints(&self, start: LocalPos, goal: LocalPos) -> Option<RouteCells> {
        Some(RouteCells {
            start_cell: self.terrain().cell_for_point(start)?,
            goal_cell: self.terrain().cell_for_point(goal)?,
        })
    }

    fn cells_share_component(&self, route_cells: RouteCells) -> bool {
        self.reachability
            .same_component(route_cells.start_cell, route_cells.goal_cell)
    }

    fn build_direct_path(
        &self,
        route_cells: RouteCells,
        start: LocalPos,
        goal: LocalPos,
    ) -> NavPath {
        self.build_path(
            &[route_cells.start_cell, route_cells.goal_cell],
            start,
            goal,
        )
    }

    fn build_path(&self, cells: &[GridCell], start: LocalPos, goal: LocalPos) -> NavPath {
        planner::prune_cells_to_path(self.terrain(), cells, start, goal, |from, to| {
            self.segment_clear(from, to)
        })
    }

    fn terrain(&self) -> &TerrainFlagsGrid {
        self.walkability.terrain()
    }
}

#[derive(Debug, Clone, Copy)]
struct RouteCells {
    start_cell: GridCell,
    goal_cell: GridCell,
}

#[derive(Debug)]
enum RouteResolution {
    InvalidEndpoints,
    Direct(RouteCells),
    OutOfScope,
    Unreachable,
    Planned(Vec<GridCell>),
}

fn next_waypoint_from_path(path: &NavPath) -> Option<LocalPos> {
    path.waypoints
        .get(1)
        .copied()
        .or_else(|| path.waypoints.last().copied())
}
