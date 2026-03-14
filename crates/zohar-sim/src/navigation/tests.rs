use zohar_domain::TerrainFlags;
use zohar_domain::coords::LocalPos;

use super::*;

fn test_grid(width: usize, height: usize, blocked_cells: &[(usize, usize)]) -> TerrainFlagsGrid {
    let mut flags = vec![TerrainFlags::empty(); width * height];
    for (x, y) in blocked_cells.iter().copied() {
        flags[y * width + x] = TerrainFlags::BLOCK;
    }
    TerrainFlagsGrid::new(1.0, width, height, flags).expect("terrain flags grid")
}

#[test]
fn next_waypoint_skips_planner_for_direct_segments() {
    let navigator = MapNavigator::new(test_grid(4, 1, &[]));
    assert_eq!(
        navigator.next_waypoint(LocalPos::new(0.1, 0.1), LocalPos::new(3.9, 0.1)),
        Some(LocalPos::new(3.9, 0.1))
    );
}

#[test]
fn planner_routes_around_blockers() {
    let navigator = MapNavigator::new(test_grid(5, 5, &[(2, 1), (2, 2), (2, 3)]));
    let path = navigator
        .find_path(LocalPos::new(0.5, 2.5), LocalPos::new(4.5, 2.5))
        .expect("path");
    assert!(path.cells.len() > 5, "path should route around wall");
    assert_eq!(
        path.waypoints.first().copied(),
        Some(LocalPos::new(0.5, 2.5))
    );
    assert_eq!(
        path.waypoints.last().copied(),
        Some(LocalPos::new(4.5, 2.5))
    );
}

#[test]
fn planner_returns_none_for_disconnected_regions() {
    let navigator = MapNavigator::new(test_grid(3, 3, &[(1, 0), (1, 1), (1, 2)]));
    assert!(
        navigator
            .find_path(LocalPos::new(0.5, 1.5), LocalPos::new(2.5, 1.5))
            .is_none()
    );
}

#[test]
fn same_cell_paths_still_keep_the_goal_waypoint() {
    let navigator = MapNavigator::new(test_grid(4, 4, &[]));
    let path = navigator
        .find_path(LocalPos::new(1.1, 1.1), LocalPos::new(1.8, 1.7))
        .expect("path within one cell");

    assert_eq!(
        path.waypoints,
        vec![LocalPos::new(1.1, 1.1), LocalPos::new(1.8, 1.7)]
    );
}

#[test]
fn default_planner_rejects_far_blocked_goals_outside_scope() {
    let navigator = MapNavigator::new(test_grid(64, 3, &[(10, 1)]));
    let start = LocalPos::new(0.5, 1.5);
    let goal = LocalPos::new(30.5, 1.5);

    assert_eq!(navigator.next_waypoint(start, goal), None);
    assert!(navigator.find_path(start, goal).is_none());
}

#[test]
fn find_path_and_next_waypoint_stay_consistent_for_local_goal() {
    let navigator = MapNavigator::new(test_grid(9, 7, &[(3, 1), (3, 2), (3, 3), (4, 3), (5, 3)]));
    let start = LocalPos::new(1.5, 1.5);
    let goal = LocalPos::new(6.5, 4.5);
    let path = navigator.find_path(start, goal).expect("local path");

    assert_eq!(path.waypoints.first().copied(), Some(start));
    assert_eq!(path.waypoints.last().copied(), Some(goal));
    assert_eq!(
        navigator.next_waypoint(start, goal),
        path.waypoints
            .get(1)
            .copied()
            .or_else(|| path.waypoints.last().copied())
    );
}

#[test]
fn out_of_bounds_endpoints_return_none() {
    let navigator = MapNavigator::new(test_grid(4, 4, &[]));

    assert!(
        navigator
            .find_path(LocalPos::new(-1.0, 0.0), LocalPos::new(1.0, 1.0))
            .is_none()
    );
    assert_eq!(
        navigator.next_waypoint(LocalPos::new(-1.0, 0.0), LocalPos::new(1.0, 1.0)),
        None
    );
}

#[test]
fn direct_routes_return_consistent_path_and_waypoint_queries() {
    let navigator = MapNavigator::new(test_grid(4, 1, &[]));
    let start = LocalPos::new(0.1, 0.1);
    let goal = LocalPos::new(3.9, 0.1);

    assert_eq!(
        navigator.find_path(start, goal).map(|path| path.waypoints),
        Some(vec![start, goal])
    );
    assert_eq!(navigator.next_waypoint(start, goal), Some(goal));
}

#[test]
fn far_direct_routes_are_not_rejected_by_local_scope() {
    let navigator = MapNavigator::new(test_grid(64, 3, &[]));
    let start = LocalPos::new(0.5, 1.5);
    let goal = LocalPos::new(30.5, 1.5);

    assert_eq!(
        navigator.find_path(start, goal).map(|path| path.waypoints),
        Some(vec![start, goal])
    );
    assert_eq!(navigator.next_waypoint(start, goal), Some(goal));
}

#[test]
fn public_queries_fail_consistently_for_unreachable_routes() {
    let disconnected = MapNavigator::new(test_grid(3, 3, &[(1, 0), (1, 1), (1, 2)]));
    let start = LocalPos::new(0.5, 1.5);
    let goal = LocalPos::new(2.5, 1.5);

    assert!(!disconnected.same_component(start, goal));
    assert!(disconnected.find_path(start, goal).is_none());
    assert_eq!(disconnected.next_waypoint(start, goal), None);
}

#[test]
fn public_queries_fail_consistently_for_invalid_endpoints() {
    let navigator = MapNavigator::new(test_grid(4, 4, &[]));
    let start = LocalPos::new(-1.0, 0.0);
    let goal = LocalPos::new(1.0, 1.0);

    assert!(!navigator.same_component(start, goal));
    assert!(navigator.find_path(start, goal).is_none());
    assert_eq!(navigator.next_waypoint(start, goal), None);
}
