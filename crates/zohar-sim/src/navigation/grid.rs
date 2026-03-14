use zohar_domain::TerrainFlags;
use zohar_domain::coords::LocalPos;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell {
    pub x: usize,
    pub y: usize,
}

impl GridCell {
    pub(crate) fn manhattan_distance(self, other: Self) -> usize {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }
}

#[derive(Debug)]
pub struct TerrainFlagsGrid {
    cell_size_m: f32,
    pub(crate) inv_cell_size_m: f32,
    width: usize,
    height: usize,
    flags: Box<[TerrainFlags]>,
}

impl TerrainFlagsGrid {
    /// Construct a terrain grid from row-major terrain flags.
    pub fn new(
        cell_size_m: f32,
        width: usize,
        height: usize,
        flags: impl Into<Box<[TerrainFlags]>>,
    ) -> Option<Self> {
        if !cell_size_m.is_finite() || cell_size_m <= 0.0 || width == 0 || height == 0 {
            return None;
        }

        let flags = flags.into();
        if flags.len() != width.checked_mul(height)? {
            return None;
        }

        Some(Self {
            cell_size_m,
            inv_cell_size_m: cell_size_m.recip(),
            width,
            height,
            flags,
        })
    }

    pub fn cell_size_m(&self) -> f32 {
        self.cell_size_m
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn cell_for_point(&self, pos: LocalPos) -> Option<GridCell> {
        Some(GridCell {
            x: self.axis_cell(pos.x, self.width)?,
            y: self.axis_cell(pos.y, self.height)?,
        })
    }

    pub fn cell_center(&self, cell: GridCell) -> Option<LocalPos> {
        self.contains_cell(cell).then(|| {
            LocalPos::new(
                (cell.x as f32 + 0.5) * self.cell_size_m,
                (cell.y as f32 + 0.5) * self.cell_size_m,
            )
        })
    }

    pub fn contains_cell(&self, cell: GridCell) -> bool {
        cell.x < self.width && cell.y < self.height
    }

    pub fn flags_at(&self, cell: GridCell) -> Option<TerrainFlags> {
        self.index_of(cell).map(|idx| self.flags[idx])
    }

    pub(crate) fn index_of(&self, cell: GridCell) -> Option<usize> {
        self.contains_cell(cell)
            .then_some(cell.y * self.width + cell.x)
    }

    pub(crate) fn all_cells(&self) -> impl Iterator<Item = GridCell> + '_ {
        (0..self.height).flat_map(|y| (0..self.width).map(move |x| GridCell { x, y }))
    }

    pub(crate) fn neighbors4(&self, cell: GridCell) -> impl Iterator<Item = GridCell> + '_ {
        [
            cell.x.checked_sub(1).map(|x| GridCell { x, y: cell.y }),
            cell.x.checked_add(1).map(|x| GridCell { x, y: cell.y }),
            cell.y.checked_sub(1).map(|y| GridCell { x: cell.x, y }),
            cell.y.checked_add(1).map(|y| GridCell { x: cell.x, y }),
        ]
        .into_iter()
        .flatten()
        .filter(|&neighbor| self.contains_cell(neighbor))
    }

    pub(crate) fn search_bounds(&self, center: GridCell, radius_cells: usize) -> GridBounds {
        GridBounds {
            min_x: center.x.saturating_sub(radius_cells),
            max_x: center.x.saturating_add(radius_cells).min(self.width - 1),
            min_y: center.y.saturating_sub(radius_cells),
            max_y: center.y.saturating_add(radius_cells).min(self.height - 1),
        }
    }

    fn axis_cell(&self, coord_m: f32, limit: usize) -> Option<usize> {
        if !coord_m.is_finite() || coord_m < 0.0 {
            return None;
        }

        let cell = (coord_m * self.inv_cell_size_m).floor();
        if !cell.is_finite() || cell < 0.0 {
            return None;
        }

        let cell = cell as usize;
        (cell < limit).then_some(cell)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GridBounds {
    min_x: usize,
    max_x: usize,
    min_y: usize,
    max_y: usize,
}

impl GridBounds {
    pub(crate) fn contains(self, cell: GridCell) -> bool {
        cell.x >= self.min_x && cell.x <= self.max_x && cell.y >= self.min_y && cell.y <= self.max_y
    }
}

#[cfg(test)]
mod tests {
    use super::TerrainFlagsGrid;
    use zohar_domain::TerrainFlags;

    #[test]
    fn invalid_construction_is_rejected() {
        assert!(TerrainFlagsGrid::new(0.5, 2, 2, vec![TerrainFlags::BLOCK; 3]).is_none());
        assert!(TerrainFlagsGrid::new(0.0, 2, 2, vec![TerrainFlags::BLOCK; 4]).is_none());
    }
}
