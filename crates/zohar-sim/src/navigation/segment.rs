//! 2D implementation of the traversal algorithm derived in Amanatides/Woo,
//! "A Fast Voxel Traversal Algorithm for Ray Tracing" (1987).
//!
//! We use the traversal both to answer "is this segment clear?" and to report
//! the parametric entry time into the first blocked cell for clipping.
//! Unlike the paper's full ray traversal, this bounded helper requires the
//! segment endpoints to already lie inside the grid; it does not compute an
//! entry point from outside the grid.

use zohar_domain::coords::LocalPos;

use super::grid::{GridCell, TerrainFlagsGrid};
use super::reachability::WalkabilityView;

const CLIP_BACKOFF_EPSILON_M: f32 = 0.001;

pub(crate) fn can_stand(walkability: &WalkabilityView, pos: LocalPos) -> bool {
    walkability
        .terrain()
        .cell_for_point(pos)
        .is_some_and(|cell| walkability.is_walkable_cell(cell))
}

pub(crate) fn segment_clear(
    walkability: &WalkabilityView,
    start: LocalPos,
    goal: LocalPos,
) -> bool {
    matches!(
        walk_segment(walkability.terrain(), start, goal, |cell| {
            walkability.is_walkable_cell(cell)
        }),
        Some(SegmentTrace::Complete)
    )
}

pub(crate) fn clip_segment(
    walkability: &WalkabilityView,
    start: LocalPos,
    goal: LocalPos,
) -> LocalPos {
    match walk_segment(walkability.terrain(), start, goal, |cell| {
        walkability.is_walkable_cell(cell)
    }) {
        Some(SegmentTrace::Complete) => goal,
        Some(SegmentTrace::Blocked { enter_t, .. }) => {
            let dx = goal.x - start.x;
            let dy = goal.y - start.y;
            let segment_len = dx.hypot(dy);
            if !segment_len.is_finite() || segment_len <= f32::EPSILON || enter_t <= 0.0 {
                return start;
            }

            let backoff_t = (CLIP_BACKOFF_EPSILON_M / segment_len).min(enter_t);
            let clipped_t = (enter_t - backoff_t).max(0.0);
            LocalPos::new(start.x + dx * clipped_t, start.y + dy * clipped_t)
        }
        None => start,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SegmentTrace {
    Complete,
    Blocked { cell: GridCell, enter_t: f32 },
}

#[derive(Debug, Clone, Copy)]
struct GridCursor {
    x: isize,
    y: isize,
}

impl GridCursor {
    fn from_cell(cell: GridCell) -> Self {
        Self {
            x: cell.x as isize,
            y: cell.y as isize,
        }
    }

    fn to_cell(self) -> GridCell {
        debug_assert!(self.x >= 0 && self.y >= 0);
        GridCell {
            x: self.x as usize,
            y: self.y as usize,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum StepKind {
    X,
    Y,
    Corner,
}

#[derive(Debug)]
struct SegmentTraversal<'a> {
    terrain: &'a TerrainFlagsGrid,
    // The current voxel coordinates (`X`, `Y` in the paper).
    current: GridCursor,
    // The voxel containing the segment endpoint.
    target: GridCursor,
    // `stepX` and `stepY`: the sign of the segment direction on each axis.
    // The paper describes them as 1 or -1; this bounded segment variant also
    // permits 0 for an axis-aligned segment.
    step_x: isize,
    step_y: isize,
    // `tMaxX` and `tMaxY`: the value of `t` at which the segment next crosses a
    // vertical or horizontal voxel boundary.
    t_max_x: f32,
    t_max_y: f32,
    // `tDeltaX` and `tDeltaY`: how far along the segment we must move (in units
    // of `t`) for the horizontal or vertical component of that movement to
    // equal one voxel width/height. For an axis with no motion, this is
    // positive infinity so that axis is never selected as the minimum.
    t_delta_x: f32,
    t_delta_y: f32,
    // The `t` value at which we most recently entered `current`.
    last_enter_t: f32,
}

impl<'a> SegmentTraversal<'a> {
    // Initialization phase corresponding to Amanatides/Woo's "The New Traversal Algorithm".
    fn new(terrain: &'a TerrainFlagsGrid, start: LocalPos, goal: LocalPos) -> Option<Self> {
        let start_cell = terrain.cell_for_point(start)?;
        let goal_cell = terrain.cell_for_point(goal)?;

        let dx = goal.x - start.x;
        let dy = goal.y - start.y;
        let step_x = step_direction(dx);
        let step_y = step_direction(dy);

        let current = GridCursor::from_cell(start_cell);
        let first_boundary_x = first_boundary(current.x, step_x, terrain.cell_size_m());
        let first_boundary_y = first_boundary(current.y, step_y, terrain.cell_size_m());

        Some(Self {
            terrain,
            current,
            target: GridCursor::from_cell(goal_cell),
            step_x,
            step_y,
            t_max_x: initial_boundary_t(first_boundary_x, start.x, dx, step_x),
            t_max_y: initial_boundary_t(first_boundary_y, start.y, dy, step_y),
            t_delta_x: boundary_step_t(terrain.cell_size_m(), dx, step_x),
            t_delta_y: boundary_step_t(terrain.cell_size_m(), dy, step_y),
            last_enter_t: 0.0,
        })
    }

    fn trace(mut self, mut allow_cell: impl FnMut(GridCell) -> bool) -> SegmentTrace {
        let start_cell = self.current_cell();
        if !allow_cell(start_cell) {
            return SegmentTrace::Blocked {
                cell: start_cell,
                enter_t: 0.0,
            };
        }

        if start_cell == self.goal_cell() {
            return SegmentTrace::Complete;
        }

        // Incremental traversal phase corresponding to the paper's inner loop:
        // advance to the next boundary, update `X`/`Y`, and then apply this
        // module's walkability test to the voxel(s) entered at that `t`.
        while !self.reached_target() {
            let previous = self.current;
            match self.step() {
                StepKind::X | StepKind::Y => {
                    let cell = self.current_cell();
                    if !allow_cell(cell) {
                        return SegmentTrace::Blocked {
                            cell,
                            enter_t: self.last_enter_t,
                        };
                    }
                }
                StepKind::Corner => {
                    // The paper's 2D pseudocode chooses one axis by `<`/`else`.
                    // For collision queries we need a stricter tie case:
                    // when `tMaxX == tMaxY`, the segment passes exactly through
                    // a grid corner, so both newly entered adjacent cells must
                    // be checked to avoid missing a blocker.
                    let x_cell = GridCell {
                        x: self.current.x as usize,
                        y: previous.y as usize,
                    };
                    let y_cell = self.current_cell();
                    if !allow_cell(x_cell) {
                        return SegmentTrace::Blocked {
                            cell: x_cell,
                            enter_t: self.last_enter_t,
                        };
                    }
                    if !allow_cell(y_cell) {
                        return SegmentTrace::Blocked {
                            cell: y_cell,
                            enter_t: self.last_enter_t,
                        };
                    }
                }
            }
        }

        SegmentTrace::Complete
    }

    fn reached_target(&self) -> bool {
        self.current.x == self.target.x && self.current.y == self.target.y
    }

    fn goal_cell(&self) -> GridCell {
        self.target.to_cell()
    }

    // Determine which `tMax*` is minimal and advance to the next voxel in the
    // same style as the paper's inner loop, with an explicit tie case for
    // corner crossings.
    #[inline(always)]
    fn step(&mut self) -> StepKind {
        if self.t_max_x < self.t_max_y {
            self.last_enter_t = self.t_max_x;
            self.current.x += self.step_x;
            self.t_max_x += self.t_delta_x;
            StepKind::X
        } else if self.t_max_y < self.t_max_x {
            self.last_enter_t = self.t_max_y;
            self.current.y += self.step_y;
            self.t_max_y += self.t_delta_y;
            StepKind::Y
        } else {
            self.last_enter_t = self.t_max_x;
            self.current.x += self.step_x;
            self.current.y += self.step_y;
            self.t_max_x += self.t_delta_x;
            self.t_max_y += self.t_delta_y;
            StepKind::Corner
        }
    }

    fn current_cell(&self) -> GridCell {
        let cell = self.current.to_cell();
        debug_assert!(self.terrain.contains_cell(cell));
        cell
    }
}

pub(crate) fn walk_segment(
    terrain: &TerrainFlagsGrid,
    start: LocalPos,
    goal: LocalPos,
    allow_cell: impl FnMut(GridCell) -> bool,
) -> Option<SegmentTrace> {
    Some(SegmentTraversal::new(terrain, start, goal)?.trace(allow_cell))
}

fn step_direction(delta: f32) -> isize {
    if delta > 0.0 {
        1
    } else if delta < 0.0 {
        -1
    } else {
        0
    }
}

// Compute the first voxel boundary on this axis that will be crossed while
// stepping in `step`.
fn first_boundary(cell: isize, step: isize, cell_size_m: f32) -> f32 {
    let boundary_cell = if step > 0 { cell + 1 } else { cell };
    boundary_cell as f32 * cell_size_m
}

// Compute `tMax*`: the value of `t` at which the segment first crosses the next
// voxel boundary on this axis.
fn initial_boundary_t(boundary_m: f32, start_coord_m: f32, delta_m: f32, step: isize) -> f32 {
    if step == 0 {
        f32::INFINITY
    } else {
        (boundary_m - start_coord_m) / delta_m
    }
}

// Compute `tDelta*`: the amount added to `tMax*` after each crossed boundary on
// this axis.
fn boundary_step_t(cell_size_m: f32, delta_m: f32, step: isize) -> f32 {
    if step == 0 {
        f32::INFINITY
    } else {
        cell_size_m / delta_m.abs()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zohar_domain::TerrainFlags;
    use zohar_domain::coords::LocalPos;

    use super::{SegmentTrace, can_stand, clip_segment, segment_clear, walk_segment};
    use crate::navigation::TerrainFlagsGrid;
    use crate::navigation::reachability::WalkabilityView;

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
    fn point_queries_require_valid_walkable_cells() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(2, 2, &[(1, 1)])));

        assert!(can_stand(&walkability, LocalPos::new(0.1, 0.1)));
        assert!(!can_stand(&walkability, LocalPos::new(1.1, 1.1)));
        assert!(!can_stand(&walkability, LocalPos::new(-1.0, 0.0)));
    }

    #[test]
    fn traversal_catches_intermediate_blockers() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(6, 4, &[(2, 0)])));

        assert!(!segment_clear(
            &walkability,
            LocalPos::new(0.1, 0.1),
            LocalPos::new(5.9, 0.1),
        ));
    }

    #[test]
    fn traversal_allows_vertical_clear_segments() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(3, 5, &[])));

        assert!(segment_clear(
            &walkability,
            LocalPos::new(1.5, 0.1),
            LocalPos::new(1.5, 4.9),
        ));
    }

    #[test]
    fn traversal_catches_stair_step_diagonal_blockers() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(5, 5, &[(2, 2)])));

        assert!(!segment_clear(
            &walkability,
            LocalPos::new(0.1, 0.1),
            LocalPos::new(4.9, 4.9),
        ));
    }

    #[test]
    fn traversal_allows_zero_length_segments() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(4, 4, &[])));
        let pos = LocalPos::new(1.2, 1.8);

        assert!(segment_clear(&walkability, pos, pos));
    }

    #[test]
    fn traversal_reports_blocked_start_cell() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(3, 3, &[(0, 0)])));

        assert_eq!(
            walk_segment(
                walkability.terrain(),
                LocalPos::new(0.1, 0.1),
                LocalPos::new(2.9, 0.1),
                |cell| walkability.is_walkable_cell(cell),
            ),
            Some(SegmentTrace::Blocked {
                cell: crate::navigation::GridCell { x: 0, y: 0 },
                enter_t: 0.0,
            })
        );
    }

    #[test]
    fn traversal_checks_both_cells_when_crossing_a_corner() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(3, 3, &[(1, 0)])));

        // Amanatides/Woo's simultaneous-boundary case: the ray passes exactly
        // through the corner at (1, 1), so both entered cells must be tested.
        assert!(!segment_clear(
            &walkability,
            LocalPos::new(0.5, 0.5),
            LocalPos::new(2.5, 2.5),
        ));
    }

    #[test]
    fn clipping_returns_last_clear_point_before_blocker() {
        let walkability = WalkabilityView::new(Arc::new(test_grid(4, 1, &[(2, 0)])));
        let clipped = clip_segment(
            &walkability,
            LocalPos::new(0.1, 0.1),
            LocalPos::new(3.9, 0.1),
        );

        assert!(
            clipped.x < 2.0,
            "clipped point should stay before the blocked cell"
        );
        assert!(
            clipped.x > 1.0,
            "clipped point should remain inside the last clear cell"
        );
        assert_eq!(clipped.y, 0.1);
    }
}
